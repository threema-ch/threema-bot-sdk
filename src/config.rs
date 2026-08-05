//! Configuration management.
//!
//! This module provides [`BotConfig`] and its sub-structs ([`ServerConfig`], [`ThreemaConfig`],
//! [`RateLimitingConfig`]).
//!
//! # Minimal Example Config
//!
//! ```toml
//! [server]
//! host = "0.0.0.0"
//! port = 8080
//! webhook_path = "/webhook"
//!
//! [threema]
//! gateway_id = "*MYBOT01"
//! private_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
//! api_secret = "your-api-secret"
//! ```
//!
//! All other values have defaults and can be omitted.
//!
//! Note: All config values can also be passed via environment variables rather than the config
//! file. Depending on your setup, this might make sense for secrets like `private_key` or
//! `api_secret`.
//!
//! # Configuration Options
//!
//! The table below lists all configuration options that are used / required by Threema Gateway Bot.
//! The config system can be extended with custom variables, used by your bot implementation.
//!
//! Environment variable names use the prefix passed to [`BotConfig::load_with_prefix`] (shown here
//! as `PREFIX`).
//!
//! ## `[server]`
//!
//! | Key / Env Var | Description | Required | Default |
//! |---|---|---|---|
//! | `host`<br>`PREFIX__SERVER__HOST` | IP address or hostname to bind to (e.g. `0.0.0.0`) | Yes | - |
//! | `port`<br>`PREFIX__SERVER__PORT` | TCP port to listen on (e.g. `8080`) | Yes | - |
//! | `webhook_path`<br>`PREFIX__SERVER__WEBHOOK_PATH` | URL path for webhook callbacks (e.g. `/webhook`) | Yes | - |
//! | `message_id_cache_size`<br>`PREFIX__SERVER__MESSAGE_ID_CACHE_SIZE` | Max message IDs in dedup cache | No | `100000` |
//! | `max_webhook_age_seconds`<br>`PREFIX__SERVER__MAX_WEBHOOK_AGE_SECONDS` | Max age of webhook timestamps (seconds) | No | `300` |
//!
//! ## `[threema]`
//!
//! | Key / Env Var | Description | Required | Default |
//! |---|---|---|---|
//! | `gateway_id`<br>`PREFIX__THREEMA__GATEWAY_ID` | Bot's Gateway ID (8 chars, starts with `*`) | Yes | - |
//! | `private_key`<br>`PREFIX__THREEMA__PRIVATE_KEY` | E2E private key (hex) | Yes | - |
//! | `api_secret`<br>`PREFIX__THREEMA__API_SECRET` | API secret from [gateway.threema.ch](https://gateway.threema.ch/) | Yes | - |
//! | `api_url`<br>`PREFIX__THREEMA__API_URL` | Gateway API base URL | No | `https://msgapi.threema.ch` |
//! | `allowed_users`<br>`PREFIX__THREEMA__ALLOWED_USERS` | Threema IDs allowed to use the bot (comma-separated list when set through the environment, e.g. `ECHOECHO,ABCD1234`) | No | `[]` (allow anyone) |
//!
//! ## `[rate_limiting]`
//!
//! | Key / Env Var | Description | Required | Default |
//! |---|---|---|---|
//! | `enabled`<br>`PREFIX__RATE_LIMITING__ENABLED` | Enable per-user rate limiting | No | `true` |
//! | `messages_per_minute`<br>`PREFIX__RATE_LIMITING__MESSAGES_PER_MINUTE` | Max messages per user per minute | No | `20` |
//! | `messages_per_hour`<br>`PREFIX__RATE_LIMITING__MESSAGES_PER_HOUR` | Max messages per user per hour | No | `100` |
//!
//! # Usage
//!
//! There are three ways to configure your bot:
//!
//! ## 1. Use `BotConfig::load` directly
//!
//! The simplest approach. Loads the default config from a TOML file with environment variable
//! overrides (env vars take precedence):
//!
//! ```rust,no_run
//! # use std::path::Path;
//! use threema_gateway_bot::{config::BotConfig, server::BotServer};
//! # use threema_gateway_bot::server::handler::{Action, HandlerResult, MessageContext, MessageHandler, Response, TypingHandle};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # struct MyHandler;
//! # #[async_trait::async_trait]
//! # impl MessageHandler for MyHandler {
//! #     async fn handle_text(&self, _ctx: &MessageContext, text: &str, typing: &TypingHandle) -> HandlerResult<Action> {
//! #         Ok(Action::Ignore)
//! #     }
//! # }
//! # let handler = MyHandler {};
//! let config = BotConfig::load_with_prefix("MYBOT", Path::new("config.toml"))?;
//! BotServer::new(config, handler)?;
//! # Ok(())
//! # }
//! ```
//!
//! Environment variables use the format `PREFIX__SECTION__KEY`, e.g. `MYBOT__THREEMA__API_SECRET`.
//!
//! List values are comma-separated, e.g. `MYBOT__THREEMA__ALLOWED_USERS=ECHOECHO,ABCD1234`. Note
//! that the entries are not trimmed, so don't add spaces around the commas.
//!
//! ## 2. Extend with custom fields
//!
//! If your bot needs additional configuration (e.g. an API key, a database URL), define your own
//! config struct that embeds the library's types. Use the [`config`](https://docs.rs/config) crate
//! (already a dependency) to load everything from a single file:
//!
//! ```rust,no_run
//! use serde::Deserialize;
//! use threema_gateway_bot::{
//!     config::{BotConfig, RateLimitingConfig, ServerConfig, ThreemaConfig},
//!     server::BotServer,
//! };
//! # use threema_gateway_bot::server::handler::{Action, HandlerResult, MessageContext, MessageHandler, Response, TypingHandle};
//!
//! #[derive(Deserialize)]
//! struct MyBotConfig {
//!     // Sections required for creating `BotServer`
//!     server: ServerConfig,
//!     threema: ThreemaConfig,
//!     #[serde(default)]
//!     rate_limiting: RateLimitingConfig,
//!
//!     // Your own fields
//!     database_url: String,
//!     api_token: String,
//!     admins: Vec<String>,
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load with the `config` crate
//! //
//! // `BotConfig::env_source` exposes an environment source that is set up like the one used by
//! // `BotConfig::load_with_prefix`, so the settings of this crate (including its list keys)
//! // don't have to be replicated. Sequence fields of your own config need to be registered with
//! // `with_list_parse_key`.
//! let raw = config::Config::builder()
//!     .add_source(config::File::with_name("config.toml"))
//!     .add_source(BotConfig::env_source("MYBOT").with_list_parse_key("admins"))
//!     .build()?;
//! let my_config: MyBotConfig = raw.try_deserialize()?;
//!
//! // Construct the library's BotConfig from the parts
//! let bot_config = BotConfig {
//!     server: my_config.server,
//!     threema: my_config.threema,
//!     rate_limiting: my_config.rate_limiting,
//! };
//!
//! # struct MyHandler;
//! # #[async_trait::async_trait]
//! # impl MessageHandler for MyHandler {
//! #     async fn handle_text(&self, _ctx: &MessageContext, text: &str, typing: &TypingHandle) -> HandlerResult<Action> {
//! #         Ok(Action::Ignore)
//! #     }
//! # }
//! # let handler = MyHandler {};
//! BotServer::new(bot_config, handler)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## 3. Use a different config system entirely
//!
//! All config structs have public fields and derive `Deserialize`, so they work with any
//! deserialization framework. Construct [`BotConfig`] manually:
//!
//! ```rust,ignore
//! let config = BotConfig {
//!     server: ServerConfig { /* ... */ },
//!     threema: ThreemaConfig { /* ... */ },
//!     rate_limiting: RateLimitingConfig::default(),
//! };
//! BotServer::new(config, handler)?;
//! ```
//!
//! Validation happens in [`BotServer::new`](crate::server::BotServer::new), so all three approaches
//! get the same checks.

