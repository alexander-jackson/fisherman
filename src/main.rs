use std::net::{Ipv4Addr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;

use actix_web::{http::HeaderValue, web, App, HttpRequest, HttpResponse, HttpServer};
use tera::{Context, Tera};
use tokio_stream::StreamExt;

use crate::config::Config;

#[macro_use]
extern crate serde;

mod auth;
mod config;
mod events;
mod git;
mod logging;
mod webhook;

/// Defines the state that each request can access.
#[derive(Clone, Debug)]
pub struct State {
    pub config: Arc<Config>,
}

#[derive(Debug)]
enum Webhook {
    Push(webhook::Push),
    Ping(webhook::Ping),
}

impl Webhook {
    /// Gets the full name of the repository this hook refers to.
    pub fn get_full_name(&self) -> &str {
        match self {
            Webhook::Ping(p) => p.get_full_name(),
            Webhook::Push(p) => p.get_full_name(),
        }
    }

    /// Handles the payload of the request depending on its type.
    pub fn handle(&self, config: &Arc<Config>, events: &events::TimeseriesQueue) -> HttpResponse {
        match self {
            Webhook::Ping(p) => p.handle(config, events),
            Webhook::Push(p) => p.handle(config, events),
        }
    }

    /// Deserializes JSON from bytes depending on which variant is expected.
    pub fn from_slice(variant: &str, bytes: &[u8]) -> serde_json::Result<Self> {
        let webhook = match variant {
            "push" => Self::Push(serde_json::from_slice(bytes)?),
            "ping" => Self::Ping(serde_json::from_slice(bytes)?),
            _ => unreachable!(),
        };

        Ok(webhook)
    }
}

async fn status(tera: web::Data<Tera>, shared: web::Data<events::TimeseriesQueue>) -> HttpResponse {
    // Get a read lock to the queue
    let queue = shared.read();

    // Render the template given the queue
    let mut context = Context::new();
    context.insert("queue", &*queue);

    let content = tera.render("status.html.tera", &context).unwrap();

    HttpResponse::Ok().content_type("text/html").body(content)
}

/// Receives messages from GitHub's API and deserializes them before handling.
///
/// Reads the content of the payload as a stream of bytes before checking which variant is expected
/// and deserializing the payload. It then verifies that the included hash is correct for the given
/// repository before handling the request.
async fn handle_webhook(
    state: web::Data<State>,
    shared: web::Data<events::TimeseriesQueue>,
    mut payload: web::Payload,
    request: HttpRequest,
) -> HttpResponse {
    let mut bytes = web::BytesMut::new();

    while let Some(Ok(item)) = payload.next().await {
        bytes.extend_from_slice(&item);
    }

    // Decide the variant to parse based on the headers
    let variant = match request
        .headers()
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
    {
        Some(variant) => variant,
        None => return HttpResponse::BadRequest().finish(),
    };

    let webhook = match Webhook::from_slice(&variant, &bytes) {
        Ok(webhook) => webhook,
        Err(e) => {
            log::warn!("Error deserializing: {}", e);
            return HttpResponse::UnprocessableEntity().finish();
        }
    };

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

    webhook.handle(&state.config, &shared)
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

    // Create some shared state
    let shared = web::Data::new(events::TimeseriesQueue::default());

    let server = HttpServer::new(move || {
        let state = State {
            config: Arc::clone(&config),
        };

        // Initialise the templating engine
        let tera = Tera::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/**/*")).unwrap();

        App::new()
            .data(state)
            .data(tera)
            .app_data(shared.clone())
            .route("/", web::post().to(handle_webhook))
            .route("/status", web::get().to(status))
    })
    .bind(socket)?
    .run();

    server.await?;

    Ok(())
}
