use warp::Filter;

#[macro_use]
extern crate serde;

pub mod git;
mod logging;
mod webhook;

use crate::webhook::Webhook;

#[tokio::main]
async fn main() {
    logging::setup_logger();

    let webhook = warp::post().and(warp::body::json()).map(|body: Webhook| {
        log::debug!("Webhook body: {:?}", &body);

        if body.is_master_push() {
            log::info!("Commits were pushed to `master` in this event");
            body.trigger_pull().expect("Failed to execute the pull.");
            body.trigger_build().expect("Failed to rebuild the binary");
            body.trigger_restart()
                .expect("Failed to restart the process");
        }

        "Thanks for the update"
    });

    warp::serve(webhook).run(([127, 0, 0, 1], 5000)).await;
}
