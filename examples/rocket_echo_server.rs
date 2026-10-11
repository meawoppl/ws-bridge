mod common;
#[macro_use]
#[path = "common/echo_handler.rs"]
mod echo_handler;
use ws_bridge::{rocket_server, WsEndpoint};

// Rocket's routing macro requires a literal path; assert it matches the shared endpoint.
#[rocket::get("/ws/echo")]
fn handle(ws: rocket_ws::WebSocket) -> rocket_ws::Channel<'static> {
    rocket_server::upgrade::<common::EchoEndpoint, _, _>(ws, echo_handler!())
}
#[tokio::main]
async fn main() -> Result<(), Box<rocket::Error>> {
    let route = rocket::routes![handle].remove(0);
    assert_eq!(route.uri.path(), common::EchoEndpoint::PATH);
    rocket::custom(rocket::Config {
        port: 3000,
        ..Default::default()
    })
    .mount("/", vec![route])
    .launch()
    .await?;
    Ok(())
}
