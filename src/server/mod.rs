//! Bot server and message handling infrastructure.
//!
//! Provides the [`BotServer`] for running a Threema bot with webhook handling,
//! and the [`MessageHandler`] trait for implementing custom bot logic.

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{Request, State},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::DateTime;
use moka::future::Cache as AsyncCache;
use serde_json::json;
use threema_gateway::{
    E2eApi, E2eMessage, IncomingMessage, RecipientKey,
    errors::ApiError as ThreemaApiError,
    protocol::{
        MessageId, ThreemaId,
        e2e::delivery_receipt::{DeliveryReceipt, DeliveryReceiptMessage},
    },
};

pub mod handler;

use crate::{
    client::ThreemaClient,
    commands::{self, CommandRegistry, ParsedCommand},
    config::BotConfig,
    dedup::{DeduplicateResult, MessageDeduplicator},
    errors::{InitError, RunError, SendError},
    rate_limit::{RateLimitConfig, RateLimitResult, RateLimiterManager},
    server::handler::{
        Action, ClassicReaction, CommandType, MessageContext, MessageHandler, Response,
        TypingHandle,
    },
    webhook::validate_timestamp,
};

/// Encrypt and send a text message.
async fn send_text(
    api: &E2eApi,
    to: &ThreemaId,
    text: &str,
    recipient_key: &RecipientKey,
) -> Result<MessageId, SendError> {
    let encrypted = api
        .encode_and_encrypt(&E2eMessage::Text(text.into()), recipient_key)
        .map_err(SendError::Encrypt)?;
    let message_id = api
        .send(to, &encrypted, false)
        .await
        .map_err(SendError::Send)?;
    tracing::info!("Sent message to {}: {} chars", to, text.len());
    Ok(message_id)
}

/// Encrypt and send a delivery receipt.
async fn send_receipt(
    api: &E2eApi,
    to: &ThreemaId,
    msg_id: MessageId,
    receipt: DeliveryReceipt,
    recipient_key: &RecipientKey,
) -> Result<MessageId, SendError> {
    let msg = E2eMessage::DeliveryReceipt(DeliveryReceiptMessage::new(receipt, msg_id));
    let encrypted = api
        .encode_and_encrypt(&msg, recipient_key)
        .map_err(SendError::Encrypt)?;
    api.send(to, &encrypted, false)
        .await
        .map_err(SendError::Send)
}

/// Shared application state.
struct AppState<H: MessageHandler> {
    config: BotConfig,
    threema_client: Arc<ThreemaClient>,
    handler: Arc<H>,
    commands: Arc<CommandRegistry>,
    message_dedup: MessageDeduplicator,
    rate_limiter: Option<RateLimiterManager>,
    rate_limit_notification_cache: AsyncCache<String, ()>,
}

impl<H: MessageHandler> AppState<H> {
    /// Handle priority commands that bypass rate limiting.
    ///
    /// Returns `true` if the command was handled, `false` if it should continue through the normal
    /// pipeline.
    async fn handle_priority_command(
        &self,
        command: &ParsedCommand<'_>,
        msg: &IncomingMessage,
        sender_public_key: &RecipientKey,
    ) -> bool {
        match command {
            ParsedCommand::Help => {
                let help_text = self.commands.help_text();
                let _ = send_text(
                    self.threema_client.api(),
                    &msg.from,
                    &help_text,
                    sender_public_key,
                )
                .await;
                true
            }
            ParsedCommand::Registered { .. }
            | ParsedCommand::Unknown { .. }
            | ParsedCommand::None(_) => false,
        }
    }

