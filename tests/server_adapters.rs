//! Live interoperability tests shared by all optional server adapters.
use futures_util::{SinkExt, StreamExt};
use std::{net::SocketAddr, time::Duration};
use tokio_tungstenite::tungstenite::Message;
use ws_bridge::{DecodeError, EncodeError, WsCodec, WsEndpoint, WsMessage};

struct Echo;
impl WsEndpoint for Echo {
    const PATH: &'static str = "/ws/echo";
    type ServerMsg = Packet;
    type ClientMsg = Packet;
}
#[derive(Clone, Debug, PartialEq)]
enum Packet {
    Text(String),
    Binary(Vec<u8>),
}
impl WsCodec for Packet {
    fn encode(&self) -> Result<WsMessage, EncodeError> {
        Ok(match self {
            Self::Text(t) => WsMessage::Text(serde_json::to_string(t)?),
            Self::Binary(b) => WsMessage::Binary(b.clone()),
        })
    }
    fn decode(msg: WsMessage) -> Result<Self, DecodeError> {
        Ok(match msg {
            WsMessage::Text(t) => Self::Text(serde_json::from_str(&t)?),
            WsMessage::Binary(b) => Self::Binary(b),
        })
    }
}
macro_rules! echo {
    () => { echo!(ws_bridge::WsConnection<Packet, Packet>) };
    ($conn_type:ty) => {
        |conn: $conn_type| async move {
            let (mut tx, mut rx) = conn.split();
            while let Some(result) = rx.recv().await {
                match result {
                    Ok(packet) => {
                        if tx.send(packet).await.is_err() {
                            break;
                        }
                    }
                    Err(ws_bridge::RecvError::Decode(_)) => continue,
                    Err(ws_bridge::RecvError::Closed) => break,
                }
            }
            let _ = tx.close().await;
        }
    };
}

async fn listener() -> (tokio::net::TcpListener, SocketAddr) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    (listener, addr)
}
async fn exercise(addr: SocketAddr) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let base = format!("ws://{addr}");
        let conn = ws_bridge::native_client::connect::<Echo>(&base)
            .await
            .unwrap();
        let (mut tx, mut rx) = conn.split();
        for packet in [
            Packet::Text("hello 🦀".into()),
            Packet::Binary(vec![0, 128, 255]),
        ] {
            tx.send(packet.clone()).await.unwrap();
            assert_eq!(rx.recv().await.unwrap().unwrap(), packet);
        }
        tx.close().await.unwrap();
        assert!(rx.recv().await.is_none());

        let (mut raw, _) = tokio_tungstenite::connect_async(format!("{base}{}", Echo::PATH))
            .await
            .unwrap();
        raw.send(Message::Text("invalid JSON".into()))
            .await
            .unwrap();
        raw.send(Message::Ping(vec![1, 2, 3].into())).await.unwrap();
        raw.send(Message::Text("\"after malformed frame\"".into()))
            .await
            .unwrap();
        let mut pong = false;
        let mut echoed = false;
        while !pong || !echoed {
            match raw.next().await.unwrap().unwrap() {
                Message::Pong(bytes) => {
                    assert_eq!(&bytes[..], &[1, 2, 3]);
                    pong = true;
                }
                Message::Text(text) => {
                    assert_eq!(text.as_str(), "\"after malformed frame\"");
                    echoed = true;
                }
                other => panic!("unexpected frame: {other:?}"),
            }
        }
        // A logical message must be decoded once, after its continuations are
        // aggregated. This also verifies Actix's local stream configuration.
        use tokio_tungstenite::tungstenite::protocol::frame::{
            coding::{Data, OpCode},
            Frame,
        };
        raw.send(Message::Frame(Frame::message(
            b"\"frag".to_vec(),
            OpCode::Data(Data::Text),
            false,
        )))
        .await
        .unwrap();
        raw.send(Message::Frame(Frame::message(
            b"mented\"".to_vec(),
            OpCode::Data(Data::Continue),
            true,
        )))
        .await
        .unwrap();
        assert_eq!(
            raw.next().await.unwrap().unwrap().into_text().unwrap(),
            "\"fragmented\""
        );
        raw.close(None).await.unwrap();
        assert!(matches!(
            raw.next().await,
            Some(Ok(Message::Close(_))) | None
        ));
    })
    .await
    .expect("adapter interoperability timed out");
}

#[cfg(feature = "warp-server")]
#[tokio::test]
async fn warp_interoperability() {
    use warp::Filter;
    let route = warp::path("ws")
        .and(warp::path("echo"))
        .and(warp::path::end())
        .and(warp::ws())
        .map(|ws| ws_bridge::warp_server::upgrade::<Echo, _, _>(ws, echo!()));
    let (listener, addr) = listener().await;
    let server = tokio::spawn(warp::serve(route).incoming(listener).run());
    exercise(addr).await;
    server.abort();
}

