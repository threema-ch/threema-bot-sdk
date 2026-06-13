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
//! The command parsing infrastructure allows for both slash-command style (`/remind 30m`) or
//! word-command style (`remind 30m`).
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

mod client;
pub mod commands;
pub mod config;
mod dedup;
pub mod errors;
mod rate_limit;
pub mod server;
mod webhook;
