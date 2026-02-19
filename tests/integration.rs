//! Integration tests: spin up an axum server and connect with a native client.
//!
//! These tests require both `server` and `native-client` features.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use ws_bridge::{WsCodec, WsEndpoint, WsMessage};

// -- Test message types --

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ServerMsg {
    Hello { greeting: String },
    Echo { payload: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ClientMsg {
    Echo { payload: String },
    Goodbye,
}

struct TestEndpoint;

impl WsEndpoint for TestEndpoint {
    const PATH: &'static str = "/ws/test";
    type ServerMsg = ServerMsg;
    type ClientMsg = ClientMsg;
}

// -- Binary codec test types --

#[derive(Debug, Clone, PartialEq, Eq)]
struct BinaryFrame {
    id: u32,
    data: Vec<u8>,
}

impl WsCodec for BinaryFrame {
    fn encode(&self) -> Result<WsMessage, ws_bridge::EncodeError> {
        let mut buf = Vec::with_capacity(4 + self.data.len());
        buf.extend_from_slice(&self.id.to_le_bytes());
        buf.extend_from_slice(&self.data);
        Ok(WsMessage::Binary(buf))
    }

    fn decode(msg: WsMessage) -> Result<Self, ws_bridge::DecodeError> {
        match msg {
            WsMessage::Binary(data) if data.len() >= 4 => {
                let id = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                Ok(BinaryFrame {
                    id,
                    data: data[4..].to_vec(),
                })
            }
            WsMessage::Binary(_) => Err(ws_bridge::DecodeError::InvalidData(
                "frame too short".into(),
            )),
            WsMessage::Text(_) => Err(ws_bridge::DecodeError::UnexpectedText),
        }
    }
}

struct BinaryEndpoint;

impl WsEndpoint for BinaryEndpoint {
    const PATH: &'static str = "/ws/binary";
    type ServerMsg = BinaryFrame;
    type ClientMsg = BinaryFrame;
}

// -- Unidirectional endpoint using NoMessages --

struct PushOnlyEndpoint;

impl WsEndpoint for PushOnlyEndpoint {
    const PATH: &'static str = "/ws/push";
    type ServerMsg = ServerMsg;
    type ClientMsg = ws_bridge::NoMessages;
}

// -- Helper: start a test server --

async fn start_server(
    app: axum::Router,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle)
}

// -- Tests --

#[tokio::test]
async fn json_echo_round_trip() {
    // Server echoes back client messages wrapped in ServerMsg::Echo
    let app = axum::Router::new().route(
        TestEndpoint::PATH,
        ws_bridge::server::handler::<TestEndpoint, _, _>(|mut conn| async move {
            conn.send(ServerMsg::Hello {
                greeting: "welcome".into(),
            })
            .await
            .unwrap();

            while let Some(Ok(msg)) = conn.recv().await {
                match msg {
                    ClientMsg::Echo { payload } => {
                        conn.send(ServerMsg::Echo { payload }).await.unwrap();
                    }
                    ClientMsg::Goodbye => break,
                }
            }
        }),
    );

    let (addr, _server) = start_server(app).await;
    let base_url = format!("ws://{}", addr);

    let mut conn =
        ws_bridge::native_client::connect::<TestEndpoint>(&base_url)
            .await
            .unwrap();

    // Receive hello
    let msg = conn.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Hello {
            greeting: "welcome".into()
        }
    );

    // Send echo, receive echo
    conn.send(ClientMsg::Echo {
        payload: "test123".into(),
    })
    .await
    .unwrap();

    let msg = conn.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Echo {
            payload: "test123".into()
        }
    );

    // Send goodbye
    conn.send(ClientMsg::Goodbye).await.unwrap();
}

#[tokio::test]
async fn binary_codec_round_trip() {
    let app = axum::Router::new().route(
        BinaryEndpoint::PATH,
        ws_bridge::server::handler::<BinaryEndpoint, _, _>(|mut conn| async move {
            while let Some(Ok(frame)) = conn.recv().await {
                // Echo back with id incremented
                conn.send(BinaryFrame {
                    id: frame.id + 1,
                    data: frame.data,
                })
                .await
                .unwrap();
            }
        }),
    );

    let (addr, _server) = start_server(app).await;
    let base_url = format!("ws://{}", addr);

    let mut conn =
        ws_bridge::native_client::connect::<BinaryEndpoint>(&base_url)
            .await
            .unwrap();

    let payload = vec![0xDE, 0xAD, 0xBE, 0xEF];
    conn.send(BinaryFrame {
        id: 42,
        data: payload.clone(),
    })
    .await
    .unwrap();

    let frame = conn.recv().await.unwrap().unwrap();
    assert_eq!(frame.id, 43);
    assert_eq!(frame.data, payload);
}

