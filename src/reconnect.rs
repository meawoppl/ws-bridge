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
/// drops. Calls the user-provided `on_connect` callback after each
/// successful reconnection.
///
/// # Example
///
/// ```ignore
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
///     || async { ws_bridge::native_client::connect::<MyEndpoint>("ws://localhost:3000").await.ok() },
/// );
///
/// while let Some(result) = ws.recv().await {
///     match result {
///         Ok(msg) => { /* handle message */ }
///         Err(e) => { /* decode error */ }
///     }
/// }
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
            _types: PhantomData,
        }
    }

    /// Sends a message, reconnecting if necessary.
    ///
    /// Returns `Err` if the message could not be sent even after reconnecting.
    pub async fn send(&mut self, msg: S) -> Result<(), SendError> {
        loop {
            if self.conn.is_none() {
                self.reconnect().await;
            }
            if let Some(ref mut conn) = self.conn {
                match conn.send(msg.clone()).await {
                    Ok(()) => {
                        self.attempt = 0;
                        return Ok(());
                    }
                    Err(_) => {
                        self.conn = None;
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
                        self.attempt = 0;
                        return Some(Ok(msg));
                    }
                    Some(Err(RecvError::Decode(e))) => {
                        self.attempt = 0;
                        return Some(Err(RecvError::Decode(e)));
                    }
                    Some(Err(RecvError::Closed)) | None => {
                        // Connection lost, reconnect.
                        self.conn = None;
                        continue;
                    }
                }
            }
        }
    }

    /// Forces a reconnection, dropping the current connection if any.
    pub async fn reconnect(&mut self) {
        self.conn = None;
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
                self.attempt = 0;
                return;
            }
        }
    }
}

/// Convenience constructor for reconnecting to a native-client endpoint.
///
/// # Example
///
/// ```ignore
/// use ws_bridge::reconnect::{self, BackoffConfig};
///
/// let mut ws = reconnect::connect_native::<MyEndpoint>(
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
