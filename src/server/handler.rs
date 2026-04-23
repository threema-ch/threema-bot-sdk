//! Message handler logic.

use std::sync::Mutex;

use chrono::{DateTime, Utc};
use threema_gateway::{
    E2eApi, E2eMessage, RecipientKey,
    protocol::{
        MessageId, ThreemaId,
        e2e::{
            delivery_receipt::DeliveryReceipt,
            typing_indicator::{TypingIndicatorMessage, TypingStatus},
        },
    },
};
use tokio::{task::JoinHandle, time::Duration};

use crate::commands::Commands;

/// Error type for [`MessageHandler`] methods.
///
/// This is a type-erased error: Handler implementations can return any error type that implements
/// [`std::error::Error`].
///
/// The simplest way to do this is by converting a string:
///
/// ```rust
/// use threema_gateway_bot::server::handler::HandlerError;
///
/// let err: HandlerError = "something went wrong".into();
/// let err: HandlerError = format!("bad value: {}", 42).into();
/// ```
pub type HandlerError = Box<dyn std::error::Error + Send + Sync>;

/// Result type for [`MessageHandler`] methods.
pub type HandlerResult<T> = Result<T, HandlerError>;

/// Indicates how a command was classified by the command registry.
///
/// Passed to [`MessageHandler::handle_command`] so the handler can distinguish
/// between commands it explicitly registered and commands that were not recognized.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CommandType {
    /// A command registered via [`Commands::register`](crate::commands::Commands::register).
    Registered,
    /// A command that was not registered.
    ///
    /// Only dispatched to the handler if
    /// [`Commands::handle_unknown`](crate::commands::Commands::handle_unknown) was called.
    Unknown,
}

/// Handle for showing a typing indicator during message processing.
///
/// Call [`send`](Self::send) to start showing a typing indicator to the user.
/// The indicator is automatically re-sent every 10 seconds to keep it alive.
/// When processing completes (success or failure), the server automatically
/// stops the indicator.
pub struct TypingHandle {
    api: E2eApi,
    to: ThreemaId,
    recipient_key: RecipientKey,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl TypingHandle {
    /// Create a new typing handle.
    pub(crate) fn new(api: E2eApi, to: ThreemaId, recipient_key: RecipientKey) -> Self {
        Self {
            api,
            to,
            recipient_key,
            task: Mutex::new(None),
        }
    }

    /// Send a typing indicator to the user.
    ///
    /// The indicator is sent immediately and then re-sent every 10 seconds to
    /// keep it alive. When processing completes (success or failure), the server
    /// automatically resets the indicator.
    ///
    /// This is a best-effort operation: failures are logged but do not interrupt
    /// message processing. Calling this multiple times is a no-op after the
    /// first call.
    pub fn send(&self) {
        let mut guard = self.task.lock().expect("typing handle lock poisoned");
        if guard.is_some() {
            return;
        }

        let api = self.api.clone();
        let to = self.to;
        let recipient_key = self.recipient_key.clone();

        *guard = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                let msg = TypingIndicatorMessage::new(TypingStatus::Typing).into();
                match api.encode_and_encrypt(&msg, &recipient_key) {
                    Ok(encrypted) => {
                        if let Err(err) = api.send(&to, &encrypted, false).await {
                            tracing::warn!("Failed to send typing indicator to {to}: {err}");
                        }
                    }
                    Err(err) => {
                        tracing::warn!("Failed to encrypt typing indicator for {to}: {err}");
                    }
                }
            }
        }));
    }

    /// Stop the typing indicator.
    ///
    /// Aborts the repeating task and sends a stop-typing indicator. Safe to call
    /// even if [`send`](Self::send) was never called.
    pub(crate) async fn stop(&self) {
        let handle = {
            let mut guard = self.task.lock().expect("typing handle lock poisoned");
            guard.take()
        };

        if let Some(handle) = handle {
            handle.abort();

            let msg =
                E2eMessage::TypingIndicator(TypingIndicatorMessage::new(TypingStatus::NotTyping));
            match self.api.encode_and_encrypt(&msg, &self.recipient_key) {
                Ok(encrypted) => {
                    if let Err(err) = self.api.send(&self.to, &encrypted, false).await {
                        tracing::warn!(
                            "Failed to send stop-typing indicator to {}: {err}",
                            self.to
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed to encrypt stop-typing indicator for {}: {err}",
                        self.to
                    );
                }
            }
        }
    }
}

