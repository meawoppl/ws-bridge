//! Example: native WebSocket client using ws-bridge.
//!
//! Run with:
//!   cargo run --example echo_client --features native-client
//!
//! Make sure the echo_server example is running first.

mod common;

use common::{ClientMsg, EchoEndpoint, ServerMsg};

#[tokio::main]
async fn main() {
    let mut conn = ws_bridge::native_client::connect::<EchoEndpoint>("ws://127.0.0.1:3000")
        .await
        .expect("Failed to connect");

    println!("Connected to server");

    // Read the welcome message
    match conn.recv().await {
        Some(Ok(ServerMsg::Welcome { message })) => println!("Server: {message}"),
        other => panic!("Expected Welcome, got: {other:?}"),
    }

    // Send a few messages
    for text in ["Hello", "World", "ws-bridge is great"] {
        conn.send(ClientMsg::Say { text: text.into() })
            .await
            .unwrap();

        match conn.recv().await {
            Some(Ok(ServerMsg::Echo { payload })) => println!("Echo: {payload}"),
            other => panic!("Expected Echo, got: {other:?}"),
        }
    }

    // Say goodbye
    conn.send(ClientMsg::Quit).await.unwrap();
    println!("Done!");
}