use std::path::Path;

use serde::{Deserialize, Serialize};
use threema_gateway::{SecretKey, protocol::ThreemaId};

use crate::errors::{ConfigError, ConfigLoadError};

/// Main configuration struct for a Threema bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    /// Server config
    pub server: ServerConfig,
    /// Threema config
    pub threema: ThreemaConfig,
    /// Optional rate limiting config
    #[serde(default)]
    pub rate_limiting: RateLimitingConfig,
}

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// IP address or hostname the HTTP server binds to (e.g. `"0.0.0.0"` or `"127.0.0.1"`)
    pub host: String,
    /// TCP port the HTTP server listens on (e.g. 8080)
    pub port: u16,
    /// URL path at which Threema Gateway delivers webhook callbacks (e.g. `/webhook/`)
    pub webhook_path: String,
    /// Maximum number of message IDs kept in the deduplication cache
    ///
    /// Defaults to `100_000`
    #[serde(default = "default_message_id_cache_size")]
    pub message_id_cache_size: u64,
    /// Maximum age of webhook timestamps in seconds
    ///
    /// Webhooks older (or further in the future) than this are rejected to
    /// prevent replay attacks. Defaults to 300 (5 minutes).
    #[serde(default = "default_max_webhook_age_seconds")]
    pub max_webhook_age_seconds: i64,
}

