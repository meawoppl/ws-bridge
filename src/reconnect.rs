use std::future::Future;
use std::marker::PhantomData;
use std::time::Duration;

use crate::codec::WsCodec;
use crate::connection::{RecvError, SendError, WsConnection};
/// Configuration for exponential backoff reconnection.
#[derive(Debug, Clone)]
pub struct BackoffConfig {
    /// Initial delay before the first reconnection attempt.
    pub initial: Duration,
    /// Maximum delay between reconnection attempts.
    pub max: Duration,
    /// Multiplier applied to the delay after each failed attempt.
    pub multiplier: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        BackoffConfig {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(30),
            multiplier: 2.0,
        }
    }
}

impl BackoffConfig {
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay_secs = self.initial.as_secs_f64() * self.multiplier.powi(attempt as i32);
        let clamped = delay_secs.min(self.max.as_secs_f64());
        Duration::from_secs_f64(clamped)
    }
}

/// A reconnecting WebSocket wrapper.
///
/// Automatically reconnects with exponential backoff when the connection
/// drops. Use the connection factory for setup that must run on every
/// connection. Retry backoff resets after a connection lasts 30 seconds.
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
/// # async fn connect() -> Option<ws_bridge::WsConnection<String, String>> { None }
/// # async fn example() {
/// use ws_bridge::reconnect::{ReconnectingWs, BackoffConfig};
/// use std::time::Duration;
///
/// let config = BackoffConfig {
///     initial: Duration::from_millis(500),
///     max: Duration::from_secs(30),
///     multiplier: 2.0,
/// };
///
/// let mut ws = ReconnectingWs::new(
///     config,
///     connect,
/// );
///
/// while let Some(result) = ws.recv().await {
///     match result {
///         Ok(msg) => { /* handle message */ }
///         Err(e) => { /* decode error */ }
///     }
/// }
/// # }
/// ```
pub struct ReconnectingWs<S, R, F, Fut>
where
    S: WsCodec + Clone,
    R: WsCodec,
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<WsConnection<S, R>>>,
{
    config: BackoffConfig,
    connect_fn: F,
    conn: Option<WsConnection<S, R>>,
    attempt: u32,
    connected_at: Option<tokio::time::Instant>,
    _types: PhantomData<(S, R)>,
}

