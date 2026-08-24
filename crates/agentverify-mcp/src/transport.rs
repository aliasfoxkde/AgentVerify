//! MCP Transport Layer
//!
//! Provides transport implementations for MCP protocol communication.
//! Currently supports stdio (subprocess) transport.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{mpsc, Mutex as TokioMutex};

use crate::protocol::JsonRpcMessage;

/// Transport errors
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("Process exited: {0}")]
    ProcessExited(i32),

    #[error("Not connected")]
    NotConnected,
}

/// Stdio transport for subprocess communication
pub struct StdioTransport {
    writer: Arc<tokio::sync::Mutex<ChildStdin>>,
    reader: Arc<tokio::sync::Mutex<BufReader<ChildStdout>>>,
    connected: Arc<AtomicBool>,
}

impl StdioTransport {
    /// Connect to an MCP server via stdio
    ///
    /// # Arguments
    /// * `command` - The command to execute (e.g., "npx", "python")
    /// * `args` - Command arguments (e.g., ["-m", "mcp_server"])
    pub async fn connect(command: &str, args: &[&str]) -> Result<Self, TransportError> {
        use std::process::Stdio;

        let mut child = tokio::process::Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                TransportError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, e))
            })?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let writer = Arc::new(tokio::sync::Mutex::new(stdin));
        let reader = Arc::new(tokio::sync::Mutex::new(BufReader::new(stdout)));
        let connected = Arc::new(AtomicBool::new(true));

        Ok(Self {
            writer,
            reader,
            connected,
        })
    }

    /// Send a JSON-RPC message as a newline-delimited JSON line
    pub async fn send(&self, msg: JsonRpcMessage) -> Result<(), TransportError> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(TransportError::NotConnected);
        }

        let line = serde_json::to_string(&msg)?;
        let mut writer = self.writer.lock().await;
        writer.write_all(line.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        Ok(())
    }

    /// Receive a JSON-RPC message (blocking read line)
    pub async fn recv(&self) -> Result<JsonRpcMessage, TransportError> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(TransportError::NotConnected);
        }

        let mut reader = self.reader.lock().await;
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        // Handle empty line (server closed or keepalive)
        if line.trim().is_empty() {
            return Err(TransportError::NotConnected);
        }

        let msg: JsonRpcMessage = serde_json::from_str(&line)?;
        Ok(msg)
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Set connected state
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::SeqCst);
    }
}

/// In-memory channel transport for testing
pub struct ChannelTransport {
    tx: mpsc::Sender<JsonRpcMessage>,
    rx: TokioMutex<mpsc::Receiver<JsonRpcMessage>>,
    connected: Arc<AtomicBool>,
}

impl ChannelTransport {
    /// Create a connected pair of channel transports
    pub fn channel() -> (Self, Self) {
        let (tx1, rx1) = mpsc::channel(100);
        let (tx2, rx2) = mpsc::channel(100);
        let connected = Arc::new(AtomicBool::new(true));

        let t1 = Self {
            tx: tx1,
            rx: TokioMutex::new(rx2),
            connected: connected.clone(),
        };
        let t2 = Self {
            tx: tx2,
            rx: TokioMutex::new(rx1),
            connected,
        };

        (t1, t2)
    }

    /// Send a JSON-RPC message
    pub async fn send(&self, msg: JsonRpcMessage) -> Result<(), TransportError> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(TransportError::NotConnected);
        }
        self.tx
            .send(msg)
            .await
            .map_err(|_| TransportError::Channel("Receiver dropped".to_string()))
    }

    /// Receive a JSON-RPC message
    pub async fn recv(&self) -> Result<JsonRpcMessage, TransportError> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(TransportError::NotConnected);
        }

        let mut rx = self.rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| TransportError::Channel("Sender dropped".to_string()))
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Set connected state
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_channel_transport() {
        let (t1, t2) = ChannelTransport::channel();

        let msg =
            JsonRpcMessage::Notification(crate::protocol::JsonRpcNotification::new("test", None));

        t1.send(msg.clone()).await.unwrap();
        let received = t2.recv().await.unwrap();

        assert!(matches!(received, JsonRpcMessage::Notification(_)));
    }
}