fn default_message_id_cache_size() -> u64 {
    100_000
}

fn default_max_webhook_age_seconds() -> i64 {
    300
}

/// Threema Gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreemaConfig {
    /// Threema Gateway ID of the bot (8 characters, must start with `*`)
    pub gateway_id: ThreemaId,
    /// Private key for E2E encryption (32 bytes, serialized as lowercase hex)
    pub private_key: SecretKey,
    /// API secret as listed at gateway.threema.ch
    pub api_secret: String,
    /// Base URL of the Threema Gateway API. Defaults to `https://msgapi.threema.ch`
    ///
    /// Override for testing against a custom endpoint.
    #[serde(default = "default_api_url")]
    pub api_url: String,
    /// List of Threema IDs allowed to interact with the bot
    ///
    /// Messages from IDs not on this list are rejected. An empty list allows anyone.
    #[serde(default)]
    pub allowed_users: Vec<ThreemaId>,
}

fn default_api_url() -> String {
    "https://msgapi.threema.ch".to_owned()
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingConfig {
    /// Whether per-user rate limiting is active
    ///
    /// Defaults to `true`.
    #[serde(default = "default_rate_limiting_enabled")]
    pub enabled: bool,
    /// Maximum number of messages a single user may send within any one-minute window
    ///
    /// Must be greater than zero when rate limiting is enabled. Defaults to `20`.
    #[serde(default = "default_messages_per_minute")]
    pub messages_per_minute: u32,
    /// Maximum number of messages a single user may send within any one-hour window
    ///
    /// Must be greater than zero when rate limiting is enabled. Defaults to `100`.
    #[serde(default = "default_messages_per_hour")]
    pub messages_per_hour: u32,
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            enabled: default_rate_limiting_enabled(),
            messages_per_minute: default_messages_per_minute(),
            messages_per_hour: default_messages_per_hour(),
        }
    }
}

fn default_rate_limiting_enabled() -> bool {
    true
}

fn default_messages_per_minute() -> u32 {
    20
}

fn default_messages_per_hour() -> u32 {
    100
}

