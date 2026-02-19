use std::fmt;
use std::marker::PhantomData;

use crate::codec::{DecodeError, EncodeError, WsCodec, WsMessage};

/// Error when sending a message.
#[derive(Debug)]
pub enum SendError {
    Encode(EncodeError),
    Closed,
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::Encode(e) => write!(f, "send error: {e}"),
            SendError::Closed => write!(f, "connection closed"),
        }
    }
}

impl std::error::Error for SendError {}

impl From<EncodeError> for SendError {
    fn from(e: EncodeError) -> Self {
        SendError::Encode(e)
    }
}

/// Error when receiving a message.
#[derive(Debug)]
pub enum RecvError {
    Decode(DecodeError),
    Closed,
}

impl fmt::Display for RecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecvError::Decode(e) => write!(f, "recv error: {e}"),
            RecvError::Closed => write!(f, "connection closed"),
        }
    }
}

impl std::error::Error for RecvError {}

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
    pub async fn send(&mut self, msg: S) -> Result<(), SendError> {
        let ws_msg = msg.encode()?;
        self.sink.send(ws_msg).await.map_err(|_| SendError::Closed)
    }

    pub async fn close(&mut self) -> Result<(), SendError> {
        self.sink.close().await.map_err(|_| SendError::Closed)
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
    pub async fn recv(&mut self) -> Option<Result<R, RecvError>> {
        match self.stream.next().await {
            None => None,
            Some(Err(_)) => Some(Err(RecvError::Closed)),
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
    pub async fn send(&mut self, msg: S) -> Result<(), SendError> {
        let ws_msg = msg.encode()?;
        self.sink.send(ws_msg).await.map_err(|_| SendError::Closed)
    }

    pub async fn recv(&mut self) -> Option<Result<R, RecvError>> {
        match self.stream.next().await {
            None => None,
            Some(Err(_)) => Some(Err(RecvError::Closed)),
            Some(Ok(ws_msg)) => Some(R::decode(ws_msg).map_err(RecvError::Decode)),
        }
    }

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
    fn send(&mut self, msg: WsMessage) -> BoxFuture<'_, Result<(), ()>>;
    fn close(&mut self) -> BoxFuture<'_, Result<(), ()>>;
}

/// Type-erased stream that yields `WsMessage`s.
pub(crate) trait ErasedStream: Send {
    fn next(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, ()>>>;
}
