use std::{io::{ErrorKind, IoSlice, Write}, net::TcpListener as SyncTcpListener, sync::{atomic::{AtomicBool, Ordering}, Arc}, thread::{self, JoinHandle}, time::Duration};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use opusic_c::Encoder;
use tokio::sync::mpsc;

use super::stream_metadata::StreamMetadata;


type WorkerId = u16;
type AudioFrame = Arc<[f32; 1920]>;

#[derive(Debug, thiserror::Error)]
pub enum EncoderSetupError {
    #[error("<{0}> channels are unsupported.")]
    InvalidChannels(u16),

    #[error("<{0}> is unsupported sample rate.")]
    InvalidSampleRate(u32),

    #[error("Failed to build encoder: {0}")]
    FailedToBuildEncoder(String)
}

#[derive(Debug, thiserror::Error)]
pub enum EncoderWorkerError {

    #[error("Error accepting cleint via data socket: {0}")]
    DataSocketAcceptError(std::io::Error),

    #[error(transparent)]
    EncoderSetupError(#[from] EncoderSetupError),

    #[error("Error during encoding the frame: {0}")]
    FrameEncodingError(String),

    #[error("Error writing into the data socket: {0}")]
    DataSocketWriteError(std::io::Error),

    #[error("The worker (id=<{0}>) has being disconnected from the broadcaster.")]
    DisconnectedFromBroadcasterError(WorkerId),

    #[error("Error creating data socket: {0}")]
    TcpBindingError(std::io::Error),
    
    #[error("Failed to set listener to non blocking mode: {0}")]
    SetNoneblockingError(std::io::Error),

    #[error("No client connected for the 30 sec.")]
    AcceptTimeoutError(),

    #[error("Failet to set socket to be non blocking: {0}")]
    SetWriteTimeout(std::io::Error)


}

#[derive(Debug)]
pub struct WorkerMessage {
    pub id: WorkerId,
    pub event: WorkerEvent,
}

impl WorkerMessage {
    pub fn new(id: WorkerId, event: WorkerEvent) -> Self {
        Self {
            id,
            event
        }
    }
}

#[derive(Debug)]
pub enum WorkerEvent {
    StoppedWithError(EncoderWorkerError),
}

pub struct EncoderWorker {
    id: WorkerId,
    port: u16,
    frames_receiver: Receiver<AudioFrame>,
    error_sender: mpsc::Sender<WorkerMessage>,
    stop_signal: Arc<AtomicBool>,
    stream_metadata: StreamMetadata
}

impl EncoderWorker {
    pub fn new(id: u16, port: u16, frames_receiver: Receiver<AudioFrame>, error_sender: mpsc::Sender<WorkerMessage>, stream_metadata: StreamMetadata, stop_signal: Arc<AtomicBool>) -> Self {
        Self {
            id,
            port,
            frames_receiver,
            error_sender,
            stop_signal,
            stream_metadata
        }
    }

    pub fn run(self) -> JoinHandle<()> {
        thread::spawn(move || {
            let run_result = self.run_internal();

            if let Err(e) = run_result {
                // pretty much every error is unrecoverable
                let msg = WorkerMessage::new(self.id, WorkerEvent::StoppedWithError(e));

                if self.error_sender.blocking_send(msg).is_err() {
                        tracing::error!("Fatal: could not send error message from worker {}. Manager is gone?", self.id);
                }
            }
        })
    }

    fn create_encoder(&self) -> Result<Encoder, EncoderSetupError> {
        let (application, channels, sample_rate) = (
            opusic_c::Application::Audio,
            match self.stream_metadata.channels {
                1u16 => opusic_c::Channels::Mono,
                2u16 => opusic_c::Channels::Stereo,
                rest => return Err(EncoderSetupError::InvalidChannels(rest))
            },
            match self.stream_metadata.sample_rate {
                48000u32 => opusic_c::SampleRate::Hz48000,
                rest => return Err(EncoderSetupError::InvalidSampleRate(rest))
            }
        );

        opusic_c::Encoder::new(channels, sample_rate, application)
            .map_err(|err| EncoderSetupError::FailedToBuildEncoder(err.message().to_string()))
    }

    fn run_internal(&self) -> Result<(), EncoderWorkerError> {
        let sync_tcp_listener = SyncTcpListener::bind(format!("0.0.0.0:{}", self.port)).map_err(|e| EncoderWorkerError::TcpBindingError(e))?;

        // for the case if python crush between requesting the worker and connecting to the port
        sync_tcp_listener.set_nonblocking(true).map_err(EncoderWorkerError::SetNoneblockingError)?;
        let accept_deadline = std::time::Instant::now() + Duration::from_secs(30);

        let mut sync_data_socket = loop {
            if self.stop_signal.load(Ordering::Relaxed) {
                tracing::info!("Worker {} stopped before client connected", self.id);
                return Ok(());
            }

            if std::time::Instant::now() > accept_deadline {
                return Err(EncoderWorkerError::AcceptTimeoutError());
            }

            match sync_tcp_listener.accept() {
                Ok((socket, addr)) => {
                    tracing::info!("Worker {} accepted connection from {}", self.id, addr);
                    break socket;
                },

                Err(e) => {
                    if e.kind() == ErrorKind::WouldBlock {
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }

                    return Err(EncoderWorkerError::DataSocketAcceptError(e));
                }
            }

        };

        // for the same case if python crashes somewhere
        sync_data_socket.set_write_timeout(Some(Duration::from_secs(5))).map_err(EncoderWorkerError::SetWriteTimeout)?;

        let mut encoder = self.create_encoder().map_err(|e| EncoderWorkerError::EncoderSetupError(e))?;
        let mut opus_buffer = Vec::with_capacity(4000);

        loop {
            if self.stop_signal.load(Ordering::Relaxed) {
                tracing::info!("Worker {} received stop signal. Shutting down.", self.id);
                return Ok(());
            }

            // timeouts are for the stop signal checking; 
            // without timeout thread might hang and we couldn't stop it
            match self.frames_receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(frame) => {
                    let amount = encoder.encode_float_to_vec(&*frame, &mut opus_buffer).map_err(|e| EncoderWorkerError::FrameEncodingError(e.message().to_string()))?;
                    let header = (opus_buffer.len() as u32).to_be_bytes();

                    // avoids extra allocs and copies
                    let buf = [
                        IoSlice::new(&header),
                        IoSlice::new(&opus_buffer[..amount])
                    ];

                        sync_data_socket.write_vectored(&buf).map_err(|e| {
                            if e.kind() == ErrorKind::TimedOut {
                                tracing::warn!("Worker {} write timeout - Python might be hung", self.id);
                            }

                            EncoderWorkerError::DataSocketWriteError(e)
                        })?;
                        
                        opus_buffer.clear();
                    },

                    Err(RecvTimeoutError::Timeout) => { continue; },
                    Err(RecvTimeoutError::Disconnected) => {
                        tracing::warn!("Worker {} disconnected from broadcaster. Shutting down.", self.id);
                        return Err(EncoderWorkerError::DisconnectedFromBroadcasterError(self.id));
                    }
            }
        }
    }
}