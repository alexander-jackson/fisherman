use std::fmt;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use actix_web::{http::HeaderValue, web, App, HttpRequest, HttpResponse, HttpServer};
use chrono::Utc;
use tokio_stream::StreamExt;

use crate::config::Config;

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

#[derive(Debug)]
pub enum Event {
    Ping,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Ping => write!(f, "ping"),
        }
    }
}

#[derive(Debug)]
pub struct TimestampedEvent {
    timestamp: i64,
    event: Event,
}

impl fmt::Display for TimestampedEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.timestamp, self.event)
    }
}

impl TimestampedEvent {
    pub fn new(event: Event) -> Self {
        // Get the current timestamp
        let timestamp = Utc::now().timestamp();

        Self { timestamp, event }
    }
}

pub type TimeseriesQueue = RwLock<Vec<TimestampedEvent>>;

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
    pub fn handle(&self, config: &Arc<Config>, events: &TimeseriesQueue) -> HttpResponse {
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

async fn status(shared: web::Data<TimeseriesQueue>) -> HttpResponse {
    // Get a read lock to the queue
    let queue = shared.read().unwrap();

    let queue_state = queue
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    HttpResponse::Ok().body(queue_state)
}

/// Receives messages from GitHub's API and deserializes them before handling.
///
/// Reads the content of the payload as a stream of bytes before checking which variant is expected
/// and deserializing the payload. It then verifies that the included hash is correct for the given
/// repository before handling the request.
async fn handle_webhook(
    state: web::Data<State>,
    shared: web::Data<TimeseriesQueue>,
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
    let shared = web::Data::new(TimeseriesQueue::default());

    let server = HttpServer::new(move || {
        let state = State {
            config: Arc::clone(&config),
        };

        App::new()
            .data(state)
            .app_data(shared.clone())
            .route("/", web::post().to(handle_webhook))
            .route("/status", web::get().to(status))
    })
    .bind(socket)?
    .run();

    server.await?;

    Ok(())
}
