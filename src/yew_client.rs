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
pub struct ConnectError(Box<dyn std::error::Error>);

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WebSocket connect error: {:?}", self.0)
    }
}

impl std::error::Error for ConnectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

fn location_error(message: &str) -> ConnectError {
    ConnectError(Box::new(std::io::Error::other(message.to_owned())))
}

/// Connects to a WebSocket endpoint, deriving `ws://`/`wss://` from the
/// page's current protocol and host.
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
/// # async fn example() -> Result<(), ws_bridge::SendError> {
/// let mut conn = ws_bridge::yew_client::connect::<SessionSocket>().unwrap();
/// conn.send("hello".to_owned()).await?;
/// while let Some(Ok(msg)) = conn.recv().await {
///     println!("{msg}");
/// }
/// # Ok(())
/// # }
/// ```
pub fn connect<E: WsEndpoint>() -> Result<Connection<E>, ConnectError> {
    let url = derive_ws_url(E::PATH)?;
    connect_to::<E>(&url)
}

/// Connects to a specific WebSocket URL.
///
/// Use this when you need a custom URL (e.g., a different host).
pub fn connect_to<E: WsEndpoint>(url: &str) -> Result<Connection<E>, ConnectError> {
    let ws = WebSocket::open(url).map_err(|e| {
        ConnectError(Box::new(
            gloo_net::websocket::WebSocketError::MessageSendError(e),
        ))
    })?;
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
    /// Encodes and sends a client message. Cancellation may leave delivery uncertain.
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

    /// Initiates the close handshake; continue receiving until the stream ends.
    pub async fn close(&mut self) -> Result<(), SendError> {
        self.sink.close().await.map_err(|_| SendError::Closed)
    }

    /// Receives a server message; malformed payloads produce recoverable decode errors.
    pub async fn recv(&mut self) -> Option<Result<E::ServerMsg, RecvError>> {
        match self.stream.next().await {
            None => None,
            Some(Err(_)) => Some(Err(RecvError::Closed)),
            Some(Ok(msg)) => {
                let ws_msg = match msg {
                    Message::Text(t) => WsMessage::Text(t),
                    Message::Bytes(b) => WsMessage::Binary(b),
                };
                Some(E::ServerMsg::decode(ws_msg).map_err(RecvError::Decode))
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
    /// Initiates the close handshake.
    pub async fn close(&mut self) -> Result<(), SendError> {
        self.sink.close().await.map_err(|_| SendError::Closed)
    }

    /// Encodes and sends a client message. Cancellation may leave delivery uncertain.
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
    /// Receives a server message; malformed payloads produce recoverable decode errors.
    pub async fn recv(&mut self) -> Option<Result<E::ServerMsg, RecvError>> {
        match self.stream.next().await {
            None => None,
            Some(Err(_)) => Some(Err(RecvError::Closed)),
            Some(Ok(msg)) => {
                let ws_msg = match msg {
                    Message::Text(t) => WsMessage::Text(t),
                    Message::Bytes(b) => WsMessage::Binary(b),
                };
                Some(E::ServerMsg::decode(ws_msg).map_err(RecvError::Decode))
            }
        }
    }
}

/// Derives a WebSocket URL from the current page's protocol and host.
///
/// `http:` → `ws:`, `https:` → `wss:`.
fn derive_ws_url(path: &str) -> Result<String, ConnectError> {
    let window = web_sys::window().ok_or_else(|| location_error("no browser window"))?;
    let location = window.location();
    let protocol = location
        .protocol()
        .map_err(|_| location_error("cannot read page protocol"))?;
    let host = location
        .host()
        .map_err(|_| location_error("cannot read page host"))?;
    page_ws_url(&protocol, &host, path)
}

fn page_ws_url(protocol: &str, host: &str, path: &str) -> Result<String, ConnectError> {
    let ws_protocol = match protocol {
        "http:" => "ws:",
        "https:" => "wss:",
        _ => return Err(location_error("page must use http or https")),
    };
    if host.is_empty() || !path.starts_with('/') || path.starts_with("//") {
        return Err(location_error(
            "endpoint requires a host and an absolute path",
        ));
    }
    Ok(format!("{ws_protocol}//{host}{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_urls_are_validated() {
        assert_eq!(
            page_ws_url("https:", "localhost:8080", "/ws").unwrap(),
            "wss://localhost:8080/ws"
        );
        for (protocol, host, path) in [
            ("file:", "", "/ws"),
            ("http:", "", "/ws"),
            ("http:", "host", "ws"),
            ("http:", "host", "//other"),
        ] {
            assert!(page_ws_url(protocol, host, path).is_err());
        }
    }
}
