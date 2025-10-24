use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tokio::{io::{AsyncBufReadExt, BufReader}, net::TcpStream as AsyncTcpStream};

#[derive(Debug, thiserror::Error)]
pub enum CommandSerializationError {
    #[error("Failed to serialize json string to a struct: {0}")]
    SerdeSerializationError(#[from] serde_json::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum TcpConnectionError {
    #[error(transparent)]
    MessageSerializationError(#[from] CommandSerializationError),

    #[error("Failed to read from stream: {0}")]
    FailedToReadFromStream(std::io::Error),

    #[error("Client has closed the connection.")]
    ConnectionClosed()
}


// TODO: move this struct somewhere
#[derive(Serialize, Deserialize, Debug)]
pub struct StreamStats {
    pub frames_sent: u64,
    pub frames_dropped: u64,
    pub connected_duration_secs: u64,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "cmd", rename_all = "lowercase")]
pub enum Command {
    /// Requests a new audio stream. Rust will find and assign an available port.
    Issue,

    /// Instructs Rust to close the stream on a specific port and free the resources.
    Close { port: u16 },

    /// Asks for a list of all currently active ports.
    Active,

    /// Asks for statistics for a specific stream.
    Stats { port: u16 },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SuccessPayload {
    /// Response to an `Issue` command.
    Issued { port: u16 },
    /// Response to a `Close` command.
    Closed { port: u16 },
    /// Response to an `Active` command.
    Active { ports: Vec<u16> },
    /// Response to a `Stats` command.
    Stats { port: u16, stats: StreamStats },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ErrorPayload {
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Response {
    Success(SuccessPayload),
    Error(ErrorPayload),
}

impl Response {
    pub fn issued_from_result(result: Result<u16, impl Display>) -> Response {
        match result {
            Ok(port) => Response::Success(SuccessPayload::Issued { port }),
            Err(e) => Response::Error(ErrorPayload { message: format!("Failed to issue new worker: {}", e) })
        }
    }

    pub fn closed_from_result(result: Result<u16, impl Display>) -> Response {
        match result {
            Ok(port) => Response::Success(SuccessPayload::Closed { port }),
            Err(e) => Response::Error(ErrorPayload { message: format!("Failed to close the worker: {}", e) })
        }
    }

    pub fn active_from_result(result: Result<Vec<u16>, impl Display>) -> Response {
        match result {
            Ok(ports) => Response::Success(SuccessPayload::Active { ports }),
            Err(e) => Response::Error(ErrorPayload { message: format!("Failed to retreive active workers: {}", e) })
        }
    }

    pub fn stats_from_result(result: Result<(u16, StreamStats), impl Display>) -> Response {
        match result {
            Ok((port, stats)) => Response::Success(SuccessPayload::Stats { port, stats }),
            Err(e) => Response::Error(ErrorPayload { message: format!("Failed to retreive stats for the worker: {}", e) })
        }
    }
}

pub async fn recv_command(socket: &mut AsyncTcpStream) -> Result<Command, TcpConnectionError> {
    let mut reader = BufReader::new(socket);
    let mut jstr = String::new();

    let bytes_read = reader.read_line(&mut jstr).await.map_err(TcpConnectionError::FailedToReadFromStream)?;

    if bytes_read == 0 {
        return Err(TcpConnectionError::ConnectionClosed())
    }

    let command = serde_json::from_str(&jstr).map_err(|err| CommandSerializationError::SerdeSerializationError(err))?;
    Ok(command)
}