#[cfg(feature = "poem-server")]
#[tokio::test]
async fn poem_interoperability() {
    #[poem::handler]
    async fn handle(ws: poem::web::websocket::WebSocket) -> impl poem::IntoResponse {
        ws_bridge::poem_server::upgrade::<Echo, _, _>(ws, echo!())
    }
    let route = poem::Route::new().at(Echo::PATH, poem::get(handle));
    let (listener, addr) = listener().await;
    let acceptor = poem::listener::TcpAcceptor::from_tokio(listener).unwrap();
    let server = tokio::spawn(poem::Server::new_with_acceptor(acceptor).run(route));
    exercise(addr).await;
    server.abort();
}

#[cfg(feature = "salvo-server")]
#[tokio::test]
async fn salvo_interoperability() {
    #[salvo::handler]
    async fn handle(
        req: &mut salvo::Request,
        res: &mut salvo::Response,
    ) -> Result<(), salvo::http::StatusError> {
        ws_bridge::salvo_server::upgrade::<Echo, _, _>(
            salvo::websocket::WebSocketUpgrade::new(),
            req,
            res,
            echo!(),
        )
        .await
    }
    let route = salvo::Router::with_path("ws/echo").get(handle);
    let (listener, addr) = listener().await;
    let acceptor = salvo::conn::tcp::TcpAcceptor::try_from(listener).unwrap();
    let server = tokio::spawn(salvo::Server::new(acceptor).serve(route));
    exercise(addr).await;
    server.abort();
}

#[cfg(feature = "hyper-server")]
#[tokio::test]
async fn hyper_interoperability() {
    async fn handle(
        mut request: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<hyper::body::Bytes>>, std::convert::Infallible>
    {
        assert_eq!(request.uri().path(), Echo::PATH);
        assert!(ws_bridge::hyper_server::is_upgrade_request(&request));
        let (response, socket) = ws_bridge::hyper_server::upgrade(&mut request, None).unwrap();
        tokio::spawn(async move {
            let conn = ws_bridge::hyper_server::into_connection::<Echo>(socket.await.unwrap());
            echo!()(conn).await;
        });
        Ok(response)
    }
    let (listener, addr) = listener().await;
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        hyper_util::rt::TokioIo::new(stream),
                        hyper::service::service_fn(handle),
                    )
                    .with_upgrades()
                    .await
                    .unwrap();
            });
        }
    });
    exercise(addr).await;
    server.abort();
}

#[cfg(feature = "actix-server")]
#[tokio::test]
async fn actix_interoperability() {
    async fn handle(
        req: actix_web::HttpRequest,
        body: actix_web::web::Payload,
    ) -> actix_web::Result<actix_web::HttpResponse> {
        ws_bridge::actix_server::upgrade::<Echo, _, _>(
            &req,
            body,
            echo!(ws_bridge::actix_server::Connection<Echo>),
        )
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        actix_web::rt::System::new().block_on(async move {
            let server = actix_web::HttpServer::new(|| {
                actix_web::App::new().route(Echo::PATH, actix_web::web::get().to(handle))
            })
            .workers(1)
            .disable_signals()
            .listen(listener)
            .unwrap()
            .run();
            ready_tx.send(server.handle()).unwrap();
            server.await.unwrap();
        });
    });
    let handle = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .unwrap()
        .unwrap();
    exercise(addr).await;
    handle.stop(true).await;
    thread.join().unwrap();
}

#[cfg(feature = "rocket-server")]
#[rocket::get("/ws/echo")]
fn rocket_handle(ws: rocket_ws::WebSocket) -> rocket_ws::Channel<'static> {
    ws_bridge::rocket_server::upgrade::<Echo, _, _>(ws, echo!())
}

#[cfg(feature = "rocket-server")]
#[tokio::test]
async fn rocket_interoperability() {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let rocket = rocket::custom(rocket::Config {
        port: 0,
        address: "127.0.0.1".parse().unwrap(),
        log_level: rocket::config::LogLevel::Off,
        ..Default::default()
    })
    .mount("/", rocket::routes![rocket_handle])
    .attach(rocket::fairing::AdHoc::on_liftoff("Ready", move |rocket| {
        Box::pin(async move {
            ready_tx
                .send((
                    SocketAddr::new(rocket.config().address, rocket.config().port),
                    rocket.shutdown(),
                ))
                .unwrap();
        })
    }));
    let server = tokio::spawn(rocket.launch());
    let (addr, shutdown) = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .unwrap()
        .unwrap();
    exercise(addr).await;
    shutdown.notify();
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
