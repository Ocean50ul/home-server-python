use std::io::IoSlice;

use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, net::{TcpListener as AsyncTcpListener, TcpStream as AsyncTcpStream}};

use crate::{audio::{StreamMetadata, WorkerEvent, WorkersManager, WorkersManagerError}, server::comm_protocol::TcpConnectionError};
use super::comm_protocol::{recv_command, Command, Response, SuccessPayload};

#[derive(Debug, thiserror::Error)]
pub enum TcpServerError {
    
    #[error("Failed to bind address: {0}")]
    AsyncTcpListenerBindingError(std::io::Error),

    #[error("Failed to accept the connection {0}")]
    ConnectionAcceptError(std::io::Error),

    #[error("Failed to serialize response into json string: {0}")]
    ResponseSerializationError(serde_json::Error),

    #[error("Failed to write the response to the comm socket: {0}")]
    AsyncSocketWriteError(std::io::Error),

    #[error("Failed to shutdown all workers: {0}")]
    ShutdownAllError(WorkersManagerError),

    #[error("Failed to serialize greeting struct into json: {0}")]
    GreetingMessageSerializationError(serde_json::Error)
}

#[derive(Serialize, Deserialize)]
struct Handshake {
    stream_metadata: StreamMetadata
}

pub struct TcpServer {
    listener: AsyncTcpListener,
    manager: WorkersManager,
    handshake_message: Handshake
}

impl TcpServer {
    pub async fn new(addr: &str, manager: WorkersManager) -> Result<Self, TcpServerError> {
        Ok(
            Self {
                listener: AsyncTcpListener::bind(addr).await.map_err(TcpServerError::AsyncTcpListenerBindingError)?,
                handshake_message: Handshake { stream_metadata: manager.stream_metadata.clone()},
                manager
            }
        )
    }

    async fn handle_command_message(&mut self, command: Command, socket: &mut AsyncTcpStream) -> Result<(), TcpServerError> {

        let response = match command {
            Command::Issue => {
                let issue_result = self.manager.issue_new_worker();
                Response::issued_from_result(issue_result)
            },

            Command::Active => {
                let acitve = self.manager.active_ports();
                Response::Success(SuccessPayload::Active { ports: acitve })
            },

            Command::Close { port } => {
                let stop_result = self.manager.stop_worker(port).await;
                Response::closed_from_result(stop_result)
            },

            Command::Stats { port } => {
                let stats_result = self.manager.get_stats(port);
                Response::stats_from_result(stats_result)
            }
        };

        let json_str = serde_json::to_string(&response).map_err(|e| TcpServerError::ResponseSerializationError(e))?;

        let buf = [
            IoSlice::new(&json_str.as_bytes()),
            IoSlice::new(b"\n")
        ];

        socket.write_vectored(&buf).await.map_err(|e| TcpServerError::AsyncSocketWriteError(e))?;

        Ok(())

    }

    async fn greet(&self, socket: &mut AsyncTcpStream ) -> Result<(), TcpServerError> {
        let json_str = serde_json::to_string(&self.handshake_message).map_err(TcpServerError::GreetingMessageSerializationError)?;

        let buf = [
            IoSlice::new(&json_str.as_bytes()),
            IoSlice::new(b"\n")
        ];

        socket.write_vectored(&buf).await.map_err(|e| TcpServerError::AsyncSocketWriteError(e))?;

        Ok(())
    }

    pub async fn run(mut self) -> Result<(), TcpServerError> {
        let (mut socket, _addr) = self.listener.accept().await.map_err(TcpServerError::ConnectionAcceptError)?;
        self.greet(&mut socket).await?;

        tracing::info!("Python client has connected to the socket! Greeting message has being sent, start listening for commands..");

        'server_loop: loop {
            tokio::select! {

                // Recv commands from python
                command_recv_result = recv_command(&mut socket) => {
                    match command_recv_result {
                        Ok(message) => {
                            if let Err(e) = self.handle_command_message(message, &mut socket).await {
                                tracing::error!("Failed to send the response: {}", e);
                            }
                        },
                        Err(e) => {
                            if let TcpConnectionError::MessageSerializationError(ser_error) = &e {
                                tracing::warn!("Failed to deserialize message from python: {}", ser_error);
                                continue 'server_loop;
                            }

                            tracing::error!("Connection to Python lost: {}. Shutting down.", e);
                            self.manager.shutdown_all().await.map_err(TcpServerError::ShutdownAllError)?;
                            break 'server_loop;
                        }
                    }
                },
                
                // Recv errors from workers
                Some(worker_message) = self.manager.worker_error_receiver.recv() => {
                    match worker_message.event {
                        WorkerEvent::StoppedWithError(e) => {
                            tracing::error!("Worker {} stopped with an error: {}", worker_message.id, e);

                            match self.manager.clean_up(worker_message.id) {
                                Ok(_) => { tracing::error!("Sucessfully cleand up errored worker!") },
                                Err(e) => {
                                    tracing::error!("Failed to clean up errored worker: {}\nEngine stop returned with an error. Shutting down the server.", e);
                                    break 'server_loop;
                                }
                            }
                        }
                    }
                },

                // Handling cntr+c gracefully
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Ctrl+C signal received, starting graceful shutdown..");
                    self.manager.shutdown_all().await.map_err(TcpServerError::ShutdownAllError)?;
                    break 'server_loop;
                }
            }
        }

        Ok(())
    }
}