/// Action returned by handler methods to indicate how the bot should respond.
#[derive(Debug, Clone)]
pub enum Action {
    /// Do nothing (no response to the user).
    Ignore,
    /// Show the auto-generated help text, optionally preceded by a message.
    ShowHelp {
        /// Optional text shown before the help text (e.g. "Unknown command: /foo").
        prelude: Option<String>,
    },
    /// Send these responses to the user.
    Respond(Vec<Response>),
}

/// Trait for implementing custom message handling logic.
///
/// Implement this trait to define how your bot responds to messages.
///
/// Use the [async-trait](https://crates.io/crates/async-trait) crate to implement this trait.
///
/// # Example
///
/// ```rust
/// use async_trait::async_trait;
/// use threema_gateway_bot::server::handler::{
///     Action, HandlerResult, MessageContext, MessageHandler, Response, TypingHandle,
/// };
///
/// struct EchoMessageHandler;
///
/// #[async_trait]
/// impl MessageHandler for EchoMessageHandler {
///     async fn handle_text(
///         &self,
///         ctx: &MessageContext,
///         text: &str,
///         typing: &TypingHandle,
///     ) -> HandlerResult<Action> {
///         Ok(Action::Respond(vec![Response::text(text)]))
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait MessageHandler: Send + Sync + 'static {
    /// Bot description shown at the top of the help text.
    ///
    /// Return `None` (the default) to omit the description.
    fn description(&self) -> Option<&str> {
        None
    }

    /// Define the commands this bot supports.
    ///
    /// The default returns an empty command set (only the built-in `/help` command).
    #[must_use]
    fn commands() -> Commands {
        Commands::new()
    }

    /// Handle an incoming text message.
    ///
    /// Called for non-command messages that should be processed by your bot logic.
    ///
    /// Use `typing.send()` to show a typing indicator while processing. The indicator is
    /// automatically reset when this method returns.
    async fn handle_text(
        &self,
        ctx: &MessageContext,
        text: &str,
        typing: &TypingHandle,
    ) -> HandlerResult<Action>;

    /// Handle a command.
    ///
    /// Called when a `/command` is received. The `command` parameter contains the
    /// command name without the leading `/`, and `args` contains any text after
    /// the command. The `command_type` indicates how the command was classified.
    ///
    /// For registered commands (via [`Commands::register`]), this is always called.
    /// For unknown commands, this is only called if [`Commands::handle_unknown`]
    /// was set to `true`.
    ///
    /// Use `typing.send` to show a typing indicator while processing. The indicator is
    /// automatically reset when this method returns.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn commands() -> Commands {
    ///     Commands::new()
    ///         .register("remind", "Set a reminder")
    ///         .register("list", "List your reminders")
    /// }
    ///
    /// async fn handle_command(
    ///     &self,
    ///     ctx: &MessageContext,
    ///     command: &str,
    ///     args: &str,
    ///     command_type: CommandType,
    ///     typing: &TypingHandle,
    /// ) -> HandlerResult<Action> {
    ///     match command {
    ///         "remind" => Ok(Action::Respond(self.handle_remind(ctx, args).await?)),
    ///         "list" => Ok(Action::Respond(self.handle_list(ctx).await?)),
    ///         _ => Ok(Action::ShowHelp { prelude: None }),
    ///     }
    /// }
    /// ```
    #[expect(unused_variables, reason = "Default trait method impl")]
    async fn handle_command(
        &self,
        ctx: &MessageContext,
        command: &str,
        args: &str,
        command_type: CommandType,
        typing: &TypingHandle,
    ) -> HandlerResult<Action> {
        Ok(Action::ShowHelp { prelude: None })
    }

    /// Handle a classic reaction (thumbs up or thumbs down, sent in a delivery receipt message).
    #[expect(unused_variables, reason = "Default trait method impl")]
    async fn handle_classic_reaction(
        &self,
        ctx: &MessageContext,
        reaction: ClassicReaction,
    ) -> HandlerResult<Action> {
        Ok(Action::Ignore)
    }

    /// Called after a response message is sent to the user.
    ///
    /// This provides the message ID of the sent message, which can be used
    /// for tracking reactions to that message (e.g., for confirmation flows).
    ///
    /// # Arguments
    /// * `ctx` - The original message context (contains user's message ID)
    /// * `sent_message_id` - The ID of the message that was just sent by the bot
    #[expect(unused_variables, reason = "Default trait method impl")]
    async fn on_response_sent(
        &self,
        ctx: &MessageContext,
        sent_message_id: MessageId,
    ) -> HandlerResult<()> {
        Ok(()) // Default: no-op
    }
}

