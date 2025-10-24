use std::sync::Arc;

use cpal::{Device, SupportedStreamConfig};
use crossbeam_channel::{Receiver};

use crate::audio::{broadcasting::{Broadcaster, BroadcasterError}, cpal_stream::{CpalStream, CpalStreamError}};

type WorkerID = u16;
type AudioFrame = Arc<[f32; 1920]>;

#[derive(Debug, thiserror::Error)]
pub enum AudioPipelineError {
    
    #[error(transparent)]
    CpalStreamError(#[from] CpalStreamError),

    #[error(transparent)]
    BroadcasterError(#[from] BroadcasterError),

    #[error("Pipeline is stopped.")]
    NotRunning()
}

#[derive(Debug, PartialEq)]
enum PipelineState {
    Running,
    Stopped
}

pub struct AudioPipeline {
    cpal_stream: CpalStream,
    broadcaster: Broadcaster,
    state: PipelineState
}

impl AudioPipeline {
    pub fn new(device: Device, config: SupportedStreamConfig) -> Self {
        Self {
            cpal_stream: CpalStream::new(device, config),
            broadcaster: Broadcaster::new(),
            state: PipelineState::Stopped
        }
    }

    pub fn start(&mut self) -> Result<(), AudioPipelineError> {
        if self.state == PipelineState::Running {
            return Ok(());
        }

        let to_broadcaster = self.cpal_stream.start()?;
        self.broadcaster.start(to_broadcaster);
        self.state = PipelineState::Running;
        
        tracing::info!("Audio pipeline started");
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), AudioPipelineError> {
        if self.state == PipelineState::Stopped {
            return Ok(())
        }

        self.cpal_stream.stop()?;
        self.state = PipelineState::Stopped;

        tracing::info!("Audio pipeline stopped");
        Ok(())
    }

    pub fn subscribe(&self, worker_id: WorkerID) -> Result<Receiver<AudioFrame>, AudioPipelineError> {
        if self.state == PipelineState::Stopped {
            return Err(AudioPipelineError::NotRunning())
        }

        Ok(self.broadcaster.subscribe(worker_id))
    }

    pub fn unsubscribe(&self, worker_id: &WorkerID) -> () {
        self.broadcaster.unsubscribe(worker_id);
    }
}