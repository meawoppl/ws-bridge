# Custom Codecs

By default, any type implementing `serde::Serialize + serde::de::DeserializeOwned` automatically gets a `WsCodec` implementation that encodes as JSON text frames. For binary protocols, implement `WsCodec` manually.

## The `WsCodec` trait

```rust
pub trait WsCodec: Sized {
    fn encode(&self) -> Result<WsMessage, EncodeError>;
    fn decode(msg: WsMessage) -> Result<Self, DecodeError>;
}
```

`WsMessage` is either `Text(String)` or `Binary(Vec<u8>)`.

## Default: JSON text frames

If your message types derive `Serialize` and `Deserialize`, they automatically encode as JSON text frames:

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    Welcome { message: String },
    Echo { payload: String },
}

// WsCodec is automatically implemented:
// - encode() → WsMessage::Text(json_string)
// - decode(WsMessage::Text(s)) → Ok(parsed)
// - decode(WsMessage::Binary(_)) → Err(DecodeError::UnexpectedBinary)
```

## Binary protocols

For binary data (images, audio, custom wire formats), implement `WsCodec` manually. **Do not** derive `Serialize`/`Deserialize` on the type, as that would conflict with the blanket JSON impl.

### Example: camera frame

A camera frame with a header (width, height, frame number) followed by JPEG data:

```rust
use ws_bridge::{WsCodec, WsMessage, EncodeError, DecodeError};

pub struct CameraFrame {
    pub width: u32,
    pub height: u32,
    pub frame_number: u64,
    pub jpeg_data: Vec<u8>,
}

impl Clone for CameraFrame {
    fn clone(&self) -> Self {
        CameraFrame {
            width: self.width,
            height: self.height,
            frame_number: self.frame_number,
            jpeg_data: self.jpeg_data.clone(),
        }
    }
}

impl WsCodec for CameraFrame {
    fn encode(&self) -> Result<WsMessage, EncodeError> {
        let mut buf = Vec::with_capacity(16 + self.jpeg_data.len());
        buf.extend_from_slice(&self.width.to_le_bytes());
        buf.extend_from_slice(&self.height.to_le_bytes());
        buf.extend_from_slice(&self.frame_number.to_le_bytes());
        buf.extend_from_slice(&self.jpeg_data);
        Ok(WsMessage::Binary(buf))
    }

    fn decode(msg: WsMessage) -> Result<Self, DecodeError> {
        match msg {
            WsMessage::Binary(data) if data.len() >= 16 => {
                let width = u32::from_le_bytes(data[0..4].try_into().unwrap());
                let height = u32::from_le_bytes(data[4..8].try_into().unwrap());
                let frame_number = u64::from_le_bytes(data[8..16].try_into().unwrap());
                let jpeg_data = data[16..].to_vec();
                Ok(CameraFrame { width, height, frame_number, jpeg_data })
            }
            WsMessage::Binary(_) => Err(DecodeError::InvalidData(
                "frame too short: need at least 16-byte header".into(),
            )),
            WsMessage::Text(_) => Err(DecodeError::UnexpectedText),
        }
    }
}
```

### Using it in an endpoint

```rust
use ws_bridge::{WsEndpoint, NoMessages};

pub struct CameraStream;

impl WsEndpoint for CameraStream {
    const PATH: &'static str = "/ws/camera";
    type ServerMsg = CameraFrame;    // Binary frames from server
    type ClientMsg = NoMessages;     // Server-push only, no client messages
}
```

## `NoMessages` for unidirectional endpoints

Use `NoMessages` as `ClientMsg` or `ServerMsg` for endpoints where data flows in only one direction:

```rust
use ws_bridge::{WsEndpoint, NoMessages};

// Server pushes events, client never sends
pub struct EventStream;
impl WsEndpoint for EventStream {
    const PATH: &'static str = "/ws/events";
    type ServerMsg = EventData;      // Server sends these
    type ClientMsg = NoMessages;     // Client never sends
}
```

`NoMessages` is an uninhabited enum (no variants). It implements `WsCodec` such that:
- `encode()` is unreachable (you can't construct a `NoMessages` value)
- `decode()` always returns `Err(DecodeError::InvalidData(...))`

## Error types

| Error variant | Meaning |
|---|---|
| `EncodeError::Json(e)` | `serde_json::to_string` failed (blanket impl) |
| `EncodeError::Custom(msg)` | Custom encode failure (for manual impls) |
| `DecodeError::Json(e)` | `serde_json::from_str` failed (blanket impl) |
| `DecodeError::UnexpectedBinary` | Got binary frame, expected text (JSON impl) |
| `DecodeError::UnexpectedText` | Got text frame, expected binary (custom impl) |
| `DecodeError::InvalidData(msg)` | Custom decode failure (e.g., header too short) |

## Tips

- **Don't derive Serialize/Deserialize on binary types.** The blanket JSON impl will kick in and conflict with your manual `WsCodec`.
- **Use `InvalidData` for validation errors** in your `decode()` impl — it carries a descriptive string.
- **`Clone` is required** on both `ServerMsg` and `ClientMsg`. For binary types with large payloads, consider `Arc<Vec<u8>>` or similar if cloning is expensive and you need to broadcast.