/// Message context passed to the handler.
#[derive(Debug, Clone)]
pub struct MessageContext {
    /// The incoming message ID.
    pub message_id: MessageId,
    /// Sender's Threema ID.
    pub sender_identity: ThreemaId,
    /// Sender's public nickname.
    pub sender_nickname: Option<String>,
    /// Timestamp when the message was created.
    ///
    /// Note: This value is set by the sender of the message (not the server).
    pub created_at: DateTime<Utc>,
}

/// A classic reaction (i.e. thumbs up or down, sent in a delivery receipt message).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ClassicReaction {
    /// Thumbs up / acknowledge
    ThumbsUp,
    /// Thumbs down / decline
    ThumbsDown,
}

impl TryFrom<DeliveryReceipt> for ClassicReaction {
    type Error = String;

    fn try_from(value: DeliveryReceipt) -> Result<Self, Self::Error> {
        match value {
            DeliveryReceipt::Acknowledged => Ok(Self::ThumbsUp),
            DeliveryReceipt::Declined => Ok(Self::ThumbsDown),
            other => Err(format!("Invalid delivery receipt type: {other:?}")),
        }
    }
}

/// A response towards a user contacting the bot.
#[derive(Debug, Clone)]
pub enum Response {
    /// Text to send back to the user.
    Text(String),
    /// Image displayed inline in the chat (media rendering type).
    Image(FileResponse),
    /// File shown as a downloadable attachment (file rendering type).
    File(FileResponse),
}

/// File/image response data.
#[derive(Debug, Clone)]
pub struct FileResponse {
    /// The file data bytes.
    pub data: Vec<u8>,
    /// The media (MIME) type (e.g., "image/jpeg", "application/pdf").
    pub media_type: String,
    /// Optional file name.
    pub file_name: Option<String>,
    /// Optional caption/description.
    pub caption: Option<String>,
}

impl Response {
    /// Create a simple text response.
    pub fn text<T: Into<String>>(text: T) -> Self {
        Self::Text(text.into())
    }

    /// Create an image response displayed inline in the chat.
    ///
    /// Note: Media type should be either "image/jpeg" or "image/png".
    pub fn image<M: Into<String>>(data: Vec<u8>, media_type: M, caption: Option<String>) -> Self {
        Self::Image(FileResponse {
            data,
            media_type: media_type.into(),
            file_name: None,
            caption,
        })
    }

    /// Create a file response shown as a downloadable attachment.
    pub fn file<M: Into<String>>(
        data: Vec<u8>,
        media_type: M,
        file_name: Option<String>,
        caption: Option<String>,
    ) -> Self {
        Self::File(FileResponse {
            data,
            media_type: media_type.into(),
            file_name,
            caption,
        })
    }
}
