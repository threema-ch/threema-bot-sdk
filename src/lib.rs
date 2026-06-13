//! # Threema Bot SDK
//!
//! A library for building Threema Gateway bots in Rust.
//!
//! This library provides the foundational components for building Threema bots
//! with:
//!
//! - **Webhook handling** to receive and validate Threema Gateway messages
//! - **Configuration system** based on TOML files and env vars, extensible by your bot
//! - **Rate limiting** and **caching** built-in
//! - **Command parsing** infrastructure with grouped, per-user help text
//!
//! The command parsing allows for both slash-command style (`/remind 30m`) or word-command style
//! (`remind 30m`).
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use std::path::Path;
//! use threema_gateway_bot::{
//!     config::BotConfig,
//!     server::{
//!         BotServer,
//!         handler::{Action, HandlerResult, MessageContext, MessageHandler, Response, TypingHandle},
//!     },
//! };
//!
//! // Create a handler struct
//! struct MyHandler;
//!
//! // Implement `MessageHandler` trait for your struct
//! #[async_trait::async_trait]
//! impl MessageHandler for MyHandler {
//!     async fn handle_text(&self, _ctx: &MessageContext, text: &str, typing: &TypingHandle) -> HandlerResult<Action> {
//!         let text_response = Response::text(format!("You said: {}", text));
//!         Ok(Action::Respond(vec![text_response]))
//!     }
//! }
//!
//! // Start bot server
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = BotConfig::load_with_prefix("MYBOT", Path::new("config.toml"))?;
//!     BotServer::new(config, MyHandler)?.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Sending messages proactively
//!
//! Handlers react to incoming messages, but a bot can also send messages on its
//! own – for example in reaction to an external event such as a subscription
//! update. Obtain a [`ThreemaClient`](client::ThreemaClient) from
//! [`BotServer::client()`](server::BotServer::client) and move it into a
//! background task. Grab the handle *before* calling
//! [`run()`](server::BotServer::run), which borrows the server for the lifetime
//! of the process:
//!
//! ```rust,no_run
//! # use std::path::Path;
//! # use threema_gateway::protocol::ThreemaId;
//! # use threema_gateway_bot::{
//! #     config::BotConfig,
//! #     server::{
//! #         BotServer,
//! #         handler::{Action, HandlerResult, MessageContext, MessageHandler, Response, TypingHandle},
//! #     },
//! # };
//! # struct MyHandler;
//! # #[async_trait::async_trait]
//! # impl MessageHandler for MyHandler {
//! #     async fn handle_text(&self, _ctx: &MessageContext, text: &str, _typing: &TypingHandle) -> HandlerResult<Action> {
//! #         Ok(Action::Respond(vec![Response::text(text)]))
//! #     }
//! # }
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = BotConfig::load_with_prefix("MYBOT", Path::new("config.toml"))?;
//!     let server = BotServer::new(config, MyHandler)?;
//!
//!     // The client handle is cheap to clone and can be moved across tasks.
//!     let client = server.client();
//!     tokio::spawn(async move {
//!         let recipient: ThreemaId = "ECHOECHO".parse().expect("valid Threema ID");
//!         if let Err(err) = client.send_text(&recipient, "Your subscription updated").await {
//!             tracing::error!("Failed to send proactive message: {err}");
//!         }
//!     });
//!
//!     server.run().await?;
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod commands;
pub mod config;
mod dedup;
pub mod errors;
mod rate_limit;
pub mod server;
mod webhook;
