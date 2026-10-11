use crate::TransportError;
use std::marker::PhantomData;

use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite;

use crate::codec::WsMessage;
use crate::connection::{BoxFuture, ErasedSink, ErasedStream, WsConnection};
use crate::WsEndpoint;

/// A native client-side typed WebSocket connection.
///
/// Sends `E::ClientMsg` and receives `E::ServerMsg` (flipped from server).
pub type Connection<E> = WsConnection<<E as WsEndpoint>::ClientMsg, <E as WsEndpoint>::ServerMsg>;

/// Error connecting to a WebSocket endpoint.
#[derive(Debug)]
pub struct ConnectError(tungstenite::Error);

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WebSocket connect error: {}", self.0)
    }
}

impl std::error::Error for ConnectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Connects to a WebSocket endpoint using the endpoint's path.
///
/// The `base_url` should be the server's WebSocket base URL
/// (e.g., `"ws://localhost:3000"`). The endpoint's `PATH` is appended.
///
/// # Example
///
/// ```no_run
/// # use ws_bridge::WsEndpoint;
/// # struct SessionSocket;
/// # impl WsEndpoint for SessionSocket {
/// #     const PATH: &'static str = "/ws/session";
/// #     type ServerMsg = String;
/// #     type ClientMsg = String;
/// # }
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut conn = ws_bridge::native_client::connect::<SessionSocket>("ws://localhost:3000").await?;
/// conn.send("hello".to_owned()).await?;
/// while let Some(Ok(msg)) = conn.recv().await {
///     // msg is SessionSocket::ServerMsg
/// }
/// # Ok(())
/// # }
/// ```
pub async fn connect<E: WsEndpoint>(base_url: &str) -> Result<Connection<E>, ConnectError> {
    let url = format!("{}{}", base_url.trim_end_matches('/'), E::PATH);
    connect_to_url::<E>(&url).await
}

/// Connects to a specific URL, bypassing the endpoint's `PATH`.
///
/// Useful for parameterized paths like `/ws/voice/:session_id`.
pub async fn connect_to_url<E: WsEndpoint>(url: &str) -> Result<Connection<E>, ConnectError> {
    connect_request::<E>(url, None).await.map(|(conn, _)| conn)
}

/// Types used for custom handshake requests and transport limits.
/// Re-exported to avoid requiring a second tungstenite dependency.
pub use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, handshake::client::Response, protocol::WebSocketConfig,
};

/// Connects with custom headers, subprotocols, and optional frame/message limits.
/// Returns the handshake response so callers can inspect negotiated subprotocols.
/// Build a request with [`IntoClientRequest`] before adding application headers.
/// Authentication must be supplied again on each reconnect attempt.
pub async fn connect_request<E: WsEndpoint>(
    request: impl IntoClientRequest + Unpin,
    config: Option<WebSocketConfig>,
) -> Result<(Connection<E>, Response), ConnectError> {
    if config
        .as_ref()
        .is_some_and(|c| c.max_write_buffer_size <= c.write_buffer_size)
    {
        return Err(ConnectError(tungstenite::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "max_write_buffer_size must exceed write_buffer_size",
        ))));
    }
    let (ws_stream, response) =
        tokio_tungstenite::connect_async_with_config(request, config, false)
            .await
            .map_err(ConnectError)?;
    let (sink, stream) = ws_stream.split();
    Ok((
        WsConnection {
            sink: Box::new(TungsteniteSink(sink)),
            stream: Box::new(TungsteniteStream(stream)),
            _types: PhantomData,
        },
        response,
    ))
}

// -- tokio-tungstenite transport adapters --

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct TungsteniteSink(futures_util::stream::SplitSink<WsStream, tungstenite::Message>);

impl ErasedSink for TungsteniteSink {
    fn send(&mut self, msg: WsMessage) -> BoxFuture<'_, Result<(), TransportError>> {
        Box::pin(async move {
            let tung_msg = match msg {
                WsMessage::Text(t) => tungstenite::Message::Text(t.into()),
                WsMessage::Binary(b) => tungstenite::Message::Binary(b.into()),
            };
            self.0.send(tung_msg).await.map_err(TransportError::new)
        })
    }

    fn close(&mut self) -> BoxFuture<'_, Result<(), TransportError>> {
        Box::pin(async move { self.0.close().await.map_err(TransportError::new) })
    }
}

struct TungsteniteStream(futures_util::stream::SplitStream<WsStream>);

impl ErasedStream for TungsteniteStream {
    fn next(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, TransportError>>> {
        Box::pin(async move {
            loop {
                match self.0.next().await {
                    None => return None,
                    Some(Err(e)) => return Some(Err(TransportError::new(e))),
                    Some(Ok(msg)) => match msg {
                        tungstenite::Message::Text(t) => {
                            return Some(Ok(WsMessage::Text(t.to_string())));
                        }
                        tungstenite::Message::Binary(b) => {
                            return Some(Ok(WsMessage::Binary(b.to_vec())));
                        }
                        tungstenite::Message::Close(_) => return None,
                        tungstenite::Message::Ping(_)
                        | tungstenite::Message::Pong(_)
                        | tungstenite::Message::Frame(_) => continue,
                    },
                }
            }
        })
    }
}
