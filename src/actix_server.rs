//! Typed WebSockets for Actix Web using `actix-ws` (no actors required).
//!
//! Connections stay on Actix's local runtime beca//!
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
//! async fn handle(req: actix_web::HttpRequest, body: actix_web::web::Payload)
//!     -> actix_web::Result<actix_web::HttpResponse>
//! {
//!     ws_bridge::actix_server::upgrade::<Echo, _, _>(&req, body, |mut conn| async move {
//!         while let Some(Ok(msg)) = conn.recv().await {
//!             if conn.send(msg).await.is_err() { break; }
//!         }
//!     })
//! }
//! ```
use crate::{RecvError, SendError, WsCodec, WsEndpoint, WsMessage};
use actix_ws::{AggregatedMessage, AggregatedMessageStream, MessageStream, Session};
use std::{future::Future, marker::PhantomData};

/// A local server-side typed connection.
pub struct Connection<E: WsEndpoint> {
    sender: Sender<E>,
    receiver: Receiver<E>,
}

/// The sending half of an Actix connection.
pub struct Sender<E: WsEndpoint> {
    session: Option<Session>,
    _endpoint: PhantomData<E>,
}

/// The receiving half of an Actix connection. Ping replies and close replies
/// are handled here; keep polling `recv()` while the connection is active.
pub struct Receiver<E: WsEndpoint> {
    stream: AggregatedMessageStream,
    session: Option<Session>,
    closed: bool,
    _endpoint: PhantomData<E>,
}

/// Upgrades an Actix request and runs a typed callback on the local runtime.
/// Use ordinary Actix extractors before calling this function for auth/state.
pub fn upgrade<E, F, Fut>(
    req: &actix_web::HttpRequest,
    body: actix_web::web::Payload,
    callback: F,
) -> actix_web::Result<actix_web::HttpResponse>
where
    E: WsEndpoint + 'static,
    F: FnOnce(Connection<E>) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    let (response, session, stream) = actix_ws::handle(req, body)?;
    actix_web::rt::spawn(async move {
        let conn = into_connection::<E>(session, stream);
        // Keep a session handle for closing even if the callback splits conn.
        let session = conn.sender.session.as_ref().unwrap().clone();
        callback(conn).await;
        let _ = session.close(None).await;
    });
    Ok(response)
}

/// Wraps the session and stream from `actix_ws::handle` or
/// `actix_ws::handle_with_protocols`, preserving custom upgrade configuration.
pub fn into_connection<E: WsEndpoint>(session: Session, stream: MessageStream) -> Connection<E> {
    into_aggregated_connection(session, stream.aggregate_continuations())
}

/// Wraps a configured aggregated stream, for custom frame/message size limits.
pub fn into_aggregated_connection<E: WsEndpoint>(
    session: Session,
    stream: AggregatedMessageStream,
) -> Connection<E> {
    Connection {
        sender: Sender {
            session: Some(session.clone()),
            _endpoint: PhantomData,
        },
        receiver: Receiver {
            stream,
            session: Some(session),
            closed: false,
            _endpoint: PhantomData,
        },
    }
}

impl<E: WsEndpoint> Connection<E> {
    /// Sends a server message.
    pub async fn send(&mut self, msg: E::ServerMsg) -> Result<(), SendError> {
        self.sender.send(msg).await
    }
    /// Receives a client message.
    pub async fn recv(&mut self) -> Option<Result<E::ClientMsg, RecvError>> {
        self.receiver.recv().await
    }
    /// Splits into independently usable sender and receiver halves.
    pub fn split(self) -> (Sender<E>, Receiver<E>) {
        (self.sender, self.receiver)
    }
}

impl<E: WsEndpoint> Sender<E> {
    /// Sends a server message.
    pub async fn send(&mut self, msg: E::ServerMsg) -> Result<(), SendError> {
        let msg = msg.encode()?;
        let session = self.session.as_mut().ok_or(SendError::Closed)?;
        match msg {
            WsMessage::Text(t) => session.text(t).await,
            WsMessage::Binary(b) => session.binary(b).await,
        }
        .map_err(|_| SendError::Closed)
    }
    /// Sends a close frame and prevents further sends on this half.
    pub async fn close(&mut self) -> Result<(), SendError> {
        self.session
            .take()
            .ok_or(SendError::Closed)?
            .close(None)
            .await
            .map_err(|_| SendError::Closed)
    }
}

impl<E: WsEndpoint> Receiver<E> {
    /// Receives a typed message, aggregating fragmented messages and replying
    /// to ping/close control frames before yielding application data.
    pub async fn recv(&mut self) -> Option<Result<E::ClientMsg, RecvError>> {
        if self.closed {
            return None;
        }
        loop {
            let msg = match self.stream.recv().await {
                None => {
                    self.closed = true;
                    return None;
                }
                Some(Err(_)) => {
                    self.closed = true;
                    return Some(Err(RecvError::Closed));
                }
                Some(Ok(msg)) => msg,
            };
            let data = match msg {
                AggregatedMessage::Text(t) => WsMessage::Text(t.to_string()),
                AggregatedMessage::Binary(b) => WsMessage::Binary(b.to_vec()),
                AggregatedMessage::Ping(b) => {
                    if self.session.as_mut().unwrap().pong(&b).await.is_err() {
                        self.closed = true;
                        return Some(Err(RecvError::Closed));
                    }
                    continue;
                }
                AggregatedMessage::Pong(_) => continue,
                AggregatedMessage::Close(reason) => {
                    self.closed = true;
                    let _ = self.session.take().unwrap().close(reason).await;
                    return None;
                }
            };
            return Some(E::ClientMsg::decode(data).map_err(RecvError::Decode));
        }
    }
}
