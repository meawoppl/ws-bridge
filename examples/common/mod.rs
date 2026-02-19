//! Shared endpoint definitions used by both the echo_server and echo_client examples.
//!
//! In a real project, this would live in a shared crate depended on by both
//! the server and client packages.

use serde::{Deserialize, Serialize};
use ws_bridge::WsEndpoint;

/// The echo endpoint — clients send messages, server echoes them back.
pub struct EchoEndpoint;

impl WsEndpoint for EchoEndpoint {
    const PATH: &'static str = "/ws/echo";
    type ServerMsg = ServerMsg;
    type ClientMsg = ClientMsg;
}

/// Messages sent from the server to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    Welcome { message: String },
    Echo { payload: String },
    Error { message: String },
}

/// Messages sent from the client to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    Say { text: String },
    Quit,
}
