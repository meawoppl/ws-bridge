//! Typed WebSocket connections for Salvo. State and authentication stay in
//! standard Salvo handlers; pass a configured upgrade to [`upgrade`].
//!
//! # Example
//!
//! ```no_run
//! # use ws_bridge::WsEndpoint;
//! # struct Echo;
//! # impl WsEndpoint for Echo {
//! #     const PATH: &'static str = "/ws/echo";
//! #     type ServerMsg = String;
//! #     type ClientMsg = String;
//! # }
//! #[salvo::handler]
//! async fn handle(req: &mut salvo::Request, res: &mut salvo::Response)
//!     -> Result<(), salvo::http::StatusError>
//! {
//!     ws_bridge::salvo_server::upgrade::<Echo, _, _>(
//!         salvo::websocket::WebSocketUpgrade::new(), req, res,
//!         |mut conn| async move {
//!             while let Some(Ok(msg)) = conn.recv().await {
//!                 if conn.send(msg).await.is_err() { break; }
//!             }
//!             let (mut tx, _) = conn.split();
//!             let _ = tx.close().await;
//!         },
//!     ).await
//! }
//! ```
use crate::{
    transport::{self, Frame},
    WsConnection, WsEndpoint, WsMessage,
};
use salvo::websocket::{Message, WebSocket, WebSocketUpgrade};
use std::future::Future;

/// A server-side typed connection.
pub type Connection<E> = WsConnection<<E as WsEndpoint>::ServerMsg, <E as WsEndpoint>::ClientMsg>;

/// Completes a configured Salvo upgrade with a typed callback.
pub async fn upgrade<E, F, Fut>(
    ws: WebSocketUpgrade,
    req: &mut salvo::Request,
    res: &mut salvo::Response,
    callback: F,
) -> Result<(), salvo::http::StatusError>
where
    E: WsEndpoint,
    F: FnOnce(Connection<E>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    ws.upgrade(req, res, move |socket| {
        callback(into_connection::<E>(socket))
    })
    .await
}

/// Wraps an already upgraded Salvo socket.
pub fn into_connection<E: WsEndpoint>(socket: WebSocket) -> Connection<E> {
    transport::into_connection::<E, _, _, _>(socket, encode, decode)
}
fn encode(msg: WsMessage) -> Message {
    match msg {
        WsMessage::Text(t) => Message::text(t),
        WsMessage::Binary(b) => Message::binary(b),
    }
}
fn decode(msg: Message) -> Frame {
    if msg.is_close() {
        Frame::Close
    } else if msg.is_text() {
        Frame::Message(WsMessage::Text(
            msg.as_str().expect("validated text frame").to_owned(),
        ))
    } else if msg.is_binary() {
        Frame::Message(WsMessage::Binary(msg.as_bytes().to_vec()))
    } else {
        Frame::Control
    }
}