    /// Check rate limit for a sender, sending notification if rate limited.
    ///
    /// Returns true if the request should proceed, false if rate limited.
    async fn check_rate_limit(
        &self,
        sender_id: &ThreemaId,
        sender_public_key: &RecipientKey,
    ) -> bool {
        if let Some(limiter) = &self.rate_limiter
            && let RateLimitResult::Limited { message, .. } = limiter.check(*sender_id)
        {
            tracing::warn!("Rate limit exceeded for {}", sender_id);

            // Deduplicate notifications
            let notification_key = format!("rate_limit:{sender_id}");
            if self
                .rate_limit_notification_cache
                .get(&notification_key)
                .await
                .is_none()
            {
                self.rate_limit_notification_cache
                    .insert(notification_key, ())
                    .await;
                let api = self.threema_client.api().clone();
                let sender = *sender_id;
                let key = sender_public_key.clone();
                let msg = message.clone();
                tokio::spawn(async move {
                    let _ = send_text(&api, &sender, &msg, &key).await;
                });
            }

            return false;
        }
        true
    }
}

/// Bot server that handles webhooks and routes messages to your handler.
pub struct BotServer<H: MessageHandler> {
    config: BotConfig,
    threema_client: Arc<ThreemaClient>,
    handler: Arc<H>,
    commands: Arc<CommandRegistry>,
}

impl<H: MessageHandler> BotServer<H> {
    /// Create a new bot server.
    ///
    /// Validates the configuration and constructs the Threema Gateway client.
    ///
    /// Command configuration is read from the handler's
    /// [`description`](MessageHandler::description) and [`commands`](MessageHandler::commands)
    /// methods.
    pub fn new(config: BotConfig, handler: H) -> Result<Self, InitError> {
        // Validate config
        config.validate()?;

        // Construct ThreemaClient
        let threema_client = ThreemaClient::from_config(&config.threema)?;

        // Build command registry from handler
        let description = handler.description().map(String::from);
        let commands = CommandRegistry::new(description, H::commands());

        Ok(Self {
            config,
            threema_client: Arc::new(threema_client),
            handler: Arc::new(handler),
            commands: Arc::new(commands),
        })
    }

    /// Run the bot server.
    ///
    /// This starts the HTTP server and begins processing webhooks.
    /// The server runs until interrupted.
    pub async fn run(&self) -> Result<(), RunError> {
        let config = self.config.clone();

        // Validate API credentials
        self.threema_client
            .validate_api_secret()
            .await
            .map_err(RunError::CredentialValidation)?;

        // Create message deduplicator (5 minute TTL)
        let message_dedup = MessageDeduplicator::new(config.server.message_id_cache_size, 5 * 60);

        // Create rate limiter if enabled
        let rate_limiter = if config.rate_limiting.enabled {
            Some(RateLimiterManager::new(RateLimitConfig {
                messages_per_minute: config.rate_limiting.messages_per_minute,
                messages_per_hour: config.rate_limiting.messages_per_hour,
            }))
        } else {
            None
        };

        // Create rate limit notification cache (10 second TTL)
        let rate_limit_notification_cache = AsyncCache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(10))
            .build();

        // Create application state
        let app_state = Arc::new(AppState {
            config: config.clone(),
            threema_client: Arc::clone(&self.threema_client),
            handler: Arc::clone(&self.handler),
            commands: Arc::clone(&self.commands),
            message_dedup,
            rate_limiter,
            rate_limit_notification_cache,
        });

        // Build router
        let app = Router::new()
            .route(&config.server.webhook_path, post(webhook_handler::<H>))
            .route("/health", get(health_handler))
            .fallback(fallback_handler)
            .with_state(app_state);

        if config.threema.allowed_users.is_empty() {
            tracing::warn!(
                "No allowed_users configured – the bot will accept messages from anyone. \
                 Set threema.allowed_users to restrict access."
            );
        }

        // Start server
        let addr = format!("{}:{}", config.server.host, config.server.port);
        tracing::info!("Starting bot server on {}", addr);
        tracing::info!("Webhook endpoint: {}", config.server.webhook_path);

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(RunError::Bind)?;
        axum::serve(listener, app).await.map_err(RunError::Serve)?;

        Ok(())
    }
}

/// Fallback handler for unmatched routes.
async fn fallback_handler(request: Request) -> impl IntoResponse {
    tracing::debug!(
        "{} {} — no matching route",
        request.method(),
        request.uri().path()
    );
    axum::http::StatusCode::NOT_FOUND
}