impl<S, R, F, Fut> ReconnectingWs<S, R, F, Fut>
where
    S: WsCodec + Clone,
    R: WsCodec,
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<WsConnection<S, R>>>,
{
    /// Creates a new reconnecting WebSocket.
    ///
    /// The `connect_fn` is called to establish (or re-establish) the connection.
    /// It should return `Some(connection)` on success, `None` on failure.
    pub fn new(config: BackoffConfig, connect_fn: F) -> Self {
        ReconnectingWs {
            config,
            connect_fn,
            conn: None,
            attempt: 0,
            connected_at: None,
            _types: PhantomData,
        }
    }

    /// Sends a message, reconnecting if necessary.
    ///
    /// Encoding errors are returned immediately without dropping the connection.
    /// Connection failures are retried indefinitely.
    pub async fn send(&mut self, msg: S) -> Result<(), SendError> {
        loop {
            if self.conn.is_none() {
                self.reconnect().await;
            }
            if let Some(ref mut conn) = self.conn {
                match conn.send(msg.clone()).await {
                    Ok(()) => {
                        return Ok(());
                    }
                    Err(e @ SendError::Encode(_)) => return Err(e),
                    Err(SendError::Closed) => {
                        self.drop_connection();
                        continue;
                    }
                }
            }
            return Err(SendError::Closed);
        }
    }

    /// Receives a message, reconnecting if the connection drops.
    ///
    /// Decode errors (`RecvError::Decode`) are returned to the caller.
    /// Connection closures (`None` or `RecvError::Closed`) trigger
    /// automatic reconnection.
    ///
    /// Returns `None` only if reconnection permanently fails (which
    /// currently never happens — it retries indefinitely).
    pub async fn recv(&mut self) -> Option<Result<R, RecvError>> {
        loop {
            if self.conn.is_none() {
                self.reconnect().await;
            }
            if let Some(ref mut conn) = self.conn {
                match conn.recv().await {
                    Some(Ok(msg)) => {
                        return Some(Ok(msg));
                    }
                    Some(Err(RecvError::Decode(e))) => {
                        return Some(Err(RecvError::Decode(e)));
                    }
                    Some(Err(RecvError::Closed)) | None => {
                        // Connection lost, reconnect.
                        self.drop_connection();
                        continue;
                    }
                }
            }
        }
    }

    /// Forces a reconnection, dropping the current connection if any.
    pub async fn reconnect(&mut self) {
        self.drop_connection();
        loop {
            if self.attempt > 0 {
                let delay = self
                    .config
                    .delay_for_attempt(self.attempt.saturating_sub(1));
                tokio::time::sleep(delay).await;
            }
            self.attempt = self.attempt.saturating_add(1);

            if let Some(conn) = (self.connect_fn)().await {
                self.conn = Some(conn);
                self.connected_at = Some(tokio::time::Instant::now());
                return;
            }
        }
    }

    fn drop_connection(&mut self) {
        if self
            .connected_at
            .take()
            .is_some_and(|at| at.elapsed() >= Duration::from_secs(30))
        {
            self.attempt = 0;
        }
        self.conn = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{BoxFuture, ErasedSink, ErasedStream};
    use crate::{DecodeError, EncodeError, WsMessage};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct Sink;
    impl ErasedSink for Sink {
        fn send(&mut self, _: WsMessage) -> BoxFuture<'_, Result<(), ()>> {
            Box::pin(async { Ok(()) })
        }
        fn close(&mut self) -> BoxFuture<'_, Result<(), ()>> {
            Box::pin(async { Ok(()) })
        }
    }
    struct ClosedStream;
    impl ErasedStream for ClosedStream {
        fn next(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, ()>>> {
            Box::pin(async { None })
        }
    }
    fn connection<S: WsCodec, R: WsCodec>() -> WsConnection<S, R> {
        WsConnection {
            sink: Box::new(Sink),
            stream: Box::new(ClosedStream),
            _types: PhantomData,
        }
    }

    #[derive(Clone)]
    struct Message {
        invalid: bool,
    }
    impl WsCodec for Message {
        fn encode(&self) -> Result<WsMessage, EncodeError> {
            if self.invalid {
                Err(EncodeError::Custom("invalid message".into()))
            } else {
                Ok(WsMessage::Text("valid".into()))
            }
        }
        fn decode(_: WsMessage) -> Result<Self, DecodeError> {
            Ok(Self { invalid: false })
        }
    }

    #[tokio::test]
    async fn encoding_error_preserves_connection() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let mut ws = ReconnectingWs::new(BackoffConfig::default(), move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { Some(connection::<Message, Message>()) }
        });
        let result =
            tokio::time::timeout(Duration::from_secs(1), ws.send(Message { invalid: true }))
                .await
                .unwrap();
        assert!(matches!(result, Err(SendError::Encode(_))));
        ws.send(Message { invalid: false }).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn short_connections_keep_backoff_and_stable_connections_reset_it() {
        let mut ws = ReconnectingWs::new(BackoffConfig::default(), || async {
            Some(connection::<Message, Message>())
        });
        ws.reconnect().await;
        let start = tokio::time::Instant::now();
        ws.reconnect().await;
        assert_eq!(start.elapsed(), Duration::from_secs(1));
        let start = tokio::time::Instant::now();
        ws.reconnect().await;
        assert_eq!(start.elapsed(), Duration::from_secs(2));
        tokio::time::sleep(Duration::from_secs(30)).await;
        let start = tokio::time::Instant::now();
        ws.reconnect().await;
        assert_eq!(start.elapsed(), Duration::ZERO);
        let start = tokio::time::Instant::now();
        ws.reconnect().await;
        assert_eq!(start.elapsed(), Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn immediately_closed_connections_do_not_spin() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let mut ws = ReconnectingWs::new(BackoffConfig::default(), move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { Some(connection::<Message, Message>()) }
        });
        assert!(tokio::time::timeout(Duration::from_millis(3500), ws.recv())
            .await
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}

/// Convenience constructor for reconnecting to a native-client endpoint.
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
/// use ws_bridge::reconnect::{self, BackoffConfig};
///
/// let mut ws = reconnect::connect_native::<SessionSocket>(
///     "ws://localhost:3000".into(),
///     BackoffConfig::default(),
/// );
/// ```
#[cfg(feature = "native-client")]
#[allow(clippy::type_complexity)]
pub fn connect_native<E>(
    base_url: String,
    config: BackoffConfig,
) -> ReconnectingWs<
    E::ClientMsg,
    E::ServerMsg,
    impl FnMut() -> std::pin::Pin<
        Box<dyn Future<Output = Option<WsConnection<E::ClientMsg, E::ServerMsg>>> + Send>,
    >,
    std::pin::Pin<
        Box<dyn Future<Output = Option<WsConnection<E::ClientMsg, E::ServerMsg>>> + Send>,
    >,
>
where
    E: crate::WsEndpoint,
{
    ReconnectingWs::new(config, move || {
        let url = base_url.clone();
        Box::pin(async move { crate::native_client::connect::<E>(&url).await.ok() })
            as std::pin::Pin<
                Box<dyn Future<Output = Option<WsConnection<E::ClientMsg, E::ServerMsg>>> + Send>,
            >
    })
}
