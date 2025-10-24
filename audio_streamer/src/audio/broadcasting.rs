use std::{sync::{atomic::AtomicU64, Arc}, thread, time::Instant};

use dashmap::DashMap;
use crossbeam_channel::{bounded, Receiver, Sender};
use thiserror;

type AudioFrame = Arc<[f32; 1920]>;
type WorkerID = u16;

#[derive(Debug, thiserror::Error)]
pub enum BroadcasterError {

    #[error("Failed to run broadcaster: no frame receiver was provided. \nEngine ----channel---> Broadcaster")]
    NoFrameReceiverError()
}

pub struct Broadcaster {
    subscribers: Arc<DashMap<WorkerID, SubscriberHandle>>,
}

pub struct SubscriberHandle {
    sender: Sender<AudioFrame>,
    _metadata: SubscriberMetadata,
}

pub struct SubscriberMetadata {
    _connected_at: Instant,
    _frames_sent: AtomicU64,
    _frames_dropped: AtomicU64,
}

impl Broadcaster {

    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(DashMap::new())
        }
    }

    pub fn start(&self, from_engine_channel_receiver: Receiver<AudioFrame>) -> () {
        let subscribers = self.subscribers.clone();

        thread::spawn(move || {
            loop {
                match from_engine_channel_receiver.recv() {
                    Ok(frame) => {
                        // Log if we're dropping frames
                        let mut dropped = 0;

                        subscribers.retain(|_id, sub_handle| {
                            let sent = sub_handle.sender.try_send(frame.clone()).is_ok();

                            if !sent {
                                dropped += 1;
                            }

                            sent
                        });
                        
                        if dropped > 0 {
                            tracing::debug!("Dropped frame for {} slow subscribers", dropped);
                        }
                    },

                    Err(e) => {
                        tracing::info!("Audio engine stopped, broadcaster shutting down: {}", e);
                        return;
                    }
                }
            }
        });
    }   

    pub fn subscribe(&self, worker_id: WorkerID) -> Receiver<AudioFrame> {
        let (sender, receiver) = bounded::<AudioFrame>(10);

        let metadata = SubscriberMetadata {
            _connected_at: std::time::Instant::now(),
            _frames_sent: AtomicU64::new(0),
            _frames_dropped: AtomicU64::new(0)
        };

        self.subscribers.insert(worker_id, SubscriberHandle{ sender, _metadata: metadata });

        receiver
    }

    pub fn unsubscribe(&self, worker_id: &WorkerID) -> () {
        self.subscribers.remove(worker_id);
    }

    fn _get_subscriber_stats(&self, worker_id: &WorkerID) -> Option<SubscriberMetadata> {
        unimplemented!()
    }
}