/// Health check handler.
async fn health_handler() -> impl IntoResponse {
    let health = json!({
        "status": "healthy",
    });

    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&health).expect("health JSON serialization"),
    )
}

/// Webhook handler.
///
/// This handles incoming messages sent from Threema Gateway.
async fn webhook_handler<H: MessageHandler>(
    State(state): State<Arc<AppState<H>>>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Parse and validate MAC using threema-gateway library
    // This handles parameter extraction, hex decoding, and HMAC-SHA256 verification
    let msg = match IncomingMessage::from_urlencoded_bytes(&body, &state.config.threema.api_secret)
    {
        Ok(msg) => msg,
        Err(ThreemaApiError::InvalidMac) => {
            tracing::warn!("Invalid webhook signature");
            return (axum::http::StatusCode::UNAUTHORIZED, "Invalid signature");
        }
        Err(ThreemaApiError::ParseError(msg)) => {
            tracing::warn!("Failed to parse webhook: {}", msg);
            // Return 200 OK to prevent Threema Gateway delivery reattempts,
            // if parsing fails then retrying will not help.
            return (axum::http::StatusCode::OK, "Parse error");
        }
        Err(err) => {
            tracing::warn!("Webhook error: {}", err);
            return (axum::http::StatusCode::OK, "Error");
        }
    };

    // Verify timestamp
    if let Err(err) = validate_timestamp(msg.date, state.config.server.max_webhook_age_seconds) {
        tracing::warn!("Webhook verification failed from {}: {}", msg.from, err);
        // Return 200 OK to prevent Threema Gateway delivery reattempts,
        // if message is too old then retrying will not help.
        return (axum::http::StatusCode::OK, "Invalid timestamp");
    }

    // Check for duplicate message
    if state
        .message_dedup
        .check_and_insert(msg.from, msg.message_id)
        == DeduplicateResult::Duplicate
    {
        tracing::debug!("Duplicate message {} from {}", msg.message_id, msg.from);
        return (axum::http::StatusCode::OK, "Duplicate message");
    }

    // Fetch sender's public key
    let sender_public_key = match state.threema_client.lookup_pubkey(&msg.from).await {
        Ok(key) => key,
        Err(err) => {
            tracing::error!("Failed to fetch public key for {}: {}", msg.from, err);
            return (axum::http::StatusCode::OK, "Failed to fetch public key");
        }
    };

    // Decrypt message
    let e2e_message = match state
        .threema_client
        .api()
        .decrypt_and_decode_incoming_message(&msg, &sender_public_key)
    {
        Ok(plain) => plain,
        Err(err) => {
            tracing::error!("Incoming message error: {err}",);
            return (axum::http::StatusCode::OK, "Decryption failed");
        }
    };

    // Extract text or handle other message types
    let text = match e2e_message {
        E2eMessage::Text(text) => {
            tracing::debug!("Received text message from {}", msg.from);
            text
        }
        E2eMessage::DeliveryReceipt(delivery_receipt) => {
            tracing::debug!(
                "Received delivery receipt from {}: {:?}",
                msg.from,
                delivery_receipt
            );

            // Handle acknowledgements
            let classic_reaction: Result<ClassicReaction, String> =
                delivery_receipt.receipt.try_into();
            if let Ok(reaction) = classic_reaction
                && state.config.threema.allowed_users.contains(&msg.from)
            {
                // Note: The protocol allows reacting to multiple message IDs simultaneously. Since this is
                // rarely used and makes logic complex for the downstream bot implementor, we simplify this by
                // calling `handle_classic_reaction` for every message ID.

                // Create `MessageContext` for every message ID
                #[expect(clippy::cast_possible_wrap, reason = "timestamp fits in i64")]
                let created_at = DateTime::from_timestamp(msg.date as i64, 0)
                    .expect("message timestamp already validated");
                let contexts = delivery_receipt
                    .message_ids
                    .as_slice()
                    .iter()
                    .copied()
                    .map(|message_id| MessageContext {
                        message_id,
                        sender_identity: msg.from,
                        sender_nickname: msg.nickname.clone(),
                        created_at,
                    })
                    .collect::<Vec<_>>();

                let handler = Arc::clone(&state.handler);
                let sender_key = sender_public_key.clone();
                tokio::spawn(async move {
                    for context in contexts {
                        match handler.handle_classic_reaction(&context, reaction).await {
                            Ok(action) => {
                                if let Err(err) =
                                    handle_action(&state, &context, &msg.from, &sender_key, action)
                                        .await
                                {
                                    tracing::error!(
                                        "Failed to send classic reaction response: {}",
                                        err
                                    );
                                }
                            }
                            Err(err) => tracing::error!("Error handling classic reaction: {}", err),
                        }
                    }
                });
            }
            return (axum::http::StatusCode::OK, "OK");
        }
        E2eMessage::File(_file_message) => {
            tracing::debug!("Received file message from {}", msg.from);
            return (axum::http::StatusCode::OK, "OK");
        }
        E2eMessage::Location(_location_message) => {
            tracing::debug!("Received location message from {}", msg.from);
            return (axum::http::StatusCode::OK, "OK");
        }
        E2eMessage::TypingIndicator(typing_indicator_message) => {
            tracing::debug!(
                "Received typing indicator from {}: {:?}",
                msg.from,
                typing_indicator_message.status
            );
            return (axum::http::StatusCode::OK, "OK");
        }
        E2eMessage::Edit(edit_message) => {
            tracing::debug!(
                "Received edit message from {} for {}",
                msg.from,
                edit_message.message_id
            );
            return (axum::http::StatusCode::OK, "OK");
        }
        E2eMessage::Delete(delete_message) => {
            tracing::debug!(
                "Received delete message from {} for {}",
                msg.from,
                delete_message.message_id
            );
            return (axum::http::StatusCode::OK, "OK");
        }
        E2eMessage::Other { message_type, .. } => {
            tracing::debug!(
                "Received unknown message type from {}: 0x{:02x}",
                msg.from,
                u8::from(message_type)
            );
            return (axum::http::StatusCode::OK, "OK");
        }
    };

    // Check allowlist (empty list = allow anybody)
    if !state.config.threema.allowed_users.is_empty()
        && !state.config.threema.allowed_users.contains(&msg.from)
    {
        tracing::warn!("Access denied for {}", msg.from);
        let api = state.threema_client.api().clone();
        let from = msg.from;
        let key = sender_public_key.clone();
        tokio::spawn(async move {
            let _ = send_text(&api, &from, commands::messages::ACCESS_DENIED, &key).await;
        });
        return (axum::http::StatusCode::OK, "Unauthorized");
    }

    // Send delivery receipts
    let api = state.threema_client.api();
    let _ = send_receipt(
        api,
        &msg.from,
        msg.message_id,
        DeliveryReceipt::Received,
        &sender_public_key,
    )
    .await;
    let _ = send_receipt(
        api,
        &msg.from,
        msg.message_id,
        DeliveryReceipt::Read,
        &sender_public_key,
    )
    .await;

    // Parse command
    let command = state.commands.parse(&text);

    // Handle priority commands (bypass rate limit)
    if state
        .handle_priority_command(&command, &msg, &sender_public_key)
        .await
    {
        return (axum::http::StatusCode::OK, "OK");
    }

    // Check rate limits
    if !state.check_rate_limit(&msg.from, &sender_public_key).await {
        return (axum::http::StatusCode::TOO_MANY_REQUESTS, "Rate limited");
    }

    // Process message in background
    let state_clone = Arc::clone(&state);
    let ctx = MessageContext {
        message_id: msg.message_id,
        sender_identity: msg.from,
        sender_nickname: msg.nickname,
        #[expect(clippy::cast_possible_wrap, reason = "timestamp fits in i64")]
        created_at: DateTime::from_timestamp(msg.date as i64, 0)
            .expect("message timestamp already validated"),
    };
    tokio::spawn(async move {
        if let Err(err) = process_message(state_clone, ctx, sender_public_key, text).await {
            tracing::error!("Error processing message from {}: {}", msg.from, err);
        }
    });

    (axum::http::StatusCode::OK, "OK")
}

