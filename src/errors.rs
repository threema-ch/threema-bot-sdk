//! Error types for the gateway bot.

use thiserror::Error;
use threema_gateway::{
    cache::InMemoryPublicKeyCacheError,
    errors::{ApiBuilderError, ApiError, CryptoError},
    protocol::{ThreemaId, e2e::file::FileMessageBuilderError},
};

use crate::server::handler::HandlerError;

/// Error returned by [`BotConfig::load`](crate::config::BotConfig::load) and
/// [`BotConfig::load_with_prefix`](crate::config::BotConfig::load_with_prefix).
#[derive(Debug, Error)]
#[error("failed to load configuration: {0}")]
pub struct ConfigLoadError(#[from] pub config::ConfigError);

/// Error returned by [`BotServer::new`](crate::server::BotServer::new).
#[derive(Debug, Error)]
pub enum InitError {
    /// Configuration validation failed.
    #[error("invalid configuration: {0}")]
    Config(#[from] ConfigError),

    /// Failed to construct the Threema Gateway API client.
    #[error("failed to create API client: {0}")]
    ApiBuilder(#[from] ApiBuilderError),
}

/// Configuration validation error.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Gateway ID is not 8 characters or doesn't start with `*`.
    #[error("gateway_id must be 8 characters starting with '*', got: {0}")]
    InvalidGatewayId(String),

    /// API secret is empty.
    #[error("api_secret must not be empty")]
    EmptyApiSecret,

    /// Rate limiting value is invalid.
    #[error("invalid rate limit: {0}")]
    InvalidRateLimit(String),
}

/// Error returned by [`BotServer::run`](crate::server::BotServer::run).
#[derive(Debug, Error)]
pub enum RunError {
    /// Failed to validate Threema Gateway credentials on startup.
    #[error("failed to validate Threema Gateway credentials: {0}")]
    CredentialValidation(#[source] ApiError),

    /// Failed to bind the HTTP server.
    #[error("failed to bind server: {0}")]
    Bind(#[source] std::io::Error),

    /// The HTTP server returned an error.
    #[error("server error: {0}")]
    Serve(#[source] std::io::Error),
}

/// Error when sending a message.
#[derive(Debug, Error)]
pub enum SendError {
    /// Failed to look up a public key via the API.
    #[error("failed to look up public key for {identity}: {source}")]
    PublicKeyLookup {
        /// The Threema ID whose public key lookup failed.
        identity: ThreemaId,
        /// The underlying API error.
        #[source]
        source: ApiError,
    },

    /// Failed to look up a public key from the cache.
    #[error("public key cache error for {identity}: {source}")]
    PublicKeyCache {
        /// The Threema ID whose public key lookup failed.
        identity: ThreemaId,
        /// The underlying cache error.
        #[source]
        source: InMemoryPublicKeyCacheError,
    },

    /// Failed to encrypt a message.
    #[error("failed to encrypt message: {0}")]
    Encrypt(#[source] CryptoError),

    /// Failed to send a message via the API.
    #[error("failed to send message: {0}")]
    Send(#[source] ApiError),

    /// Failed to encrypt file data.
    #[error("failed to encrypt file data: {0}")]
    FileEncrypt(#[source] CryptoError),

    /// Failed to upload a blob.
    #[error("failed to upload {kind} blob: {source}")]
    BlobUpload {
        /// The kind of blob that failed to upload (e.g. `"file"` or `"thumbnail"`).
        kind: &'static str,
        /// The underlying API error.
        #[source]
        source: ApiError,
    },

    /// Failed to build a file message.
    #[error("failed to build file message: {0}")]
    FileBuild(#[from] FileMessageBuilderError),

    /// Invalid media type for image message.
    #[error("wrong media type, expected: {expected}, got: {got}")]
    InvalidMediaType {
        /// The media type that was expected (e.g. `"image/*"`).
        expected: &'static str,
        /// The media type that was actually provided.
        got: String,
    },

    /// Handler callback failed.
    #[error("handler callback failed: {0}")]
    Handler(#[source] HandlerError),
}

/// Error when validating a webhook timestamp.
#[derive(Debug, Error)]
pub(crate) enum TimestampValidationError {
    /// Webhook timestamp is too old.
    #[error("webhook timestamp too old: age={age_seconds}s (max={max_seconds}s)")]
    TooOld { age_seconds: i64, max_seconds: i64 },

    /// Webhook timestamp is from the future.
    #[error("webhook timestamp from the future: offset={offset_seconds}s (max={max_seconds}s)")]
    FromFuture {
        offset_seconds: i64,
        max_seconds: i64,
    },
}
