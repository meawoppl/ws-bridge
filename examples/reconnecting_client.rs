//! Example: reconnecting native client using ws-bridge.
//!
//! Run with:
//!   cargo run --example reconnecting_client --features "native-client,reconnect"
//!
//! This client automatically reconnects with exponential backoff if the
//! server disconnects. Start/stop the echo_server to see it in action.

mod common;

use std::time::Duration;
use ws_bridge::reconnect::BackoffConfig;

#[allow(unused_imports)]
use common::{ClientMsg, EchoEndpoint, ServerMsg};

#[tokio::main]
async fn main() {
    let config = BackoffConfig {
        initial: Duration::from_secs(1),
        max: Duration::from_secs(30),
        multiplier: 2.0,
    };

    let mut ws = ws_bridge::reconnect::connect_native::<EchoEndpoint>(
        "ws://127.0.0.1:3000".into(),
        config,
    );

    println!("Reconnecting client started. Press Ctrl+C to exit.");
    println!("Start the echo_server example to see messages flow.");

    loop {
        match ws.recv().await {
            Some(Ok(msg)) => {
                println!("Received: {msg:?}");

                // Echo back any welcome message
                if matches!(msg, ServerMsg::Welcome { .. }) {
                    if let Err(e) = ws
                        .send(ClientMsg::Say {
                            text: "Hello from reconnecting client!".into(),
                        })
                        .await
                    {
                        eprintln!("Send error: {e}");
                    }
                }
            }
            Some(Err(e)) => {
                eprintln!("Receive error: {e}");
            }
            None => {
                eprintln!("Connection permanently lost");
                break;
            }
        }
    }
}