/// Send the `responses` to the `recipient`.
///
/// Note: If multiple responses are in the `responses` slice, they are sent one by one. If sending fails for
/// one of them, the following responses will not be processed.
async fn send_responses<H: MessageHandler>(
    state: &Arc<AppState<H>>,
    original_ctx: &MessageContext,
    recipient: &ThreemaId,
    recipient_key: &RecipientKey,
    responses: &[Response],
) -> Result<Vec<MessageId>, SendError> {
    let mut message_ids = vec![];
    for response in responses {
        // Send response
        let message_id = match response {
            Response::Text(text) => {
                send_text(state.threema_client.api(), recipient, text, recipient_key).await
            }
            Response::Image(image) => {
                state
                    .threema_client
                    .send_image_message(
                        recipient,
                        &image.data,
                        &image.media_type,
                        image.caption.as_deref(),
                    )
                    .await
            }
            Response::File(file) => {
                state
                    .threema_client
                    .send_file_message(
                        recipient,
                        &file.data,
                        &file.media_type,
                        file.file_name.as_deref(),
                        file.caption.as_deref(),
                    )
                    .await
            }
        }?;
        message_ids.push(message_id);

        // Notify handler of sent message ID (for reaction tracking)
        state
            .handler
            .on_response_sent(original_ctx, message_id)
            .await
            .map_err(SendError::Handler)?;
    }
    Ok(message_ids)
}

