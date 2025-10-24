use std::{sync::{atomic::{AtomicBool, Ordering}, Arc}, thread::JoinHandle};

use dashmap::DashMap;
use tokio::{sync::mpsc, task::JoinError};
use rand::{self, seq::IteratorRandom};

use crate::{audio::{audio_pipeline::{AudioPipeline, AudioPipelineError}, encoder_worker::{EncoderWorker, WorkerMessage}, stream_metadata::StreamMetadata}, server::StreamStats};


type WorkerId = u16;

#[derive(Debug, thiserror::Error)]
pub enum WorkersManagerError {

    #[error(transparent)]
    AudioPipelineError(#[from] AudioPipelineError),

    #[error("Ports pool is empty: maximum server capacity reached.")]
    PortIssueError(),

    #[error("There is no worker with id/port {0} in active workers pool.")]
    NoSuchWorkerError(u16),

    #[error("Failed to spawn blocking task for {0} worker in order to stop it: {1}")]
    WorkerStopAsyncTaskError(u16, JoinError),

    #[error("Failed to join the workers handle after sending stop signal for a {0} worker.")]
    WorkerHandleJoinError(u16),

    #[error("Async taks that joins all the handles during shutdown all has returned with an error: {0}")]
    ShutdownAllAsyncTaskError(JoinError)

}

pub struct WorkerHandle {
    stop_signal: Arc<AtomicBool>, 
    join_handle: JoinHandle<()>,
}

pub struct WorkersManager {
    audio_pipeline: AudioPipeline,
    ports_pool: Vec<u16>, // A simple stack of available ports
    active_workers: DashMap<WorkerId, WorkerHandle>,
    worker_error_sender: mpsc::Sender<WorkerMessage>,
    pub worker_error_receiver: mpsc::Receiver<WorkerMessage>,
    pub stream_metadata: StreamMetadata
}

impl WorkersManager {
    pub fn new(audio_pipeline: AudioPipeline, max_ports: usize, stream_metadata: StreamMetadata) -> Result<Self, WorkersManagerError> {
        let (sender, receiver) = mpsc::channel::<WorkerMessage>(100);
        let mut rng = rand::rng();
        let ports_pool = (49152..65535).choose_multiple(&mut rng, max_ports);

        Ok (
            Self {
                audio_pipeline,
                ports_pool,
                active_workers: DashMap::new(),
                worker_error_receiver: receiver,
                worker_error_sender: sender,
                stream_metadata
            }
        )
    }

    pub fn issue_new_worker(&mut self) -> Result<u16, WorkersManagerError> {

        if self.active_ports().is_empty() {
            self.audio_pipeline.start()?;
        }

        let port = self.ports_pool.pop().ok_or(WorkersManagerError::PortIssueError())?;

        let stop_signal = Arc::new(AtomicBool::new(false));
        // mb not a good idea, but welp
        // could make a hash function for ports later though
        let worker_id = port;
        let frames_receiver =  self.audio_pipeline.subscribe(worker_id)?;
        let error_channel_sender = self.worker_error_sender.clone();

        let worker = EncoderWorker::new(
            worker_id,
            port,
            frames_receiver,
            error_channel_sender,
            self.stream_metadata.clone(),
            stop_signal.clone()
        );

        let handle = WorkerHandle { stop_signal, join_handle: worker.run() };
        self.active_workers.insert(worker_id, handle);

        tracing::info!("Issued new worker {} on port {}", worker_id, port);

        Ok(port)
    }

    pub async fn stop_worker(&mut self, port: u16) -> Result<u16, WorkersManagerError> {
        let (_port, worker_handle) = self.active_workers.remove(&port).ok_or(WorkersManagerError::NoSuchWorkerError(port))?;

        worker_handle.stop_signal.store(true, Ordering::Release);

        let join_result = tokio::task::spawn_blocking(move || {
            worker_handle.join_handle.join()
        }).await.map_err(|e| WorkersManagerError::WorkerStopAsyncTaskError(port, e))?;

        match join_result {
            Ok(_) => {
                self.ports_pool.push(port);
                self.audio_pipeline.unsubscribe(&port);

                if self.active_ports().is_empty() {
                    self.audio_pipeline.stop()?;
                }

                Ok(port)
            },

            Err(_) => { Err(WorkersManagerError::WorkerHandleJoinError(port)) }
        }
    }

    pub fn get_stats(&self, _port: u16) -> Result<(u16, StreamStats), WorkersManagerError> {
        // placeholder
        Ok((
            42069,
            StreamStats { frames_sent: 42, frames_dropped: 69, connected_duration_secs: 420}
        ))
    }

    pub fn active_ports(&self) -> Vec<u16> {
        self.active_workers.iter().map(|entry| *entry.key()).collect()
    }

    pub async fn shutdown_all(&mut self) -> Result<(), WorkersManagerError> {
        tracing::info!("Shutting down all {} workers", self.active_workers.len());

        let handles = self.active_ports()
            .iter()
            .filter_map(|port| self.active_workers.remove(port))
            .map(|entry| entry.1)
            .collect::<Vec<_>>();

        
        tokio::task::spawn_blocking(move || {
            for handle in handles {
                handle.stop_signal.store(true, Ordering::Release);
                if let Err(e) = handle.join_handle.join() {
                    tracing::error!("A worker thread panicked during shutdown: {:?}", e);
                }
            }
        }).await.map_err(WorkersManagerError::ShutdownAllAsyncTaskError)?;

        self.audio_pipeline.stop()?;

        Ok(())
    }

    pub fn clean_up(&mut self, id: u16) -> Result<(), WorkersManagerError> {
        if let Some((_, handle)) = self.active_workers.remove(&id) {
            handle.stop_signal.store(true, Ordering::Relaxed);
        }
        
        self.ports_pool.push(id);
        self.audio_pipeline.unsubscribe(&id);

        if self.active_workers.is_empty() {
            self.audio_pipeline.stop()?;
        }

        Ok(())
    }
}