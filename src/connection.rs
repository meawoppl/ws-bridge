use std::fmt;
use std::marker::PhantomData;

use crate::codec::{DecodeError, EncodeError, WsCodec, WsMessage};

/// Error when sending a message.
#[derive(Debug)]
#[non_exhaustive]
pub enum SendError {
    /// The message could not be encoded; the connection is retained.
    Encode(EncodeError),
    /// The transport is closed.
    Closed,
    /// The underlying transport failed; inspect the source for details.
    Transport(TransportError),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::Encode(e) => write!(f, "send error: {e}"),
            SendError::Transport(e) => write!(f, "transport error: {e}"),
            SendError::Closed => write!(f, "connection closed"),
        }
    }
}

impl std::error::Error for SendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(error) => Some(error),
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }
}

impl From<EncodeError> for SendError {
    fn from(e: EncodeError) -> Self {
        SendError::Encode(e)
    }
}

/// Error when receiving a message.
#[derive(Debug)]
#[non_exhaustive]
pub enum RecvError {
    /// The message could not be decoded; subsequent messages may be received.
    Decode(DecodeError),
    /// The transport is closed.
    Closed,
    /// The underlying transport failed; inspect the source for details.
    Transport(TransportError),
}

impl fmt::Display for RecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecvError::Decode(e) => write!(f, "recv error: {e}"),
            RecvError::Transport(e) => write!(f, "transport error: {e}"),
            RecvError::Closed => write!(f, "connection closed"),
        }
    }
}

impl std::error::Error for RecvError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DecodeError> for RecvError {
    fn from(e: DecodeError) -> Self {
        RecvError::Decode(e)
    }
}

/// The sending half of a typed WebSocket connection.
///
/// Obtained by calling [`WsConnection::split`].
pub struct WsSender<S: WsCodec> {
    pub(crate) sink: Box<dyn ErasedSink>,
    pub(crate) _send: PhantomData<S>,
}

impl<S: WsCodec> WsSender<S> {
    /// Encodes and flushes one message. Cancellation may leave delivery uncertain.
    pub async fn send(&mut self, msg: S) -> Result<(), SendError> {
        let ws_msg = msg.encode()?;
        self.sink.send(ws_msg).await.map_err(SendError::Transport)
    }

    /// Initiates closure. Continue receiving with a deadline to finish the handshake.
    pub async fn close(&mut self) -> Result<(), SendError> {
        self.sink.close().await.map_err(SendError::Transport)
    }
}

/// The receiving half of a typed WebSocket connection.
///
/// Obtained by calling [`WsConnection::split`].
pub struct WsReceiver<R: WsCodec> {
    pub(crate) stream: Box<dyn ErasedStream>,
    pub(crate) _recv: PhantomData<R>,
}

impl<R: WsCodec> WsReceiver<R> {
    /// Receives one message. Decode errors are recoverable; transport errors end
    /// the receiver after one error. Once ended, subsequent calls return `None`.
    /// Cancellation does not guarantee preservation of an already consumed frame.
    pub async fn recv(&mut self) -> Option<Result<R, RecvError>> {
        match self.stream.next().await {
            None => {
                self.stream = Box::new(Ended);
                None
            }
            Some(Err(e)) => {
                self.stream = Box::new(Ended);
                Some(Err(RecvError::Transport(e)))
            }
            Some(Ok(ws_msg)) => Some(R::decode(ws_msg).map_err(RecvError::Decode)),
        }
    }
}

/// A typed WebSocket connection parameterized by send and receive types.
///
/// On the server side: `Send = E::ServerMsg`, `Recv = E::ClientMsg`.
/// On the client side: `Send = E::ClientMsg`, `Recv = E::ServerMsg`.
pub struct WsConnection<S: WsCodec, R: WsCodec> {
    pub(crate) sink: Box<dyn ErasedSink>,
    pub(crate) stream: Box<dyn ErasedStream>,
    pub(crate) _types: PhantomData<(S, R)>,
}

impl<S: WsCodec, R: WsCodec> WsConnection<S, R> {
    /// Encodes and flushes one message. Cancellation may leave delivery uncertain.
    pub async fn send(&mut self, msg: S) -> Result<(), SendError> {
        let ws_msg = msg.encode()?;
        self.sink.send(ws_msg).await.map_err(SendError::Transport)
    }

