// use std::collections::{HashMap};
// use std::io::{Error, IoSlice, Write};
// use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener as StdTcpListener, TcpStream as StdTcpStream};
// use std::str::Utf8Error;
// use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
// use std::sync::{Arc};
// use std::thread::JoinHandle;
// use std::time::Duration;
// use std::{thread};
// use audio_streamer::server::comm_protocol::{Command, ErrorPayload, Response, SuccessPayload};
// use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
// use cpal::{Device, Stream, SupportedStreamConfig};
// use serde::{Deserialize, Serialize};
// use serde_json::json;
// use thiserror;
// use anyhow::anyhow;
// use opusic_c::{self, Encoder};
// use tokio::sync::mpsc;
// use tokio::net::{TcpListener, TcpStream};
// use tokio::io::{AsyncWriteExt, BufReader, AsyncBufReadExt};
// use tokio::sync::{Mutex};
// use tracing;
// use tracing_subscriber;
// use crossbeam_channel::{bounded, Sender, Receiver};
// use dashmap::DashMap;

// use audio_streamer::utils::ring_buffer::{ring_buffer, Consumer, Producer};
// use uuid::Uuid;

// use audio_streamer::audio::{cpal_stream::AudioEngine, broadcasting::Broadcaster};

// #[derive(Debug, thiserror::Error)]
// enum ConnectionError {

//     #[error("Error retrieving address: {0}")]
//     AddressRetrievalError(Error),

//     #[error("Error bindig TCP listenr to the port: {0}")]
//     TcpListenerBindingError(Error),

//     #[error(transparent)]
//     ClientHandlingError(#[from] AudioRecordingError)
// }

// #[derive(Debug, thiserror::Error)]
// enum AudioRecordingError {
//     #[error("Failed to get default output device.")]
//     DefaultOutputDeviceError(),

//     #[error(transparent)]
//     DeviceNameError(#[from] cpal::DeviceNameError),

//     #[error(transparent)]
//     DefaultStreamConfigError(#[from] cpal::DefaultStreamConfigError),

//     #[error(transparent)]
//     BuildStreamError(#[from] cpal::BuildStreamError),

//     #[error(transparent)]
//     PlayStreamError(#[from] cpal::PlayStreamError),

//     #[error(transparent)]
//     IOError(#[from] std::io::Error),

//     #[error(transparent)]
//     SupportedStreamConfigsError(#[from] cpal::SupportedStreamConfigsError),

//     #[error("Default device has {0} channels. Only Mono and Stereo are supported.")]
//     DefaultDeviceConfigChannelsError(u16),

//     #[error("Default device has {0} sample rate. Only 48000 is supported.")]
//     DefaultDeviceConfigSampleRateError(u32),

//     #[error("Failed to init Opus encoder: {0}")]
//     OpusEncoderError(String),

//     #[error(transparent)]
//     DevicesError(#[from] cpal::DevicesError)
// }

// const RING_BUFFER_SIZE: usize = 8192;



// #[derive(Serialize, Deserialize, Debug, Clone)]
// struct StreamMetadata {
//     sample_rate: u32,
//     channels: u16,
//     format: String
// }

// impl StreamMetadata {
//     fn from_cpal_config(config: &SupportedStreamConfig) -> Self {
//         Self {
//             sample_rate: config.sample_rate().0,
//             channels: config.channels(),
//             format: config.sample_format().to_string()
//         }
//     }
// }

// fn start_cpal_stream<const N: usize>(mut rb_producer: Producer<N>, device: Device, config: SupportedStreamConfig) -> Result<Stream, CpalStreamError> {
//     // Spins the Producer's (P) thread.
//     let stream = device.build_input_stream(
//         &config.into(),
//         move |data: &[f32], _: &cpal::InputCallbackInfo| {
//             let pushed = rb_producer.push_slice(data);
//             // Rationale:
//             // Happy path - this will evaluates True 99% of the time, CPU branch prediction optimize it away
//             // Unhappy path - we will lose some speed, but then we would need to change buffer len anyway
//             if pushed < data.len() {
//                 tracing::warn!(
//                     lost_samples = data.len() - pushed,
//                     "Ring buffer is full. Data loss has occurred."
//                 );
//             }
//         },

//         |err| {
//             tracing::error!("An audio stream error occurred: {}", err);
//         },

//         // Timeouts are for blocking operations
//         None
//     )?;

//     stream.play()?;


//     Ok(stream)
// }

// #[derive(Debug, thiserror::Error)]
// enum EncoderSetupError {
//     #[error("<{0}> channels are unsupported.")]
//     InvalidChannels(u16),

