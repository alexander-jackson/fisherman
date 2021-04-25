use std::net::{Ipv4Addr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;

use actix_web::{http::HeaderValue, web, App, HttpRequest, HttpResponse, HttpServer};
use tokio::sync::{mpsc, Mutex};
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
struct State {
    pub config: Arc<Config>,
    pub sender: Arc<Mutex<mpsc::UnboundedSender<Webhook>>>,
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
    pub async fn handle(&self, config: &Arc<Config>) -> HttpResponse {
        match self {
            Webhook::Ping(p) => p.handle(config).await,
            Webhook::Push(p) => p.handle(config).await,
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

/// Receives messages from GitHub's API and deserializes them before handling.
///
/// Reads the content of the payload as a stream of bytes before checking which variant is expected
/// and deserializing the payload. It then verifies that the included hash is correct for the given
/// repository before handling the request.
async fn verify_incoming_webhooks(
    state: web::Data<State>,
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
        Err(_) => return HttpResponse::UnprocessableEntity().finish(),
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

    // Send the message to the other thread
    let guard = state.sender.lock().await;
    guard.send(webhook).unwrap();

    // Return a `Processing` status code
    HttpResponse::Processing().finish()
}

async fn process_webhooks(config: Arc<Config>, mut receiver: mpsc::UnboundedReceiver<Webhook>) {
    loop {
        // Read a webhook message from the channel
        let webhook = receiver.recv().await.unwrap();

        // Process its content
        webhook.handle(&config).await;
    }
}

#[actix_rt::main]
async fn main() -> actix_web::Result<()> {
    logging::setup_logger();

    // Read the configuration file
    let content = std::fs::read_to_string("fisherman.yml")?;
    let config = Arc::new(Config::from_str(&content).expect("Failed to parse config"));

    log::info!("Using the following config: {:#?}", config);

    config.check_for_potential_mistakes();

    // Setup the socket to run on
    let port = config.default.port.unwrap_or(5000);
    let socket = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);

    let (sender, receiver) = mpsc::unbounded_channel();
    let sender = Arc::new(Mutex::new(sender));

    let config_clone = Arc::clone(&config);

    tokio::spawn(async move {
        process_webhooks(config_clone, receiver).await;
    });

    let server = HttpServer::new(move || {
        let state = State {
            config: Arc::clone(&config),
            sender: Arc::clone(&sender),
        };

        App::new()
            .data(state)
            .route("/", web::post().to(verify_incoming_webhooks))
    })
    .bind(socket)?
    .run();

    server.await?;

    Ok(())
}
