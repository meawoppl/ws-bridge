//! Typed WebSocket connections for Rocket using `rocket_ws`.
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
//! fn handle(ws: rocket_ws::WebSocket) -> rocket_ws::Channel<'static> {
//!     ws_bridge::rocket_server::upgrade::<Echo, _, _>(ws, |mut conn| async move {
//!         while let Some(Ok(msg)) = conn.recv().await {
//!             if conn.send(msg).await.is_err() { break; }
//!         }
//!         let (mut tx, _) = conn.split();
//!         let _ = tx.close().await;
//!     })
//! }
//! ```
use crate::{
    transport::{self, Frame},
    WsConnection, WsEndpoint, WsMessage,
};
use rocket_ws::{stream::DuplexStream, Channel, Message, WebSocket};
use std::future::Future;

/// A server-side typed connection.
pub type Connection<E> = WsConnection<<E as WsEndpoint>::ServerMsg, <E as WsEndpoint>::ClientMsg>;

/// Creates a Rocket channel with a typed callback.
/// Return this channel from a Rocket route receiving a `rocket_ws::WebSocket`.
pub fn upgrade<E, F, Fut>(ws: WebSocket, callback: F) -> Channel<'static>
where
    E: WsEndpoint,
    F: FnOnce(Connection<E>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    ws.channel(move |socket| {
        Box::pin(async move {
            callback(into_connection::<E>(socket)).await;
            Ok(())
        })
    })
}

/// Wraps an already upgraded Rocket socket.
pub fn into_connection<E: WsEndpoint>(socket: DuplexStream) -> Connection<E> {
    transport::into_connection::<E, _, _, _>(socket, encode, decode)
}
fn encode(msg: WsMessage) -> Message {
    match msg {
        WsMessage::Text(t) => Message::Text(t),
        WsMessage::Binary(b) => Message::Binary(b),
    }
}
fn decode(msg: Message) -> Frame {
    match msg {
        Message::Text(t) => Frame::Message(WsMessage::Text(t)),
        Message::Binary(b) => Frame::Message(WsMessage::Binary(b)),
        Message::Close(_) => Frame::Close,
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Frame::Control,
    }
}
