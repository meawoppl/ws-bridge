use std::future::Future;
use std::marker::PhantomData;

use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite;

use crate::codec::WsMessage;
use crate::connection::{ErasedSink, ErasedStream, WsConnection};
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

impl std::error::Error for ConnectError {}

/// Connects to a WebSocket endpoint using the endpoint's path.
///
/// The `base_url` should be the server's WebSocket base URL
/// (e.g., `"ws://localhost:3000"`). The endpoint's `PATH` is appended.
///
/// # Example
///
/// ```ignore
/// let mut conn = ws_bridge::native_client::connect::<SessionSocket>("ws://localhost:3000").await?;
/// conn.send(ProxyToServer::Register { ... }).await?;
/// while let Some(Ok(msg)) = conn.recv().await {
///     // msg is SessionSocket::ServerMsg
/// }
/// ```
pub async fn connect<E: WsEndpoint>(base_url: &str) -> Result<Connection<E>, ConnectError> {
    let url = format!("{}{}", base_url.trim_end_matches('/'), E::PATH);
    connect_to_url::<E>(&url).await
}

/// Connects to a specific URL, bypassing the endpoint's `PATH`.
///
/// Useful for parameterized paths like `/ws/voice/:session_id`.
pub async fn connect_to_url<E: WsEndpoint>(url: &str) -> Result<Connection<E>, ConnectError> {
    let (ws_stream, _response) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(ConnectError)?;

    let (sink, stream) = ws_stream.split();
    Ok(WsConnection {
        sink: Box::new(TungsteniteSink(sink)),
        stream: Box::new(TungsteniteStream(stream)),
        _types: PhantomData,
    })
}

// -- tokio-tungstenite transport adapters --

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

struct TungsteniteSink(futures_util::stream::SplitSink<WsStream, tungstenite::Message>);

impl ErasedSink for TungsteniteSink {
    fn send(
        &mut self,
        msg: WsMessage,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), ()>> + Send + '_>> {
        Box::pin(async move {
            let tung_msg = match msg {
                WsMessage::Text(t) => tungstenite::Message::Text(t),
                WsMessage::Binary(b) => tungstenite::Message::Binary(b),
            };
            self.0.send(tung_msg).await.map_err(|_| ())
        })
    }

    fn close(
        &mut self,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), ()>> + Send + '_>> {
        Box::pin(async move { self.0.close().await.map_err(|_| ()) })
    }
}

struct TungsteniteStream(futures_util::stream::SplitStream<WsStream>);

impl ErasedStream for TungsteniteStream {
    fn next(
        &mut self,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = Option<Result<WsMessage, ()>>> + Send + '_>,
    > {
        Box::pin(async move {
            loop {
                match self.0.next().await {
                    None => return None,
                    Some(Err(_)) => return Some(Err(())),
                    Some(Ok(msg)) => match msg {
                        tungstenite::Message::Text(t) => {
                            return Some(Ok(WsMessage::Text(t)));
                        }
                        tungstenite::Message::Binary(b) => {
                            return Some(Ok(WsMessage::Binary(b)));
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
