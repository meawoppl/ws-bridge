//! Example: echo server using ws-bridge with axum.
//!
//! Run with:
//!   cargo run --example echo_server --features server
//!
//! Then connect with the echo_client example or any WebSocket client.

mod common;

use axum::Router;
use common::{ClientMsg, EchoEndpoint, ServerMsg};
use ws_bridge::WsEndpoint;

#[tokio::main]
async fn main() {
    let app = Router::new().route(
        EchoEndpoint::PATH,
        ws_bridge::server::handler::<EchoEndpoint, _, _>(|mut conn| async move {
            println!("Client connected");

            conn.send(ServerMsg::Welcome {
                message: "Hello! Send me messages and I'll echo them back.".into(),
            })
            .await
            .unwrap();

            while let Some(result) = conn.recv().await {
                match result {
                    Ok(ClientMsg::Say { text }) => {
                        println!("Received: {text}");
                        conn.send(ServerMsg::Echo {
                            payload: text,
                        })
                        .await
                        .unwrap();
                    }
                    Ok(ClientMsg::Quit) => {
                        println!("Client said goodbye");
                        break;
                    }
                    Err(e) => {
                        eprintln!("Decode error: {e}");
                        conn.send(ServerMsg::Error {
                            message: format!("Failed to decode: {e}"),
                        })
                        .await
                        .unwrap();
                    }
                }
            }

            println!("Client disconnected");
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("Echo server listening on ws://127.0.0.1:3000{}", EchoEndpoint::PATH);
    axum::serve(listener, app).await.unwrap();
}
