use std::marker::PhantomData;

use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use gloo_net::websocket::futures::WebSocket;
use gloo_net::websocket::Message;

use crate::codec::{WsCodec, WsMessage};
use crate::connection::{RecvError, SendError};
use crate::WsEndpoint;

/// Error connecting to a WebSocket endpoint from the browser.
#[derive(Debug)]
pub struct ConnectError(gloo_net::websocket::WebSocketError);

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WebSocket connect error: {:?}", self.0)
    }
}

/// Connects to a WebSocket endpoint, deriving `ws://`/`wss://` from the
/// page's current protocol and host.
///
/// # Example
///
/// ```ignore
/// let mut conn = ws_bridge::yew_client::connect::<SessionSocket>()?;
/// conn.send(ClientToServer::Register { ... }).await?;
/// while let Some(Ok(msg)) = conn.recv().await {
///     match msg {
///         ServerToClient::ClaudeOutput { content } => { /* ... */ }
///     }
/// }
/// ```
pub fn connect<E: WsEndpoint>() -> Result<Connection<E>, ConnectError> {
    let url = derive_ws_url(E::PATH);
    connect_to::<E>(&url)
}

/// Connects to a specific WebSocket URL.
///
/// Use this when you need a custom URL (e.g., a different host).
pub fn connect_to<E: WsEndpoint>(url: &str) -> Result<Connection<E>, ConnectError> {
    let ws = WebSocket::open(url)
        .map_err(|e| ConnectError(gloo_net::websocket::WebSocketError::MessageSendError(e)))?;
    let (sink, stream) = ws.split();
    Ok(Connection {
        sink,
        stream,
        _types: PhantomData,
    })
}

/// A browser-side typed WebSocket connection.
///
/// Sends `E::ClientMsg` and receives `E::ServerMsg` (flipped from server).
///
/// This is a separate type from the transport-agnostic `WsConnection`
/// because browser WebSocket types are not `Send` (WASM is single-threaded).
pub struct Connection<E: WsEndpoint> {
    sink: futures_util::stream::SplitSink<WebSocket, Message>,
    stream: futures_util::stream::SplitStream<WebSocket>,
    _types: PhantomData<E>,
}

impl<E: WsEndpoint> Connection<E> {
    pub async fn send(&mut self, msg: E::ClientMsg) -> Result<(), SendError> {
        let ws_msg = msg.encode()?;
        let gloo_msg = match ws_msg {
            WsMessage::Text(t) => Message::Text(t),
            WsMessage::Binary(b) => Message::Bytes(b),
        };
        self.sink
            .send(gloo_msg)
            .await
            .map_err(|_| SendError::Closed)
    }

    pub async fn recv(&mut self) -> Option<Result<E::ServerMsg, RecvError>> {
        loop {
            match self.stream.next().await {
                None => return None,
                Some(Err(_)) => return Some(Err(RecvError::Closed)),
                Some(Ok(msg)) => {
                    let ws_msg = match msg {
                        Message::Text(t) => WsMessage::Text(t),
                        Message::Bytes(b) => WsMessage::Binary(b),
                    };
                    return Some(E::ServerMsg::decode(ws_msg).map_err(RecvError::Decode));
                }
            }
        }
    }

    /// Splits the connection into a sender and receiver.
    pub fn split(self) -> (Sender<E>, Receiver<E>) {
        (
            Sender {
                sink: self.sink,
                _msg: PhantomData,
            },
            Receiver {
                stream: self.stream,
                _msg: PhantomData,
            },
        )
    }
}

/// The sending half of a browser WebSocket connection.
pub struct Sender<E: WsEndpoint> {
    sink: futures_util::stream::SplitSink<WebSocket, Message>,
    _msg: PhantomData<E>,
}

impl<E: WsEndpoint> Sender<E> {
    pub async fn send(&mut self, msg: E::ClientMsg) -> Result<(), SendError> {
        let ws_msg = msg.encode()?;
        let gloo_msg = match ws_msg {
            WsMessage::Text(t) => Message::Text(t),
            WsMessage::Binary(b) => Message::Bytes(b),
        };
        self.sink
            .send(gloo_msg)
            .await
            .map_err(|_| SendError::Closed)
    }
}

/// The receiving half of a browser WebSocket connection.
pub struct Receiver<E: WsEndpoint> {
    stream: futures_util::stream::SplitStream<WebSocket>,
    _msg: PhantomData<E>,
}

impl<E: WsEndpoint> Receiver<E> {
    pub async fn recv(&mut self) -> Option<Result<E::ServerMsg, RecvError>> {
        loop {
            match self.stream.next().await {
                None => return None,
                Some(Err(_)) => return Some(Err(RecvError::Closed)),
                Some(Ok(msg)) => {
                    let ws_msg = match msg {
                        Message::Text(t) => WsMessage::Text(t),
                        Message::Bytes(b) => WsMessage::Binary(b),
                    };
                    return Some(E::ServerMsg::decode(ws_msg).map_err(RecvError::Decode));
                }
            }
        }
    }
}

/// Derives a WebSocket URL from the current page's protocol and host.
///
/// `http:` → `ws:`, `https:` → `wss:`.
fn derive_ws_url(path: &str) -> String {
    let window = web_sys::window().expect("no window object");
    let location = window.location();
    let protocol = location.protocol().expect("no protocol");
    let host = location.host().expect("no host");

    let ws_protocol = if protocol == "https:" { "wss:" } else { "ws:" };
    format!("{ws_protocol}//{host}{path}")
}