//     #[error("<{0}> is unsupported sample rate.")]
//     InvalidSampleRate(u32),

//     #[error("Failed to build encoder: {0}")]
//     FailedToBuildEncoder(String)
// }

// fn create_encoder(metadata: &StreamMetadata) -> Result<Encoder, EncoderSetupError> {
//     let (application, channels, sample_rate) = (
//         opusic_c::Application::Audio,
//         match metadata.channels {
//             1u16 => opusic_c::Channels::Mono,
//             2u16 => opusic_c::Channels::Stereo,
//             rest => return Err(EncoderSetupError::InvalidChannels(rest))
//         },
//         match metadata.sample_rate {
//             48000u32 => opusic_c::SampleRate::Hz48000,
//             rest => return Err(EncoderSetupError::InvalidSampleRate(rest))
//         }
//     );
//     let encoder = opusic_c::Encoder::new(channels, sample_rate, application).map_err(|err| EncoderSetupError::FailedToBuildEncoder(err.message().to_string()))?;
//     return Ok(encoder);
// }

// #[derive(Serialize, Deserialize, Debug)]
// struct Handshake {
//     data_port: String,
//     comm_port: String,
//     stream_metadata: StreamMetadata
// }

// struct Client {
//     id: usize,
//     client_addr: SocketAddr
// }

// async fn greet_the_client(mut main_socket: TcpStream, stream_metadata: Arc<StreamMetadata>) -> Result<(TcpListener, StdTcpListener), anyhow::Error> {
//     let comm_listener = TcpListener::bind("0.0.0.0:0").await?;
//     let data_listener  = StdTcpListener::bind("0.0.0.0:0")?;

//     let comm_port = comm_listener.local_addr()?.port();
//     let data_port = data_listener.local_addr()?.port();

//     let handshake_json = json!({
//         "comm_port": format!("0.0.0.0:{}", comm_port),
//         "data_port": format!("0.0.0.0:{}", data_port),
//         "stream_metadata": &*stream_metadata

//     });

//     let message = serde_json::to_string(&handshake_json)? + "\n";
//     main_socket.write_all(message.as_bytes()).await?;

//     Ok((comm_listener, data_listener))
// }

// fn encoding_actor(mut data_socket: StdTcpStream, mut audio_rx: mpsc::Receiver<Arc<Vec<f32>>>, stream_metadata: Arc<StreamMetadata>) -> Result<(), anyhow::Error> {
//     let mut encoder = create_encoder(&stream_metadata)?;
//     let mut encoded_opus_frame: Vec<u8> = Vec::with_capacity(4000);

//     while let Some(audio_chunk) = audio_rx.blocking_recv() {
//         let amount = encoder.encode_float_to_vec(&audio_chunk, &mut encoded_opus_frame).expect("hello world!");
//         let header = (encoded_opus_frame.len() as u32).to_be_bytes();

//         let buf = [
//             IoSlice::new(&header),
//             IoSlice::new(&encoded_opus_frame[..amount])
//         ];
        
//         data_socket.write_vectored(&buf)?;

//         encoded_opus_frame.clear();
//     }

//     Ok(())
// }

// // async fn handle_client_session(mut comm_listener: TcpListener, std_data_listener: StdTcpListener, mut channel_rx: Receiver<Arc<Vec<f32>>>, stream_metadata: Arc<StreamMetadata>) -> Result<(), anyhow::Error> {
// //     // channel between broadcast (cpals PCMs for multiple clients) and encoding thread
// //     let (to_encoder_tx, to_encoder_rx) = mpsc::channel::<Arc<Vec<f32>>>(4);

// //     let (mut comm_socket, _) = comm_listener.accept().await?;
// //     let (mut std_data_socket, _) = std_data_listener.accept()?;


// //     let (mut comm_reader, _comm_writer) = comm_socket.into_split();

// //     let encoder_handle = tokio::task::spawn_blocking(move || {
// //         encoding_actor(std_data_socket, to_encoder_rx, stream_metadata)
// //     });

// //     loop {
// //         tokio::select! {
// //             result = channel_rx.recv() => {
// //                 match result {
// //                     Ok(audio_chunk) => {
// //                         if to_encoder_tx.send(audio_chunk).await.is_err() {
// //                             eprintln!("Encoding actor has died. Shutting down session.");
// //                             break;
// //                         }
// //                     },

// //                     Err(RecvError::Lagged(missed_count)) => {
// //                         // This is the lagged case.
// //                         eprintln!("Client lagged. {} audio frames were dropped.", missed_count);
// //                         // We just continue to the next loop iteration to try and catch up.
// //                         continue; 
// //                     },

