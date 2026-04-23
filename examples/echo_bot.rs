//! Example for a simple bot that echoes text messages back to the user.
//!
//! # Usage
//!
//! ```text
//! echo_bot --config config.toml
//! ```
//!
//! Configuration is loaded from the provided TOML file and can be overridden
//! with environment variables using the `EXAMPLE_ECHO_BOT__` prefix, e.g.:
//!
//! ```text
//! EXAMPLE_ECHO_BOT__THREEMA__GATEWAY_ID=*MYBOT12 echo_bot --config config.toml
//! ```

use std::{env, path::PathBuf};

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use threema_gateway_bot::{
    config::BotConfig,
    server::{
        BotServer,
        handler::{Action, HandlerResult, MessageContext, MessageHandler, Response, TypingHandle},
    },
};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

/// A simple message handler that echoes every text message back to the sender.
struct EchoMessageHandler;

#[async_trait]
impl MessageHandler for EchoMessageHandler {
    fn description(&self) -> Option<&str> {
        Some("Echo bot: Every message you send will be echoed back to you.")
    }

    async fn handle_text(
        &self,
        _ctx: &MessageContext,
        text: &str,
        _typing: &TypingHandle,
    ) -> HandlerResult<Action> {
        Ok(Action::Respond(vec![Response::text(text)]))
    }
}

/// Parses the `--config <path>` flag from the command-line arguments.
///
/// Exits with a usage message if the flag is missing or has no value.
fn parse_args() -> Result<PathBuf> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            let path = args.next().context("--config requires a path argument")?;
            return Ok(PathBuf::from(path));
        }
    }
    anyhow::bail!("Usage: echo_bot --config <path>")
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up logging
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,threema_gateway_bot=debug")),
        )
        .init();

    // Load config
    let config_path = parse_args()?;
    let config = BotConfig::load_with_prefix("EXAMPLE_ECHO_BOT", &config_path)?;
    info!(
        "Starting echo bot on {}:{}",
        config.server.host, config.server.port
    );

    // Instantiate and run bot server
    BotServer::new(config, EchoMessageHandler)?.run().await?;

    Ok(())
}
