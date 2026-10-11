mod common;
#[macro_use]
#[path = "common/echo_handler.rs"]
mod echo_handler;
use salvo::prelude::*;
use ws_bridge::{salvo_server, WsEndpoint};

#[handler]
async fn handle(req: &mut Request, res: &mut Response) -> Result<(), salvo::http::StatusError> {
    salvo_server::upgrade::<common::EchoEndpoint, _, _>(
        salvo::websocket::WebSocketUpgrade::new(),
        req,
        res,
        echo_handler!(),
    )
    .await
}
#[tokio::main]
async fn main() {
    let app = Router::with_path(common::EchoEndpoint::PATH).get(handle);
    let acceptor = TcpListener::new("127.0.0.1:3000").bind().await;
    Server::new(acceptor).serve(app).await;
}
