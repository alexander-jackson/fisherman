use std::str::FromStr;
use std::sync::Arc;

use actix_web::{web, App, HttpResponse, HttpServer};

use crate::config::Config;
use crate::webhook::Webhook;

#[macro_use]
extern crate serde;

mod config;
mod git;
mod logging;
mod webhook;

/// Defines the state that each request can access.
#[derive(Clone, Debug)]
pub struct State {
    pub config: Arc<Config>,
}

async fn handle_webhook(state: web::Data<State>, webhook: web::Json<Webhook>) -> HttpResponse {
    log::debug!("Webhook body: {:?}", &webhook);

    if webhook.is_master_push() {
        log::info!("Commits were pushed to `master` in this event");

        // Pull the new changes
        webhook
            .trigger_pull(&state.config)
            .expect("Failed to execute the pull.");

        // Build the updated binary
        webhook
            .trigger_build(&state.config)
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

    // Read the configuration file
    let content = std::fs::read_to_string("fisherman.yml")?;
    let config = Arc::new(Config::from_str(&content).expect("Failed to parse config"));

    log::info!("Using the following config: {:#?}", config);

    let server = HttpServer::new(move || {
        let state = State {
            config: Arc::clone(&config),
        };

        App::new()
            .data(state)
            .route("/", web::post().to(handle_webhook))
    })
    .bind("127.0.0.1:5000")?
    .run();

    server.await?;

    Ok(())
}
