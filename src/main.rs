use warp::Filter;

#[macro_use]
extern crate serde;

mod webhook;

use webhook::Webhook;

#[tokio::main]
async fn main() {
    let webhook = warp::post().and(warp::body::json()).map(|body: Webhook| {
        dbg!(&body);
        "Thanks for the update"
    });

    warp::serve(webhook).run(([127, 0, 0, 1], 5000)).await;
}