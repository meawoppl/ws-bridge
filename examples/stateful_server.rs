//! Example: server with shared state using handler_with_state.
//!
//! Run with:
//!   cargo run --example stateful_server --features server
//!
//! Demonstrates passing axum State to WebSocket handlers.

mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::Router;
use common::{ClientMsg, EchoEndpoint, ServerMsg};
use ws_bridge::WsEndpoint;

struct AppState {
    connection_count: AtomicU64,
}

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState {
        connection_count: AtomicU64::new(0),
    });

    let app = Router::new()
        .route(
            EchoEndpoint::PATH,
            ws_bridge::server::handler_with_state::<EchoEndpoint, _, _, Arc<AppState>>(
                |mut conn, state| async move {
                    let n = state.connection_count.fetch_add(1, Ordering::Relaxed) + 1;
                    println!("Client #{n} connected");

                    conn.send(ServerMsg::Welcome {
                        message: format!("You are client #{n}"),
                    })
                    .await
                    .unwrap();

                    while let Some(Ok(msg)) = conn.recv().await {
                        match msg {
                            ClientMsg::Say { text } => {
                                conn.send(ServerMsg::Echo { payload: text }).await.unwrap();
                            }
                            ClientMsg::Quit => break,
                        }
                    }

                    println!("Client #{n} disconnected");
                },
            ),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!(
        "Stateful server listening on ws://127.0.0.1:3000{}",
        EchoEndpoint::PATH
    );
    axum::serve(listener, app).await.unwrap();
}
