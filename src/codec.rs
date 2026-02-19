use std::fmt;

/// A WebSocket message — either text or binary.
///
/// This is the library's own message type, independent of any specific
/// WebSocket implementation. Feature-gated modules convert to/from
/// their native message types internally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
}

/// Error encoding a message into a WebSocket frame.
#[derive(Debug)]
pub enum EncodeError {
    Json(serde_json::Error),
    Custom(String),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::Json(e) => write!(f, "JSON encode error: {e}"),
            EncodeError::Custom(msg) => write!(f, "encode error: {msg}"),
        }
    }
}

impl std::error::Error for EncodeError {}

impl From<serde_json::Error> for EncodeError {
    fn from(e: serde_json::Error) -> Self {
        EncodeError::Json(e)
    }
}

/// Error decoding a WebSocket frame into a typed message.
#[derive(Debug)]
pub enum DecodeError {
    Json(serde_json::Error),
    UnexpectedBinary,
    UnexpectedText,
    InvalidData(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Json(e) => write!(f, "JSON decode error: {e}"),
            DecodeError::UnexpectedBinary => write!(f, "expected text frame, got binary"),
            DecodeError::UnexpectedText => write!(f, "expected binary frame, got text"),
            DecodeError::InvalidData(msg) => write!(f, "invalid data: {msg}"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<serde_json::Error> for DecodeError {
    fn from(e: serde_json::Error) -> Self {
        DecodeError::Json(e)
    }
}

/// Encode/decode messages to/from WebSocket frames.
///
/// A blanket implementation covers anything that implements
/// `Serialize + DeserializeOwned`, encoding as JSON text frames.
/// Implement manually for binary protocols.
pub trait WsCodec: Sized {
    fn encode(&self) -> Result<WsMessage, EncodeError>;
    fn decode(msg: WsMessage) -> Result<Self, DecodeError>;
}

/// Blanket impl: JSON text encoding for any serde type.
impl<T: serde::Serialize + serde::de::DeserializeOwned> WsCodec for T {
    fn encode(&self) -> Result<WsMessage, EncodeError> {
        Ok(WsMessage::Text(serde_json::to_string(self)?))
    }

    fn decode(msg: WsMessage) -> Result<Self, DecodeError> {
        match msg {
            WsMessage::Text(text) => Ok(serde_json::from_str(&text)?),
            WsMessage::Binary(_) => Err(DecodeError::UnexpectedBinary),
        }
    }
}

/// A type representing no messages. Use this as `ClientMsg` or `ServerMsg`
/// for endpoints that are unidirectional (e.g., server-push-only streams).
///
/// Does not implement `Serialize`/`DeserializeOwned`, so it won't conflict
/// with the blanket JSON impl. Encoding always fails; decoding always fails.
pub enum NoMessages {}

impl Clone for NoMessages {
    fn clone(&self) -> Self {
        match *self {}
    }
}

impl WsCodec for NoMessages {
    fn encode(&self) -> Result<WsMessage, EncodeError> {
        match *self {}
    }

    fn decode(_msg: WsMessage) -> Result<Self, DecodeError> {
        Err(DecodeError::InvalidData(
            "this endpoint does not accept messages in this direction".into(),
        ))
    }
}
