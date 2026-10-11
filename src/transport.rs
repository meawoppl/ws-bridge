//! Adapt other `Send` WebSocket transports to typed connections.
//!
//! Framework adapters use this same conversion layer. Applications can supply
//! conversions for transports not covered by a built-in adapter.
use crate::TransportError;
use std::marker::PhantomData;

use futures_util::{Sink, SinkExt, Stream, StreamExt};

use crate::connection::{BoxFuture, ErasedSink, ErasedStream};
use crate::{WsConnection, WsEndpoint, WsMessage};

/// The application-visible meaning of a received transport frame.
pub enum Frame {
    /// Text or binary application data.
    Message(WsMessage),
    /// A control frame handled by the underlying WebSocket implementation.
    Control,
    /// The peer has closed the connection.
    Close,
}

/// Wraps a WebSocket stream/sink using transport-specific frame conversions.
///
/// The underlying transport must handle ping replies and the close handshake.
/// `decode` must return `Frame::Close` for close frames and `Frame::Control`
/// for ping/pong frames, so they never reach the application codec.
///
/// ```no_run
/// # use ws_bridge::{WsEndpoint, WsMessage};
/// # struct Echo;
/// # impl WsEndpoint for Echo {
/// # const PATH: &'static str = "/ws/echo";
/// # type ServerMsg = String;
/// # type ClientMsg = String;
/// # }
/// # fn example<T>(socket: T)
/// # where T: futures_util::Sink<Option<WsMessage>, Error = std::io::Error>
/// # + futures_util::Stream<Item = Result<Option<WsMessage>, std::io::Error>> + Unpin + Send + 'static {
/// use ws_bridge::transport::{self, Frame};
/// let conn = transport::into_connection::<Echo, _, _, _>(
///     socket,
///     Some,
///     |frame| match frame {
///         Some(message) => Frame::Message(message),
///         None => Frame::Close,
///     },
/// );
/// # }
/// ```
pub fn into_connection<E, T, M, Err>(
    socket: T,
    encode: fn(WsMessage) -> M,
    decode: fn(M) -> Frame,
) -> WsConnection<E::ServerMsg, E::ClientMsg>
where
    E: WsEndpoint,
    T: Sink<M, Error = Err> + Stream<Item = Result<M, Err>> + Unpin + Send + 'static,
    M: Send + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    let (sink, stream) = socket.split();
    WsConnection {
        sink: Box::new(TransportSink { sink, encode }),
        stream: Box::new(TransportStream { stream, decode }),
        _types: PhantomData,
    }
}

struct TransportSink<T, M> {
    sink: T,
    encode: fn(WsMessage) -> M,
}

impl<T, M, Err> ErasedSink for TransportSink<T, M>
where
    T: Sink<M, Error = Err> + Unpin + Send,
    M: Send,
    Err: std::error::Error + Send + Sync + 'static,
{
    fn send(&mut self, msg: WsMessage) -> BoxFuture<'_, Result<(), TransportError>> {
        Box::pin(async move {
            self.sink
                .send((self.encode)(msg))
                .await
                .map_err(TransportError::new)
        })
    }

    fn close(&mut self) -> BoxFuture<'_, Result<(), TransportError>> {
        Box::pin(async move { self.sink.close().await.map_err(TransportError::new) })
    }
}

struct TransportStream<T, M> {
    stream: T,
    decode: fn(M) -> Frame,
}

impl<T, M, Err> ErasedStream for TransportStream<T, M>
where
    T: Stream<Item = Result<M, Err>> + Unpin + Send,
    Err: std::error::Error + Send + Sync + 'static,
{
    fn next(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, TransportError>>> {
        Box::pin(async move {
            loop {
                match self.stream.next().await {
                    None => return None,
                    Some(Err(e)) => return Some(Err(TransportError::new(e))),
                    Some(Ok(msg)) => match (self.decode)(msg) {
                        Frame::Message(msg) => return Some(Ok(msg)),
                        Frame::Control => continue,
                        Frame::Close => return None,
                    },
                }
            }
        })
    }
}
