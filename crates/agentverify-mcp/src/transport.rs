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
    /// The underlying stream or process could not be read or written.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A message could not be serialized or deserialized.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The in-process channel linking the two peers failed.
    #[error("Channel error: {0}")]
    Channel(String),

    /// The peer process exited with the given status.
    #[error("Process exited: {0}")]
    ProcessExited(i32),

    /// The transport has been closed.
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
    ///
    /// * `command` - The command to execute, for example `npx` or `python`
    /// * `args` - Command arguments passed to `command`
    ///
    /// # Errors
    ///
    /// Returns `TransportError::Io` if the server process cannot be spawned,
    /// and `TransportError::Channel` if its stdin or stdout streams are not
    /// piped as requested.
    ///
    /// The signature is `async` for forward compatibility with asynchronous
    /// process spawning, even though the body is currently synchronous.
    // `unused_async_trait_impl` (clippy 1.98) fires on this signature in
    // addition to `unused_async`, so both names are allowed.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
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

        // Both streams were configured as piped above, so they are present
        // unless the child was already reaped; fail loudly but without panicking.
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TransportError::Channel("child stdin was not piped".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TransportError::Channel("child stdout was not piped".to_string()))?;

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
    ///
    /// # Errors
    ///
    /// Returns `TransportError::NotConnected` if the transport has been
    /// closed, and propagates serialization or write failures.
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
    ///
    /// # Errors
    ///
    /// Returns `TransportError::NotConnected` if the transport has been
    /// closed or the peer sent an empty line, and propagates read or
    /// deserialization failures.
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
    #[must_use]
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
    #[must_use]
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
    ///
    /// # Errors
    ///
    /// Returns `TransportError::NotConnected` if the transport has been
    /// closed, or `TransportError::Channel` if the receiving half has been
    /// dropped.
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
    ///
    /// # Errors
    ///
    /// Returns `TransportError::NotConnected` if the transport has been
    /// closed, or `TransportError::Channel` if the sending half has been
    /// dropped.
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
    use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

    fn sample_request(id: u64) -> JsonRpcMessage {
        JsonRpcMessage::Request(JsonRpcRequest::new(id, "tools/list", None))
    }

    fn sample_response(id: u64) -> JsonRpcMessage {
        JsonRpcMessage::Response(JsonRpcResponse::Success {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(id),
            result: serde_json::json!({"tools": []}),
        })
    }

    #[tokio::test]
    async fn test_channel_transport() {
        let (t1, t2) = ChannelTransport::channel();

        let msg = JsonRpcMessage::Notification(JsonRpcNotification::new("test", None));

        t1.send(msg.clone()).await.unwrap();
        let received = t2.recv().await.unwrap();

        assert!(matches!(received, JsonRpcMessage::Notification(_)));
    }

    #[tokio::test]
    async fn channel_transport_roundtrips_every_message_shape() {
        let (client, server) = ChannelTransport::channel();

        for message in [sample_request(1), sample_response(2)] {
            client.send(message.clone()).await.unwrap();
            let received = server.recv().await.unwrap();
            assert_eq!(
                serde_json::to_string(&received).unwrap(),
                serde_json::to_string(&message).unwrap()
            );
        }

        // The pair is symmetric: the server side can answer on the same
        // transport it read from.
        server.send(sample_response(1)).await.unwrap();
        assert!(matches!(
            client.recv().await.unwrap(),
            JsonRpcMessage::Response(_)
        ));
    }

    #[tokio::test]
    async fn channel_transport_send_fails_when_the_receiver_is_dropped() {
        let (client, server) = ChannelTransport::channel();
        drop(server);

        let err = client.send(sample_request(1)).await.unwrap_err();
        assert!(
            matches!(err, TransportError::Channel(ref message) if message == "Receiver dropped")
        );
    }

    #[tokio::test]
    async fn channel_transport_recv_fails_when_the_sender_is_dropped() {
        let (client, server) = ChannelTransport::channel();
        drop(client);

        let err = server.recv().await.unwrap_err();
        assert!(matches!(err, TransportError::Channel(ref message) if message == "Sender dropped"));
    }

    #[test]
    fn error_messages_are_stable() {
        let io = TransportError::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "pipe closed",
        ));
        assert_eq!(io.to_string(), "IO error: pipe closed");

        let parse = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let json = TransportError::from(parse);
        assert!(json.to_string().starts_with("JSON error: "));

        assert_eq!(
            TransportError::Channel("Receiver dropped".to_string()).to_string(),
            "Channel error: Receiver dropped"
        );
        assert_eq!(
            TransportError::ProcessExited(3).to_string(),
            "Process exited: 3"
        );
        assert_eq!(TransportError::NotConnected.to_string(), "Not connected");
    }

    #[tokio::test]
    async fn channel_transport_reports_a_closed_connection() {
        let (client, server) = ChannelTransport::channel();

        client.set_connected(false);
        assert!(!client.is_connected());
        // The connected flag is shared by both halves of the pair.
        assert!(!server.is_connected());

        let send = client.send(sample_request(1)).await.unwrap_err();
        assert!(matches!(send, TransportError::NotConnected));
        let recv = client.recv().await.unwrap_err();
        assert!(matches!(recv, TransportError::NotConnected));

        // Reconnecting restores the pair.
        client.set_connected(true);
        client.send(sample_request(1)).await.unwrap();
        assert!(matches!(
            server.recv().await.unwrap(),
            JsonRpcMessage::Request(_)
        ));
    }
}