#[tokio::test]
async fn split_connection() {
    let app = axum::Router::new().route(
        TestEndpoint::PATH,
        ws_bridge::server::handler::<TestEndpoint, _, _>(|conn| async move {
            let (mut tx, mut rx) = conn.split();

            tx.send(ServerMsg::Hello {
                greeting: "split".into(),
            })
            .await
            .unwrap();

            while let Some(Ok(msg)) = rx.recv().await {
                match msg {
                    ClientMsg::Echo { payload } => {
                        tx.send(ServerMsg::Echo { payload }).await.unwrap();
                    }
                    ClientMsg::Goodbye => break,
                }
            }
        }),
    );

    let (addr, _server) = start_server(app).await;
    let base_url = format!("ws://{}", addr);

    let conn = ws_bridge::native_client::connect::<TestEndpoint>(&base_url)
        .await
        .unwrap();

    let (mut tx, mut rx) = conn.split();

    let hello = rx.recv().await.unwrap().unwrap();
    assert_eq!(
        hello,
        ServerMsg::Hello {
            greeting: "split".into()
        }
    );

    tx.send(ClientMsg::Echo {
        payload: "via split".into(),
    })
    .await
    .unwrap();

    let echo = rx.recv().await.unwrap().unwrap();
    assert_eq!(
        echo,
        ServerMsg::Echo {
            payload: "via split".into()
        }
    );

    tx.send(ClientMsg::Goodbye).await.unwrap();
}

#[tokio::test]
async fn upgrade_fn_manual_handler() {
    // Test using upgrade() directly in a manual axum handler
    async fn my_handler(ws: axum::extract::ws::WebSocketUpgrade) -> axum::response::Response {
        ws_bridge::server::upgrade::<TestEndpoint, _, _>(ws, |mut conn| async move {
            conn.send(ServerMsg::Hello {
                greeting: "manual".into(),
            })
            .await
            .unwrap();
            while let Some(Ok(ClientMsg::Goodbye)) = conn.recv().await {
                break;
            }
        })
    }

    let app =
        axum::Router::new().route(TestEndpoint::PATH, axum::routing::get(my_handler));

    let (addr, _server) = start_server(app).await;
    let base_url = format!("ws://{}", addr);

    let mut conn =
        ws_bridge::native_client::connect::<TestEndpoint>(&base_url)
            .await
            .unwrap();

    let msg = conn.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Hello {
            greeting: "manual".into()
        }
    );
}

#[tokio::test]
async fn into_connection_fn() {
    // Test using into_connection() for max control
    let app = axum::Router::new().route(
        TestEndpoint::PATH,
        axum::routing::get(
            |ws: axum::extract::ws::WebSocketUpgrade| async move {
                ws.max_message_size(1024 * 1024)
                    .on_upgrade(|socket| async move {
                        let mut conn =
                            ws_bridge::server::into_connection::<TestEndpoint>(socket);
                        conn.send(ServerMsg::Hello {
                            greeting: "into_conn".into(),
                        })
                        .await
                        .unwrap();
                    })
            },
        ),
    );

    let (addr, _server) = start_server(app).await;
    let base_url = format!("ws://{}", addr);

    let mut conn =
        ws_bridge::native_client::connect::<TestEndpoint>(&base_url)
            .await
            .unwrap();

    let msg = conn.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Hello {
            greeting: "into_conn".into()
        }
    );
}

#[tokio::test]
async fn handler_with_state() {
    use std::sync::Arc;

    #[derive(Clone)]
    struct AppState {
        greeting: String,
    }

    let state = Arc::new(AppState {
        greeting: "stateful".into(),
    });

    let app = axum::Router::new()
        .route(
            TestEndpoint::PATH,
            ws_bridge::server::handler_with_state::<TestEndpoint, _, _, Arc<AppState>>(
                |mut conn, state| async move {
                    conn.send(ServerMsg::Hello {
                        greeting: state.greeting.clone(),
                    })
                    .await
                    .unwrap();

                    while let Some(Ok(msg)) = conn.recv().await {
                        match msg {
                            ClientMsg::Goodbye => break,
                            _ => {}
                        }
                    }
                },
            ),
        )
        .with_state(state);

    let (addr, _server) = start_server(app).await;
    let base_url = format!("ws://{}", addr);

    let mut conn =
        ws_bridge::native_client::connect::<TestEndpoint>(&base_url)
            .await
            .unwrap();

    let msg = conn.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Hello {
            greeting: "stateful".into()
        }
    );
}

#[tokio::test]
async fn connect_to_url_bypasses_path() {
    // Test native_client::connect_to_url with a custom path
    let app = axum::Router::new().route(
        "/custom/path",
        ws_bridge::server::handler::<TestEndpoint, _, _>(|mut conn| async move {
            conn.send(ServerMsg::Hello {
                greeting: "custom".into(),
            })
            .await
            .unwrap();
        }),
    );

    let (addr, _server) = start_server(app).await;
    let url = format!("ws://{}/custom/path", addr);

    let mut conn =
        ws_bridge::native_client::connect_to_url::<TestEndpoint>(&url)
            .await
            .unwrap();

    let msg = conn.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Hello {
            greeting: "custom".into()
        }
    );
}

