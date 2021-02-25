use std::net::{Ipv4Addr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;

use actix_web::{http::HeaderValue, web, App, HttpRequest, HttpResponse, HttpServer};
use tokio_stream::StreamExt;

use crate::config::Config;
use crate::webhook::Webhook;

#[macro_use]
extern crate serde;

mod auth;
mod config;
mod git;
mod logging;
mod webhook;

/// Defines the state that each request can access.
#[derive(Clone, Debug)]
pub struct State {
    pub config: Arc<Config>,
}

async fn handle_webhook(
    state: web::Data<State>,
    mut payload: web::Payload,
    request: HttpRequest,
) -> HttpResponse {
    let mut bytes = web::BytesMut::new();

    while let Some(Ok(item)) = payload.next().await {
        bytes.extend_from_slice(&item);
    }

    let webhook: Webhook = serde_json::from_slice(&bytes).unwrap();

    // Validate the payload with the secret key
    let secret = state
        .config
        .resolve_secret(webhook.get_full_name())
        .map(str::as_bytes);

    // Get the expected value as bytes
    let expected = request
        .headers()
        .get("X-Hub-Signature-256")
        .map(HeaderValue::to_str)
        .and_then(Result::ok)
        .map(str::as_bytes)
        .map(|s| s.split_at(7).1);

    if let Err(e) = auth::validate_webhook_body(&bytes, secret, expected) {
        log::error!("Payload failed to validate with secret");
        return e;
    }

    log::debug!("Webhook verified: {:?}", &webhook);

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
            .trigger_restart(&state.config)
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

    // Setup the socket to run on
    let port = config.default.port.unwrap_or(5000);
    let socket = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);

    let server = HttpServer::new(move || {
        let state = State {
            config: Arc::clone(&config),
        };

        App::new()
            .data(state)
            .route("/", web::post().to(handle_webhook))
    })
    .bind(socket)?
    .run();

    server.await?;

    Ok(())
}