// //                     Err(RecvError::Closed) => {
// //                         // This means the main audio engine has shut down.
// //                         // The sender was dropped, so no more messages will ever arrive.
// //                         eprintln!("Audio channel has closed. Shutting down session.");
// //                         break; // Break the select loop
// //                     }
// //                 }
// //             },
// //             Ok(message) = recv_message(&mut comm_reader) => {},
// //         }
// //     }

// //     drop(to_encoder_tx);
// //     let _encoder_handle_errors = encoder_handle.await;

// //     Ok(())
// // }

// type ClientId = Uuid;
// type Thread = f32;
// type AudioFrame = Arc<[f32; 1920]>;

// struct ResourceManager {
//     thread_pool: Vec<Thread>,
//     sockets_pool: Vec<TcpListener>,

//     clients: DashMap<ClientId, (Thread, SocketAddr)>
// }

// impl ResourceManager {
//     pub fn new(max: usize) -> Self {
//         Self {
//             thread_pool: Vec::with_capacity(max),
//             sockets_pool: Vec::with_capacity(max),
//             clients: DashMap::new()
//         }
//     }

//     pub fn issue(&mut self, client_id: ClientId) -> Result<(Thread, SocketAddr), anyhow::Error> {
//         Ok((42f32, SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)))
//     }

//     pub fn resign(&mut self, client_id: ClientId) -> () {
//         ()
//     }
// }


// async fn send_stream_metadata(mut socket: &TcpStream) -> Result<(), anyhow::Error> {


//     Ok(())
// }


// struct WorkersManager {
//     workers: DashMap<ClientId, JoinHandle<()>>,
//     resources: ResourceManager
// }

// impl WorkersManager {
//     pub fn new(max: usize) -> Self {
//         Self {
//             workers: DashMap::new(),
//             resources: ResourceManager::new(max)
//         }
//     }

//     pub fn spawn(&mut self, client_id: ClientId, tx: Receiver<AudioFrame>) -> Result<SocketAddr, anyhow::Error> {


//         Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080))
//     }

//     pub fn kill(&mut self, client_id: ClientId) -> Result<(), anyhow::Error> {

//         Ok(())
//     }

//     pub fn active(&self) -> Vec<ClientId> {

//         vec![Uuid::new_v4()]
//     }
// }

// async fn send(socket: TcpStream, payload: impl Serialize) -> () {

// }

// async fn recv_worker_error() -> ((), ()) {
//     ((), ())
// }


use audio_streamer::audio::{AudioPipeline, AudioPipelineError, WorkersManager, WorkersManagerError, StreamMetadata};
use audio_streamer::server::TcpServer;
use cpal::{traits::{DeviceTrait, HostTrait}, DefaultStreamConfigError, Device, SupportedStreamConfig};
use tokio::net::TcpListener;

#[derive(Debug, thiserror::Error)]
enum CpalStreamError {
    #[error("Failed to get default output device for cpal stream.")]
    DefaultDeviceNotFound(),

    #[error("<{0}> is unsupported sample rate.")]
    InvalidSampleRate(u32),

    #[error("<{0}> is unsupported sample format.")]
    InvalidSampleFormat(cpal::SampleFormat),

    #[error("Failed to get default config from a device: {0}")]
    DefaultStreamConfigError(#[from] DefaultStreamConfigError)
}

fn get_device_and_config() -> Result<(Device, SupportedStreamConfig), CpalStreamError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(CpalStreamError::DefaultDeviceNotFound())?;
    let config = device.default_output_config()?;

    let device_sample_rate = config.sample_rate();
    // Sample rate constraint coming from Opus encoders\decoders.
    if device_sample_rate != cpal::SampleRate(48000) {
        return Err(CpalStreamError::InvalidSampleRate(device_sample_rate.0))
    }

    let device_sample_format = config.sample_format();
    // Format constraint coming from Ring Buffer implementation.
    if device_sample_format != cpal::SampleFormat::F32 {
        return Err(CpalStreamError::InvalidSampleFormat(device_sample_format))
    }

    Ok((device, config))
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();

    let (device, config) = get_device_and_config()?;
    let metadata = StreamMetadata::from_cpal_config(&config);
    let pipeline = AudioPipeline::new(device, config);
    let manager = WorkersManager::new(pipeline, 10, metadata)?;

    let server = TcpServer::new("0.0.0.0:9000", manager).await?;
    server.run().await?;

    Ok(())
}