#[tokio::test]
async fn push_only_endpoint_with_no_messages() {
    let app = axum::Router::new().route(
        PushOnlyEndpoint::PATH,
        ws_bridge::server::handler::<PushOnlyEndpoint, _, _>(|mut conn| async move {
            conn.send(ServerMsg::Hello {
                greeting: "push".into(),
            })
            .await
            .unwrap();
            conn.send(ServerMsg::Echo {
                payload: "data".into(),
            })
            .await
            .unwrap();
        }),
    );

    let (addr, _server) = start_server(app).await;
    let base_url = format!("ws://{}", addr);

    let mut conn =
        ws_bridge::native_client::connect::<PushOnlyEndpoint>(&base_url)
            .await
            .unwrap();

    let msg = conn.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Hello {
            greeting: "push".into()
        }
    );

    let msg = conn.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Echo {
            payload: "data".into()
        }
    );
}

#[tokio::test]
async fn reconnecting_ws_reconnects_on_drop() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use ws_bridge::reconnect::{BackoffConfig, ReconnectingWs};

    let connect_count = Arc::new(AtomicU32::new(0));
    let connect_count_server = connect_count.clone();

    let app = axum::Router::new().route(
        TestEndpoint::PATH,
        ws_bridge::server::handler::<TestEndpoint, _, _>(move |mut conn| {
            let count = connect_count_server.clone();
            async move {
                let n = count.fetch_add(1, Ordering::SeqCst);
                conn.send(ServerMsg::Hello {
                    greeting: format!("conn-{}", n),
                })
                .await
                .unwrap();

                // First connection: close immediately after hello
                if n == 0 {
                    return;
                }

                // Second connection: echo then close
                if let Some(Ok(ClientMsg::Echo { payload })) = conn.recv().await {
                    conn.send(ServerMsg::Echo { payload }).await.unwrap();
                }
            }
        }),
    );

    let (addr, _server) = start_server(app).await;
    let base_url = format!("ws://{}", addr);

    let config = BackoffConfig {
        initial: std::time::Duration::from_millis(50),
        max: std::time::Duration::from_millis(200),
        multiplier: 2.0,
    };

    let mut ws = ReconnectingWs::new(config, {
        let url = base_url.clone();
        move || {
            let url = url.clone();
            Box::pin(async move {
                ws_bridge::native_client::connect::<TestEndpoint>(&url)
                    .await
                    .ok()
            })
                as std::pin::Pin<
                    Box<
                        dyn std::future::Future<
                                Output = Option<
                                    ws_bridge::WsConnection<ClientMsg, ServerMsg>,
                                >,
                            > + Send,
                    >,
                >
        }
    });

    // First recv: get hello from first connection
    let msg = ws.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Hello {
            greeting: "conn-0".into()
        }
    );

    // Server closes first connection, reconnect should kick in.
    // Next recv should get hello from second connection.
    let msg = ws.recv().await.unwrap().unwrap();
    assert_eq!(
        msg,
        ServerMsg::Hello {
            greeting: "conn-1".into()
        }
    );

    // Verify we reconnected
    assert_eq!(connect_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn no_messages_decode_fails() {
    // Verify that NoMessages::decode always returns an error
    let msg = WsMessage::Text("anything".into());
    let result = ws_bridge::NoMessages::decode(msg);
    assert!(result.is_err());
}

#[tokio::test]
async fn multiple_clients_concurrent() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    let counter = Arc::new(AtomicU32::new(0));

    let app = axum::Router::new().route(
        TestEndpoint::PATH,
        ws_bridge::server::handler::<TestEndpoint, _, _>(move |mut conn| {
            let counter = counter.clone();
            async move {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                conn.send(ServerMsg::Hello {
                    greeting: format!("client-{}", n),
                })
                .await
                .unwrap();

                while let Some(Ok(msg)) = conn.recv().await {
                    match msg {
                        ClientMsg::Echo { payload } => {
                            conn.send(ServerMsg::Echo { payload }).await.unwrap();
                        }
                        ClientMsg::Goodbye => break,
                    }
                }
            }
        }),
    );

    let (addr, _server) = start_server(app).await;
    let base_url = format!("ws://{}", addr);

    // Connect two clients concurrently
    let (mut conn1, mut conn2) = tokio::join!(
        async {
            ws_bridge::native_client::connect::<TestEndpoint>(&base_url)
                .await
                .unwrap()
        },
        async {
            ws_bridge::native_client::connect::<TestEndpoint>(&base_url)
                .await
                .unwrap()
        },
    );

    // Both get hellos
    let _hello1 = conn1.recv().await.unwrap().unwrap();
    let _hello2 = conn2.recv().await.unwrap().unwrap();

    // Each echoes independently
    conn1
        .send(ClientMsg::Echo {
            payload: "from-1".into(),
        })
        .await
        .unwrap();
    conn2
        .send(ClientMsg::Echo {
            payload: "from-2".into(),
        })
        .await
        .unwrap();

    let echo1 = conn1.recv().await.unwrap().unwrap();
    let echo2 = conn2.recv().await.unwrap().unwrap();

    assert_eq!(
        echo1,
        ServerMsg::Echo {
            payload: "from-1".into()
        }
    );
    assert_eq!(
        echo2,
        ServerMsg::Echo {
            payload: "from-2".into()
        }
    );
}