    /// Receives one message. Decode errors are recoverable; transport errors end
    /// the receiver after one error. Once ended, subsequent calls return `None`.
    /// Cancellation does not guarantee preservation of an already consumed frame.
    pub async fn recv(&mut self) -> Option<Result<R, RecvError>> {
        match self.stream.next().await {
            None => {
                self.stream = Box::new(Ended);
                None
            }
            Some(Err(e)) => {
                self.stream = Box::new(Ended);
                Some(Err(RecvError::Transport(e)))
            }
            Some(Ok(ws_msg)) => Some(R::decode(ws_msg).map_err(RecvError::Decode)),
        }
    }

    /// Sends and flushes a WebSocket close frame.
    /// Continue receiving until `None` to finish the peer's close handshake.
    /// Initiates closure. Continue receiving with a deadline to finish the handshake.
    pub async fn close(&mut self) -> Result<(), SendError> {
        self.sink.close().await.map_err(SendError::Transport)
    }

    #[cfg(feature = "reconnect")]
    pub(crate) async fn send_encoded(&mut self, msg: WsMessage) -> Result<(), SendError> {
        self.sink.send(msg).await.map_err(SendError::Transport)
    }

    /// Splits the connection for independent sending and receiving tasks.
    pub fn split(self) -> (WsSender<S>, WsReceiver<R>) {
        (
            WsSender {
                sink: self.sink,
                _send: PhantomData,
            },
            WsReceiver {
                stream: self.stream,
                _recv: PhantomData,
            },
        )
    }
}

// -- Erased trait objects for transport independence --

pub(crate) type BoxFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Type-erased sink that can send `WsMessage`s.
pub(crate) trait ErasedSink: Send {
    fn send(&mut self, msg: WsMessage) -> BoxFuture<'_, Result<(), TransportError>>;
    fn close(&mut self) -> BoxFuture<'_, Result<(), TransportError>>;
}

/// Type-erased stream that yields `WsMessage`s.
pub(crate) trait ErasedStream: Send {
    fn next(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, TransportError>>>;
}

/// A transport failure retaining its original error for diagnostics.
#[derive(Debug)]
pub struct TransportError(Box<dyn std::error::Error + Send + Sync>);
impl TransportError {
    /// Wraps an error from a WebSocket implementation.
    pub fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self(Box::new(error))
    }
}
impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

struct Ended;
impl ErasedStream for Ended {
    fn next(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, TransportError>>> {
        Box::pin(async { None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    struct FailingSink;
    impl ErasedSink for FailingSink {
        fn send(&mut self, _: WsMessage) -> BoxFuture<'_, Result<(), TransportError>> {
            Box::pin(async { Err(TransportError::new(std::io::Error::other("broken pipe"))) })
        }
        fn close(&mut self) -> BoxFuture<'_, Result<(), TransportError>> {
            self.send(WsMessage::Text(String::new()))
        }
    }
    struct UnexpectedStream;
    impl ErasedStream for UnexpectedStream {
        fn next(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, TransportError>>> {
            Box::pin(async { Some(Err(TransportError::new(std::io::Error::other("reset")))) })
        }
    }
    fn connection() -> WsConnection<String, String> {
        WsConnection {
            sink: Box::new(FailingSink),
            stream: Box::new(UnexpectedStream),
            _types: PhantomData,
        }
    }
    #[tokio::test]
    async fn failures_keep_sources_and_receivers_become_terminal() {
        let mut conn = connection();
        let error = conn.send("hello".into()).await.unwrap_err();
        assert_eq!(
            error.source().unwrap().source().unwrap().to_string(),
            "broken pipe"
        );
        let error = conn.recv().await.unwrap().unwrap_err();
        assert_eq!(
            error.source().unwrap().source().unwrap().to_string(),
            "reset"
        );
        assert!(conn.recv().await.is_none());
        let (mut sender, mut receiver) = connection().split();
        assert!(matches!(sender.close().await, Err(SendError::Transport(_))));
        assert!(matches!(
            receiver.recv().await,
            Some(Err(RecvError::Transport(_)))
        ));
        assert!(receiver.recv().await.is_none());
    }
}
