mod common;
#[macro_use]
#[path = "common/echo_handler.rs"]
mod echo_handler;
use http_body_util::Full;
use hyper::{
    body::{Bytes, Incoming},
    Request, Response,
};
use ws_bridge::{hyper_server, WsEndpoint};

async fn handle(
    mut request: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    if request.uri().path() != common::EchoEndpoint::PATH {
        return Ok(Response::builder()
            .status(404)
            .body(Full::new(Bytes::new()))
            .unwrap());
    }
    if !hyper_server::is_upgrade_request(&request) {
        return Ok(Response::builder()
            .status(400)
            .body(Full::new(Bytes::new()))
            .unwrap());
    }
    let (response, socket) = match hyper_server::upgrade(&mut request, None) {
        Ok(upgrade) => upgrade,
        Err(_) => {
            return Ok(Response::builder()
                .status(400)
                .body(Full::new(Bytes::new()))
                .unwrap())
        }
    };
    tokio::spawn(async move {
        match socket.await {
            Ok(socket) => {
                let callback = echo_handler!();
                // The annotation also documents the server/client direction.
                let conn: hyper_server::Connection<common::EchoEndpoint> =
                    hyper_server::into_connection::<common::EchoEndpoint>(socket);
                callback(conn).await;
            }
            Err(error) => eprintln!("upgrade failed: {error}"),
        }
    });
    Ok(response)
}
#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(error) = hyper::server::conn::http1::Builder::new()
                .serve_connection(
                    hyper_util::rt::TokioIo::new(stream),
                    hyper::service::service_fn(handle),
                )
                .with_upgrades()
                .await
            {
                eprintln!("connection failed: {error}");
            }
        });
    }
}
