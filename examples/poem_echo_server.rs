mod common;
#[macro_use]
#[path = "common/echo_handler.rs"]
mod echo_handler;
use ws_bridge::{poem_server, WsEndpoint};

#[poem::handler]
async fn handle(ws: poem::web::websocket::WebSocket) -> impl poem::IntoResponse {
    poem_server::upgrade::<common::EchoEndpoint, _, _>(ws, echo_handler!())
}
#[tokio::main]
async fn main() -> std::io::Result<()> {
    let app = poem::Route::new().at(common::EchoEndpoint::PATH, poem::get(handle));
    poem::Server::new(poem::listener::TcpListener::bind("127.0.0.1:3000"))
        .run(app)
        .await
}
