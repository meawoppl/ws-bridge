mod common;
#[macro_use]
#[path = "common/echo_handler.rs"]
mod echo_handler;
use warp::Filter;
use ws_bridge::{warp_server, WsEndpoint};

#[tokio::main]
async fn main() {
    let route = warp::path::full().and(warp::ws()).and_then(
        |path: warp::path::FullPath, ws: warp::ws::Ws| async move {
            if path.as_str() != common::EchoEndpoint::PATH {
                return Err(warp::reject::not_found());
            }
            Ok(warp_server::upgrade::<common::EchoEndpoint, _, _>(
                ws,
                echo_handler!(),
            ))
        },
    );
    warp::serve(route).run(([127, 0, 0, 1], 3000)).await;
}
