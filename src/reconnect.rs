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
    /// Multiplier applied after each failed attempt. Non-finite values and
    /// values below 1 are treated as 1; the initial delay is capped at `max`.
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
        // Public configuration must never let NaN, overflow, or a large attempt
        // counter panic inside the retry loop. Invalid multipliers use 1.0.
        let multiplier = if self.multiplier.is_finite() && self.multiplier >= 1.0 {
            self.multiplier
        } else {
            1.0
        };
        let initial = self.initial.min(self.max);
        if initial.is_zero() {
            return Duration::ZERO;
        }
        let seconds = initial.as_secs_f64() * multiplier.powf(f64::from(attempt));
        if seconds >= self.max.as_secs_f64() {
            return self.max;
        }
        Duration::try_from_secs_f64(seconds).unwrap_or(self.max)
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
    S: WsCodec,
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
    S: WsCodec,
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
    /// Encoding runs once, before attempting to connect. Connection failures
    /// are retried indefinitely. Delivery can be duplicated when a send fails
    /// after the peer received it; use application message IDs for deduplication.
    /// Wrap operations in `tokio::time::timeout` when bounded retries are needed.
    pub async fn send(&mut self, msg: S) -> Result<(), SendError> {
        let encoded = msg.encode()?;
        loop {
            if self.conn.is_none() {
                self.reconnect().await;
            }
            if let Some(ref mut conn) = self.conn {
                match conn.send_encoded(encoded.clone()).await {
                    Ok(()) => {
                        return Ok(());
                    }
                    Err(e @ SendError::Encode(_)) => return Err(e),
                    Err(SendError::Closed | SendError::Transport(_)) => {
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
    /// Connection closures and transport errors trigger
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
                    Some(Err(RecvError::Closed | RecvError::Transport(_))) | None => {
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{BoxFuture, ErasedSink, ErasedStream};
    use crate::TransportError;
    use crate::{DecodeError, EncodeError, WsMessage};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct Sink;
    impl ErasedSink for Sink {
        fn send(&mut self, _: WsMessage) -> BoxFuture<'_, Result<(), TransportError>> {
            Box::pin(async { Ok(()) })
        }
        fn close(&mut self) -> BoxFuture<'_, Result<(), TransportError>> {
            Box::pin(async { Ok(()) })
        }
    }
    struct ClosedStream;
    impl ErasedStream for ClosedStream {
        fn next(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, TransportError>>> {
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

    #[test]
    fn backoff_handles_extreme_configuration() {
        for multiplier in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1.0, 2.0, f64::MAX] {
            let config = BackoffConfig {
                initial: Duration::from_secs(2),
                max: Duration::from_secs(1),
                multiplier,
            };
            for attempt in [0, 1, u32::MAX] {
                assert!(config.delay_for_attempt(attempt) <= config.max);
            }
        }
        let config = BackoffConfig {
            initial: Duration::ZERO,
            max: Duration::MAX,
            multiplier: 2.0,
        };
        assert_eq!(config.delay_for_attempt(u32::MAX), Duration::ZERO);
    }

    #[tokio::test]
    async fn invalid_messages_do_not_connect() {
        let mut ws = ReconnectingWs::new(BackoffConfig::default(), || async {
            panic!("invalid messages must not call the connection factory");
            #[allow(unreachable_code)]
            None::<WsConnection<Message, Message>>
        });
        assert!(matches!(
            ws.send(Message { invalid: true }).await,
            Err(SendError::Encode(_))
        ));
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
