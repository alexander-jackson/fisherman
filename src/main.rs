use actix_web::{web, App, HttpResponse, HttpServer};

use crate::webhook::Webhook;

#[macro_use]
extern crate serde;

mod git;
mod logging;
mod webhook;

async fn handle_webhook(webhook: web::Json<Webhook>) -> HttpResponse {
    log::debug!("Webhook body: {:?}", &webhook);

    if webhook.is_master_push() {
        log::info!("Commits were pushed to `master` in this event");

        // Pull the new changes
        webhook.trigger_pull().expect("Failed to execute the pull.");

        // Build the updated binary
        webhook
            .trigger_build()
            .expect("Failed to rebuild the binary");

        // Restart in `supervisor`
        webhook
            .trigger_restart()
            .expect("Failed to restart the process");
    }

    HttpResponse::Ok().finish()
}

#[actix_rt::main]
async fn main() -> actix_web::Result<()> {
    logging::setup_logger();

    let server = HttpServer::new(move || App::new().route("/", web::post().to(handle_webhook)))
        .bind("127.0.0.1:5000")?
        .run();

    server.await?;

    Ok(())
}
