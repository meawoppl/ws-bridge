use std::future::Future;
use std::marker::PhantomData;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::{self, MethodRouter};
use futures_util::stream::StreamExt;
use futures_util::SinkExt;

use crate::codec::WsMessage;
use crate::connection::{ErasedSink, ErasedStream, WsConnection};
use crate::WsEndpoint;

/// A server-side typed WebSocket connection.
///
/// Sends `E::ServerMsg` and receives `E::ClientMsg`.
pub type Connection<E> = WsConnection<<E as WsEndpoint>::ServerMsg, <E as WsEndpoint>::ClientMsg>;

/// Returns an axum [`MethodRouter`] that handles WebSocket upgrades for
/// the given endpoint type.
///
/// This is the highest-level entry point for server-side ws-bridge usage.
/// Pass it directly to [`axum::Router::route`].
///
/// # Example
///
/// ```ignore
/// use axum::Router;
///
/// let app = Router::new()
///     .route(
///         SessionSocket::PATH,
///         ws_bridge::server::handler::<SessionSocket, _, _>(|mut conn| async move {
///             while let Some(Ok(msg)) = conn.recv().await {
///                 // msg is SessionSocket::ClientMsg
///             }
///         }),
///     );
/// ```
pub fn handler<E, F, Fut>(callback: F) -> MethodRouter
where
    E: WsEndpoint,
    F: FnOnce(Connection<E>) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    routing::get(move |ws: WebSocketUpgrade| async move {
        let cb = callback;
        upgrade::<E, F, Fut>(ws, cb)
    })
}

/// Returns an axum [`MethodRouter`] that handles WebSocket upgrades,
/// with access to shared application state via axum's `State` extractor.
///
/// # Example
///
/// ```ignore
/// use axum::Router;
/// use std::sync::Arc;
///
/// struct AppState { /* ... */ }
///
/// let app = Router::new()
///     .route(
///         SessionSocket::PATH,
///         ws_bridge::server::handler_with_state::<SessionSocket, _, _, Arc<AppState>>(
///             |mut conn, state| async move {
///                 // `state` is Arc<AppState>
///                 while let Some(Ok(msg)) = conn.recv().await {
///                     // ...
///                 }
///             },
///         ),
///     )
///     .with_state(Arc::new(AppState { /* ... */ }));
/// ```
pub fn handler_with_state<E, F, Fut, S>(callback: F) -> MethodRouter<S>
where
    E: WsEndpoint,
    S: Clone + Send + Sync + 'static,
    F: FnOnce(Connection<E>, S) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    routing::get(
        move |ws: WebSocketUpgrade, axum::extract::State(state): axum::extract::State<S>| async move {
            let cb = callback;
            ws.on_upgrade(move |socket| async move {
                let conn = wrap_axum_socket::<E>(socket);
                cb(conn, state).await;
            })
        },
    )
}

/// Upgrades an axum `WebSocketUpgrade` with a typed handler.
///
/// Use this when you need to handle the upgrade manually inside your
/// own axum handler function (e.g., to use custom extractors or
/// perform pre-upgrade authentication).
///
/// # Example
///
/// ```ignore
/// use axum::extract::ws::WebSocketUpgrade;
/// use axum::response::Response;
///
/// async fn handle_session(ws: WebSocketUpgrade) -> Response {
///     ws_bridge::server::upgrade::<SessionSocket>(ws, |mut conn| async move {
///         while let Some(Ok(msg)) = conn.recv().await {
///             // msg is SessionSocket::ClientMsg
///         }
///     })
/// }
/// ```
pub fn upgrade<E, F, Fut>(ws: WebSocketUpgrade, callback: F) -> Response
where
    E: WsEndpoint,
    F: FnOnce(Connection<E>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    ws.on_upgrade(move |socket| async move {
        let conn = wrap_axum_socket::<E>(socket);
        callback(conn).await;
    })
}

/// Wraps an axum `WebSocket` into a typed `Connection`.
///
/// Use this if you need more control than [`upgrade`] provides
/// (e.g., configuring the `WebSocketUpgrade` before upgrading).
///
/// # Example
///
/// ```ignore
/// async fn handle(ws: WebSocketUpgrade) -> Response {
///     ws.max_message_size(1024 * 1024)
///         .on_upgrade(|socket| async move {
///             let mut conn = ws_bridge::server::into_connection::<SessionSocket>(socket);
///             while let Some(Ok(msg)) = conn.recv().await {
///                 // ...
///             }
///         })
/// }
/// ```
pub fn into_connection<E: WsEndpoint>(socket: WebSocket) -> Connection<E> {
    wrap_axum_socket::<E>(socket)
}

fn wrap_axum_socket<E: WsEndpoint>(socket: WebSocket) -> Connection<E> {
    let (sink, stream) = socket.split();
    WsConnection {
        sink: Box::new(AxumSink(sink)),
        stream: Box::new(AxumStream(stream)),
        _types: PhantomData,
    }
}

// -- Axum transport adapters --

struct AxumSink(futures_util::stream::SplitSink<WebSocket, Message>);

impl ErasedSink for AxumSink {
    fn send(
        &mut self,
        msg: WsMessage,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), ()>> + Send + '_>> {
        Box::pin(async move {
            let axum_msg = match msg {
                WsMessage::Text(t) => Message::Text(t),
                WsMessage::Binary(b) => Message::Binary(b),
            };
            self.0.send(axum_msg).await.map_err(|_| ())
        })
    }

    fn close(
        &mut self,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), ()>> + Send + '_>> {
        Box::pin(async move { self.0.close().await.map_err(|_| ()) })
    }
}

struct AxumStream(futures_util::stream::SplitStream<WebSocket>);

impl ErasedStream for AxumStream {
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
                        Message::Text(t) => return Some(Ok(WsMessage::Text(t))),
                        Message::Binary(b) => return Some(Ok(WsMessage::Binary(b))),
                        Message::Close(_) => return None,
                        // Skip ping/pong — axum handles these automatically.
                        Message::Ping(_) | Message::Pong(_) => continue,
                    },
                }
            }
        })
    }
}
