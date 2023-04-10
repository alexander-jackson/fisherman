#![allow(clippy::module_name_repetitions)]

use std::convert::TryFrom;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;

use actix_web::http::header::HeaderValue;
use actix_web::middleware::Logger;
use actix_web::web::{self, Data};
use actix_web::{App, HttpRequest, HttpResponse, HttpServer};
use tokio::sync::{mpsc, Mutex};
use tokio_stream::StreamExt;

use crate::config::Config;
use crate::error::ServerError;

#[macro_use]
extern crate serde;

mod auth;
mod config;
mod error;
mod git;
mod logging;
mod webhook;

/// Defines the state that each request can access.
#[derive(Clone, Debug)]
struct State {
    pub config: Arc<Config>,
    pub sender: Arc<Mutex<mpsc::UnboundedSender<Webhook>>>,
}

#[derive(Copy, Clone, Debug)]
enum WebhookVariant {
    Push,
    Ping,
}

impl TryFrom<&HttpRequest> for WebhookVariant {
    type Error = ServerError;

    fn try_from(request: &HttpRequest) -> Result<Self, Self::Error> {
        // Decide the variant to parse based on the headers
        let header = match request
            .headers()
            .get("X-GitHub-Event")
            .and_then(|v| v.to_str().ok())
        {
            Some(variant) => variant,
            None => return Err(ServerError::BadRequest),
        };

        tracing::debug!(%header, "Received an X-GitHub Event header");

        match header {
            "push" => Ok(Self::Push),
            "ping" => Ok(Self::Ping),
            _ => Err(ServerError::BadRequest),
        }
    }
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
    pub fn from_slice(variant: WebhookVariant, bytes: &[u8]) -> serde_json::Result<Self> {
        let webhook = match variant {
            WebhookVariant::Push => Self::Push(serde_json::from_slice(bytes)?),
            WebhookVariant::Ping => Self::Ping(serde_json::from_slice(bytes)?),
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
) -> Result<HttpResponse, ServerError> {
    let mut bytes = web::BytesMut::new();

    while let Some(Ok(item)) = payload.next().await {
        bytes.extend_from_slice(&item);
    }

    let variant = WebhookVariant::try_from(&request)?;

    let webhook =
        Webhook::from_slice(variant, &bytes).map_err(|_| ServerError::UnprocessableEntity)?;

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

    auth::validate_webhook_body(&bytes, secret, expected)?;

    tracing::debug!(?webhook, "Verified");

    // Send the message to the other thread
    let guard = state.sender.lock().await;
    guard.send(webhook).unwrap();

    // Return an `Accepted` status code
    Ok(HttpResponse::Accepted().finish())
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

    config.check_for_potential_mistakes();

    // Setup the socket to run on
    let port = config.default.port.unwrap_or(5000);
    let socket = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);

    tracing::info!(%port, ?config, "Listening for incoming webhooks");

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
            .wrap(Logger::new("%s @ %r"))
            .app_data(Data::new(state))
            .route("/", web::post().to(verify_incoming_webhooks))
    })
    .bind(socket)?
    .run();

    server.await?;

    Ok(())
}
