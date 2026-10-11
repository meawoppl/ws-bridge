mod common;
#[macro_use]
#[path = "common/echo_handler.rs"]
mod echo_handler;
use ws_bridge::{actix_server, WsEndpoint};

async fn handle(
    req: actix_web::HttpRequest,
    body: actix_web::web::Payload,
) -> actix_web::Result<actix_web::HttpResponse> {
    actix_server::upgrade::<common::EchoEndpoint, _, _>(
        &req,
        body,
        echo_handler!(actix_server::Connection<common::EchoEndpoint>),
    )
}
fn main() -> std::io::Result<()> {
    actix_web::rt::System::new().block_on(async {
        actix_web::HttpServer::new(|| {
            actix_web::App::new()
                .route(common::EchoEndpoint::PATH, actix_web::web::get().to(handle))
        })
        .bind("127.0.0.1:3000")?
        .run()
        .await
    })
}
