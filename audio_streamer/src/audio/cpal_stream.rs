use std::{sync::{atomic::{AtomicBool, Ordering}, Arc}, thread::{self, JoinHandle}};

use cpal::{traits::{DeviceTrait, StreamTrait}, Device as CpalDevice, Stream, SupportedStreamConfig};
use crossbeam_channel::{Receiver, bounded};
use event_listener::{Event, Listener};

use crate::utils::ring_buffer::{ring_buffer, Producer};

type AudioFrame = Arc<[f32; 1920]>;

#[derive(Debug, thiserror::Error)]
pub enum CpalStreamError {
    #[error("Failed to get default output device for cpal stream.")]
    DefaultDeviceNotFound(),

    #[error("Failed to initialize config for default device: {0}")]
    DefaultDeviceConfigNotFound(#[from] cpal::DefaultStreamConfigError),

    #[error("<{0}> is unsupported sample rate.")]
    InvalidSampleRate(u32),

    #[error("<{0}> is unsupported sample format.")]
    InvalidSampleFormat(cpal::SampleFormat),

    #[error("Failed to build the stream: {0}")]
    FailedToBuildStream(#[from] cpal::BuildStreamError),

    #[error("Faile to start the stream: {0}")]
    FailedToStartStream(#[from] cpal::PlayStreamError),

    #[error("Cpal stream thread paniced.")]
    ThreadPanicError()
}

fn start_cpal_stream<const N: usize>(mut rb_producer: Producer<N>, device: CpalDevice, config: SupportedStreamConfig, event: Arc<Event>) -> Result<Stream, CpalStreamError> {
    // Spins the Producer's (P) thread.
    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let pushed = rb_producer.push_slice(data);
            // Rationale:
            // Happy path - this will evaluates True 99% of the time, CPU branch prediction optimize it away
            // Unhappy path - we will lose some speed, but then we would need to change buffer len anyway
            if pushed < data.len() {
                tracing::warn!(
                    lost_samples = data.len() - pushed,
                    "Ring buffer is full. Data loss has occurred."
                );
            }

            // again, happy path, lets praise branch prediction works.
            if pushed > 0 {
                event.notify(1);
            }
        },
        |err| { tracing::error!("An audio stream error occurred: {}", err); },
        None
    )?;

    stream.play()?;

    Ok(stream)
}

pub struct CpalStream {
    device: CpalDevice,
    config: SupportedStreamConfig,
    running: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
}

impl CpalStream {
    pub fn new(device: CpalDevice, config: SupportedStreamConfig) -> Self {
        Self {
            device,
            config,
            running: Arc::new(AtomicBool::new(false)),
            worker_handle: None
        }
    }

    pub fn start(&mut self) -> Result<Receiver<AudioFrame>, CpalStreamError> {
        let (mut producer, mut consumer) = ring_buffer::<4096>();
        let (sender, broadcasters_receiver) = bounded::<AudioFrame>(10);

        let event = Arc::new(Event::new());

        let device = self.device.clone();
        let config = self.config.clone();
        let running = self.running.clone();
        
        self.worker_handle = Some(thread::spawn(move || {
            let cpal_stream_res = start_cpal_stream(producer, device, config, event.clone());

            if let Err(e) = cpal_stream_res {
                // the caller might want to handle the case when cpal_stream failed to start
                tracing::warn!("Error starting cpal stream: {}", e);
                return;
            }

            running.store(true, Ordering::Release);

            // ============ FRAMING PCMs LOGIC ===================

            const FRAME_SIZE: usize = 1920;
            let mut frame_buffer = [0.0f32; FRAME_SIZE];

            while running.load(Ordering::Acquire) {
                let mut popped_so_far = 0;
                
                while popped_so_far < FRAME_SIZE {
                    let amount = consumer.pop_slice(&mut frame_buffer[popped_so_far..]);
                    
                    if amount > 0 {
                        popped_so_far += amount;
                        continue;
                    }
                    
                    let listener = event.listen();

                    // double check if data arrives, "lost wakeup" case
                    let amount = consumer.pop_slice(&mut frame_buffer[popped_so_far..]);

                    if amount > 0 {
                        popped_so_far += amount;
                        continue;
                    }

                    // check for the stop flag before parking
                    if !running.load(Ordering::Acquire) {
                        break;
                    }

                    listener.wait();
                }

                // check for the stop flag after parking
                if !running.load(Ordering::Acquire) {
                    break;
                }
                
                let frame = Arc::new(frame_buffer);
                
                // If receiver is dropped, stop the engine
                if let Err(e) = sender.send(frame) {
                    tracing::info!("Audio frame receiver dropped: {}, broadcaster has died, stopping engine.", e);
                    running.store(false, Ordering::Release);
                    break;
                }
            }
        }));

        Ok(broadcasters_receiver)
    }

    pub fn stop(&mut self) -> Result<(), CpalStreamError> {
        self.running.store(false, Ordering::Release);
        
        if let Some(handle) = self.worker_handle.take() {
            handle.join().map_err(|_| CpalStreamError::ThreadPanicError())?;
        }
        
        Ok(())
    }
    
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

/* 
Cpal callbacks (Thread 1) --[unframed PCMs]-> Ring Buffer [unframed PCMs] --[unframed PCMs]-> Engine worker framing pcms (Thread 2) --[Framed PCMs]-> Channel to Broadcaster
*/