impl BotConfig {
    /// Default environment variable prefix.
    pub(crate) const DEFAULT_ENV_PREFIX: &'static str = "THREEMABOT";
    /// Separator for nested configuration values in environment variables (`PREFIX__SECTION__KEY`).
    pub const ENV_SEPARATOR: &'static str = "__";
    /// Separator for list values in environment variables (`ONE,TWO`).
    pub const ENV_LIST_SEPARATOR: &'static str = ",";
    /// Configuration keys of this crate whose environment variable values are parsed as lists.
    ///
    /// Sequence-typed fields must be registered with
    /// [`Environment::with_list_parse_key`](config::Environment::with_list_parse_key), otherwise
    /// setting them through an environment variable fails with an "invalid type: string, expected a
    /// sequence" error. Keys are lowercase and dot-separated, without the environment variable
    /// prefix.
    ///
    /// [`BotConfig::env_source`] registers these keys already. Use this constant only if you build
    /// the environment source yourself, e.g. with a different configuration library:
    ///
    /// ```rust
    /// use threema_gateway_bot::config::BotConfig;
    ///
    /// let mut source = config::Environment::with_prefix("MYBOT")
    ///     .separator(BotConfig::ENV_SEPARATOR)
    ///     .try_parsing(true)
    ///     .list_separator(BotConfig::ENV_LIST_SEPARATOR);
    /// for key in BotConfig::ENV_LIST_KEYS {
    ///     source = source.with_list_parse_key(key);
    /// }
    /// ```
    pub const ENV_LIST_KEYS: &'static [&'static str] = &["threema.allowed_users"];

    /// Load configuration with the default prefix (`THREEMABOT`).
    pub fn load(config_path: &Path) -> Result<Self, ConfigLoadError> {
        Self::load_with_prefix(Self::DEFAULT_ENV_PREFIX, config_path)
    }

    /// Load configuration with a custom environment variable prefix.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use std::path::Path;
    /// use threema_gateway_bot::config::BotConfig;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// // Uses MYBOT__SERVER__HOST, MYBOT__THREEMA__PRIVATE_KEY, etc.
    /// let config = BotConfig::load_with_prefix("MYBOT", Path::new("config.toml"))?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_with_prefix(prefix: &str, config_path: &Path) -> Result<Self, ConfigLoadError> {
        tracing::debug!(
            "Loading configuration from: {} (env prefix: {prefix})",
            config_path.display()
        );

        // Build config
        let config = config::Config::builder()
            .add_source(config::File::from(config_path).required(false))
            .add_source(Self::env_source(prefix))
            .build()?;

        // Deserialize
        let cfg: BotConfig = config.try_deserialize()?;

        Ok(cfg)
    }

    /// Build the environment variable source used by [`BotConfig::load_with_prefix`].
    ///
    /// The returned source uses `PREFIX__SECTION__KEY` names, parses booleans and numbers, and
    /// registers all sequence-typed keys of this crate ([`BotConfig::ENV_LIST_KEYS`]) as
    /// comma-separated lists.
    ///
    /// Use this when you extend the configuration with your own fields, so that the settings of
    /// this crate don't have to be replicated by hand. Own sequence fields still need to be
    /// registered, but only your own:
    ///
    /// ```rust
    /// use threema_gateway_bot::config::BotConfig;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let raw = config::Config::builder()
    ///     .add_source(config::File::with_name("config.toml").required(false))
    ///     .add_source(BotConfig::env_source("MYBOT").with_list_parse_key("my_section.my_list"))
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Note: This method exposes a type of the [`config`](https://docs.rs/config) crate, so it can
    /// only be used if your bot depends on the same major version of that crate as this crate does.
    /// If it doesn't, build the source yourself using [`BotConfig::ENV_SEPARATOR`],
    /// [`BotConfig::ENV_LIST_SEPARATOR`] and [`BotConfig::ENV_LIST_KEYS`].
    pub fn env_source(prefix: &str) -> config::Environment {
        let mut source = config::Environment::with_prefix(prefix)
            .prefix_separator(Self::ENV_SEPARATOR)
            .separator(Self::ENV_SEPARATOR)
            .try_parsing(true)
            .list_separator(Self::ENV_LIST_SEPARATOR);
        for key in Self::ENV_LIST_KEYS {
            source = source.with_list_parse_key(key);
        }
        source
    }

    /// Validate configuration values.
    ///
    /// Called by [`BotServer::new`](crate::server::BotServer::new) to validate
    /// the configuration before starting the server. This ensures validation
    /// happens regardless of how the config was constructed.
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        // Validate Gateway ID (must be a gateway ID starting with *)
        if !self.threema.gateway_id.is_gateway_id() {
            return Err(ConfigError::InvalidGatewayId(
                self.threema.gateway_id.to_string(),
            ));
        }

        // Validate API secret
        if self.threema.api_secret.is_empty() {
            return Err(ConfigError::EmptyApiSecret);
        }

        // Validate rate limiting
        if self.rate_limiting.enabled {
            if self.rate_limiting.messages_per_minute == 0 {
                return Err(ConfigError::InvalidRateLimit(
                    "messages_per_minute must be > 0 when enabled".into(),
                ));
            }
            if self.rate_limiting.messages_per_hour == 0 {
                return Err(ConfigError::InvalidRateLimit(
                    "messages_per_hour must be > 0 when enabled".into(),
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use threema_gateway::{SecretKey, protocol::ThreemaId};

    use crate::{
        config::{BotConfig, RateLimitingConfig, ServerConfig, ThreemaConfig},
        errors::ConfigLoadError,
    };

    /// Load a [`BotConfig`] from the passed environment variables (prefix `TESTBOT`), without
    /// touching the process environment or the filesystem.
    fn load_from_env(vars: Vec<(&str, &str)>) -> Result<BotConfig, ConfigLoadError> {
        let source = BotConfig::env_source("TESTBOT").source(Some(
            vars.into_iter()
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
        ));
        Ok(config::Config::builder()
            .add_source(source)
            .build()?
            .try_deserialize()?)
    }

    fn create_test_config() -> BotConfig {
        BotConfig {
            server: ServerConfig {
                host: "0.0.0.0".to_owned(),
                port: 8080,
                webhook_path: "/webhook".to_owned(),
                message_id_cache_size: 100_000,
                max_webhook_age_seconds: 300,
            },
            threema: ThreemaConfig {
                gateway_id: ThreemaId::try_from("*TESTBOT").unwrap(),
                private_key: SecretKey::from_bytes([0x42_u8; 32]),
                api_secret: "secret".to_owned(),
                api_url: "https://msgapi.threema.ch".to_owned(),
                allowed_users: vec![ThreemaId::try_from("ABCD1234").unwrap()],
            },
            rate_limiting: RateLimitingConfig::default(),
        }
    }

    #[test]
    fn valid_config() {
        let config = create_test_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn invalid_gateway_id() {
        let mut config = create_test_config();
        config.threema.gateway_id = ThreemaId::try_from("NOTASTAR").unwrap();
        assert!(config.validate().is_err());
    }

    mod env_source {
        use serde::Deserialize;

        use crate::config::{BotConfig, tests::load_from_env};

        /// Load a config purely from environment variables.
        fn base_env() -> Vec<(&'static str, &'static str)> {
            vec![
                ("TESTBOT__SERVER__HOST", "0.0.0.0"),
                ("TESTBOT__SERVER__PORT", "8080"),
                ("TESTBOT__SERVER__WEBHOOK_PATH", "/webhook"),
                ("TESTBOT__THREEMA__GATEWAY_ID", "*TESTBOT"),
                (
                    "TESTBOT__THREEMA__PRIVATE_KEY",
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                ),
                ("TESTBOT__THREEMA__API_SECRET", "secret"),
            ]
        }

        #[test]
        fn single_list_entry() {
            let mut env = base_env();
            env.push(("TESTBOT__THREEMA__ALLOWED_USERS", "ECHOECHO"));
            let config = load_from_env(env).unwrap();
            assert_eq!(
                config
                    .threema
                    .allowed_users
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                vec!["ECHOECHO"],
            );
        }

        #[test]
        fn multiple_list_entries() {
            let mut env = base_env();
            env.push(("TESTBOT__THREEMA__ALLOWED_USERS", "ECHOECHO,ABCD1234"));
            let config = load_from_env(env).unwrap();
            assert_eq!(
                config
                    .threema
                    .allowed_users
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                vec!["ECHOECHO", "ABCD1234"],
            );
        }

        #[test]
        fn omitted_list() {
            let config = load_from_env(base_env()).unwrap();
            assert!(
                config.threema.allowed_users.is_empty(),
                "allowed_users should default to an empty list"
            );
        }

        /// Non-list string values must not be turned into lists by the list separator.
        #[test]
        fn string_containing_separator() {
            let mut env = base_env();
            env.push(("TESTBOT__THREEMA__API_SECRET", "a,b"));
            let config = load_from_env(env).unwrap();
            assert_eq!(config.threema.api_secret, "a,b");
        }

        /// A bot extending the config can register its own list keys on top of the library's.
        #[test]
        fn custom_list_key() {
            #[derive(Deserialize)]
            struct CustomConfig {
                admins: Vec<String>,
                database_url: String,
            }

            let source = BotConfig::env_source("TESTBOT")
                .with_list_parse_key("admins")
                .source(Some(
                    [
                        ("TESTBOT__ADMINS".to_owned(), "alice,bob".to_owned()),
                        (
                            "TESTBOT__DATABASE_URL".to_owned(),
                            "postgres://localhost".to_owned(),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
            let custom: CustomConfig = config::Config::builder()
                .add_source(source)
                .build()
                .unwrap()
                .try_deserialize()
                .unwrap();

            assert_eq!(custom.admins, vec!["alice", "bob"]);
            assert_eq!(custom.database_url, "postgres://localhost");
        }

        /// Every registered list key must look valid.
        #[test]
        fn list_keys_are_lowercase_and_dotted() {
            for key in BotConfig::ENV_LIST_KEYS {
                assert!(
                    key.chars()
                        .all(|chr| chr.is_ascii_lowercase() || chr == '_' || chr == '.'),
                    "list key {key} must be lowercase and dot-separated"
                );
            }
        }
    }
}