/// Process a text message.
async fn process_message<H: MessageHandler>(
    state: Arc<AppState<H>>,
    ctx: MessageContext,
    sender_public_key: RecipientKey,
    text: String,
) -> Result<(), SendError> {
    // Note: We already parsed the command in the request handler. However, we cannot move it into
    // this handle function without cloning the values due to lifetimes. Since parsing is very
    // simple and cheaper than doing multiple allocations, we simply re-parse the command.
    let command = state.commands.parse(&text);

    let typing_handle = TypingHandle::new(
        state.threema_client.api().clone(),
        ctx.sender_identity,
        sender_public_key.clone(),
    );

    let result = match command {
        ParsedCommand::Help => {
            // Already handled in webhook_handler
            return Ok(());
        }
        ParsedCommand::Registered { name, args } => {
            state
                .handler
                .handle_command(&ctx, name, args, CommandType::Registered, &typing_handle)
                .await
        }
        ParsedCommand::Unknown { name, args } => {
            if state.commands.handle_unknown() {
                state
                    .handler
                    .handle_command(&ctx, name, args, CommandType::Unknown, &typing_handle)
                    .await
            } else {
                Ok(Action::ShowHelp {
                    prelude: Some(format!(
                        "Unknown command: {}",
                        state.commands.format_command(name)
                    )),
                })
            }
        }
        ParsedCommand::None(text) => state.handler.handle_text(&ctx, text, &typing_handle).await,
    };

    typing_handle.stop().await;

    match result {
        Ok(action) => {
            handle_action(
                &state,
                &ctx,
                &ctx.sender_identity,
                &sender_public_key,
                action,
            )
            .await?;
        }
        Err(err) => {
            tracing::error!("Handler error for {}: {}", ctx.sender_identity, err);
            send_text(
                state.threema_client.api(),
                &ctx.sender_identity,
                commands::messages::GENERIC_ERROR,
                &sender_public_key,
            )
            .await?;
        }
    }

    Ok(())
}

/// Execute an [`Action`] returned by a handler method.
async fn handle_action<H: MessageHandler>(
    state: &Arc<AppState<H>>,
    ctx: &MessageContext,
    recipient: &ThreemaId,
    recipient_key: &RecipientKey,
    action: Action,
) -> Result<(), SendError> {
    match action {
        Action::Ignore => {}
        Action::ShowHelp { prelude } => {
            let text = state.commands.help_text_with_prelude(prelude.as_deref());
            send_text(state.threema_client.api(), recipient, &text, recipient_key).await?;
        }
        Action::Respond(responses) => {
            send_responses(state, ctx, recipient, recipient_key, &responses).await?;
        }
    }
    Ok(())
}
