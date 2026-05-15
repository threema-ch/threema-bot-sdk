//! Wrapper for the threema-gateway crate.
//!
//! Provides construction helpers and file message support with automatic
//! thumbnail generation.
//!
//! For low-level operations (sending text messages, delivery receipts,
//! typing indicators, etc.), use [`ThreemaClient::api()`] to access
//! the underlying [`E2eApi`] directly.

use std::{io::Cursor, time::Duration};

use image::{
    ImageEncoder as _, ImageReader,
    codecs::{jpeg::JpegEncoder, png::PngEncoder},
};
use threema_gateway::{
    ApiBuilder, E2eApi, FileData, RecipientKey, SecretKey,
    cache::{InMemoryPublicKeyCache, InMemoryPublicKeyCacheError},
    encrypt_file_data,
    errors::{ApiBuilderError, ApiOrCacheError},
    protocol::{
        MessageId, ThreemaId,
        e2e::file::{FileMessage, RenderingType},
    },
};

use crate::errors::SendError;

/// High-level client for interacting with the Threema Gateway API.
///
/// This wraps [`E2eApi`] and provides:
/// - Construction from bot config ([`from_config`](Self::from_config))
/// - File message support with automatic thumbnail generation
///   ([`send_file_message`](Self::send_file_message))
///
/// For all other operations, use [`api()`](Self::api) to access the
/// underlying [`E2eApi`] directly.
pub(crate) struct ThreemaClient {
    /// API client from threema-gateway library
    e2e_api: E2eApi,
    /// Public key cache
    pubkey_cache: InMemoryPublicKeyCache,
}

/// Base functions.
impl ThreemaClient {
    /// Create a new `ThreemaClient`.
    pub(crate) fn new(
        api_url: &str,
        gateway_id: ThreemaId,
        api_secret: &str,
        private_key: SecretKey,
    ) -> Result<Self, ApiBuilderError> {
        let mut builder = ApiBuilder::new(gateway_id, api_secret);

        // Support custom endpoints
        let api_url_trimmed = api_url.trim_end_matches('/').to_owned();
        if !api_url_trimmed.is_empty() && api_url_trimmed != "https://msgapi.threema.ch" {
            tracing::info!("Using custom Threema Gateway endpoint: {}", api_url_trimmed);
            builder = builder.with_custom_endpoint(api_url_trimmed);
        }

        let e2e_api = builder.with_private_key(private_key).into_e2e()?;

        let pubkey_cache = InMemoryPublicKeyCache::new(10_000, Duration::from_hours(7 * 24));

        Ok(Self {
            e2e_api,
            pubkey_cache,
        })
    }

    /// Create a `ThreemaClient` from config
    pub(crate) fn from_config(
        config: &crate::config::ThreemaConfig,
    ) -> Result<Self, ApiBuilderError> {
        Self::new(
            &config.api_url,
            config.gateway_id,
            &config.api_secret,
            config.private_key.clone(),
        )
    }

    /// Validate that the API credentials are correct.
    ///
    /// Makes a lightweight API call (credit lookup) to verify the gateway ID
    /// and API secret are accepted by the server.
    pub(crate) async fn validate_api_secret(
        &self,
    ) -> Result<(), threema_gateway::errors::ApiError> {
        self.e2e_api.lookup_credits().await?;
        Ok(())
    }

    /// Access the underlying [`E2eApi`] for low-level operations.
    ///
    /// Use this for sending text messages, delivery receipts, typing indicators,
    /// looking up credits, and any other operation provided by the gateway crate.
    pub(crate) fn api(&self) -> &E2eApi {
        &self.e2e_api
    }

    /// Look up the public key for a Threema ID.
    ///
    /// Uses an in-memory cache to avoid repeated network lookups for the same ID.
    pub(crate) async fn lookup_pubkey(
        &self,
        id: &ThreemaId,
    ) -> Result<RecipientKey, ApiOrCacheError<InMemoryPublicKeyCacheError>> {
        self.e2e_api
            .lookup_pubkey_with_cache(id, &self.pubkey_cache)
            .await
    }
}

/// Functions related to sending messages.
#[expect(
    clippy::multiple_inherent_impl,
    reason = "Used for grouping in rustdoc"
)]
impl ThreemaClient {
    /// Send a file as a download-style attachment.
    ///
    /// Uses [`RenderingType::File`]: The file is shown as a downloadable attachment in Threema
    /// clients, even for image types. No thumbnail is generated.
    pub(crate) async fn send_file_message(
        &self,
        to: &ThreemaId,
        file_data: &[u8],
        media_type: &str,
        file_name: Option<&str>,
        caption: Option<&str>,
    ) -> Result<MessageId, SendError> {
        let recipient_key = self
            .lookup_pubkey(to)
            .await
            .map_err(|err| pubkey_lookup_err(*to, err))?;

        self.upload_and_send_file(
            to,
            file_data,
            media_type,
            file_name,
            caption,
            RenderingType::File,
            None,
            &recipient_key,
        )
        .await
    }

    /// Send an image displayed inline in the chat.
    ///
    /// Uses [`RenderingType::Media`] and automatically generates a thumbnail
    /// for preview in Threema clients (PNG for PNG input, JPEG otherwise).
    ///
    /// Returns an error if `media_type` does not start with `image/`.
    pub(crate) async fn send_image_message(
        &self,
        to: &ThreemaId,
        image_data: &[u8],
        media_type: &str,
        caption: Option<&str>,
    ) -> Result<MessageId, SendError> {
        if !media_type.starts_with("image/") {
            return Err(SendError::InvalidMediaType {
                expected: "image/*",
                got: media_type.to_owned(),
            });
        }

        let recipient_key = self
            .lookup_pubkey(to)
            .await
            .map_err(|err| pubkey_lookup_err(*to, err))?;

        let is_png = media_type == "image/png";
        let thumbnail = match generate_thumbnail(image_data, is_png) {
            Ok(thumb) => Some(thumb),
            Err(err) => {
                tracing::warn!("Failed to generate thumbnail: {}", err);
                None
            }
        };

        self.upload_and_send_file(
            to,
            image_data,
            media_type,
            None,
            caption,
            RenderingType::Media,
            thumbnail,
            &recipient_key,
        )
        .await
    }

    /// Encrypt, upload, and send a file message.
    #[expect(
        clippy::too_many_arguments,
        reason = "internal helper grouping upload+send params"
    )]
    async fn upload_and_send_file(
        &self,
        to: &ThreemaId,
        file_data: &[u8],
        media_type: &str,
        file_name: Option<&str>,
        caption: Option<&str>,
        rendering_type: RenderingType,
        thumbnail: Option<Thumbnail>,
        recipient_key: &RecipientKey,
    ) -> Result<MessageId, SendError> {
        // Parse media type
        let media_type: mime::Mime = media_type.parse().unwrap_or_else(|_| {
            "application/octet-stream"
                .parse()
                .expect("valid fallback media type")
        });

        // Encrypt file (and thumbnail) with a random symmetric key
        let file = FileData {
            file: file_data.to_vec(),
            thumbnail: thumbnail.as_ref().map(|th| th.data.clone()),
        };
        let (encrypted, encryption_key) =
            encrypt_file_data(&file).map_err(SendError::FileEncrypt)?;

        // Upload file blob
        let blob_id = self
            .e2e_api
            .blob_upload_raw(&encrypted.file, false)
            .await
            .map_err(|source| SendError::BlobUpload {
                kind: "file",
                source,
            })?;

        // Upload thumbnail blob if present
        let thumbnail_blob_id = if let Some(encrypted_thumb) = encrypted.thumbnail {
            Some(
                self.e2e_api
                    .blob_upload_raw(&encrypted_thumb, false)
                    .await
                    .map_err(|source| SendError::BlobUpload {
                        kind: "thumbnail",
                        source,
                    })?,
            )
        } else {
            None
        };

        // Build file message
        let mut builder = FileMessage::builder(
            blob_id,
            encryption_key,
            media_type.to_string(),
            u32::try_from(file_data.len()).expect("file size fits in u32"),
        )
        .rendering_type(rendering_type);

        if let Some((thumb_id, thumb)) = thumbnail_blob_id.zip(thumbnail.as_ref()) {
            builder = builder.thumbnail(thumb_id, &thumb.media_type);
        }

        if let Some(thumb) = &thumbnail {
            builder = builder.dimensions(thumb.original_height, thumb.original_width);
        }

        if let Some(name) = file_name {
            builder = builder.file_name(name);
        }

        if let Some(desc) = caption {
            builder = builder.description(desc);
        }

        let file_msg = builder.build()?;

        // Encrypt and send
        let encrypted_msg = self
            .e2e_api
            .encode_and_encrypt(&file_msg.into(), recipient_key)
            .map_err(SendError::Encrypt)?;

        let message_id = self
            .e2e_api
            .send(to, &encrypted_msg, false)
            .await
            .map_err(SendError::Send)?;

        tracing::info!(
            "Sent file to {}: {} ({} bytes), message_id: {}",
            to,
            media_type,
            file_data.len(),
            message_id
        );

        Ok(message_id)
    }
}

/// Map a public key lookup error to a [`SendError`].
fn pubkey_lookup_err(
    identity: ThreemaId,
    err: ApiOrCacheError<InMemoryPublicKeyCacheError>,
) -> SendError {
    let identity = identity.to_string();
    match err {
        ApiOrCacheError::ApiError(source) => SendError::PublicKeyLookup { identity, source },
        ApiOrCacheError::CacheError(source) => SendError::PublicKeyCache { identity, source },
    }
}

/// Generated thumbnail data with its media type and the original image dimensions.
struct Thumbnail {
    data: Vec<u8>,
    media_type: String,
    original_width: u32,
    original_height: u32,
}

/// Generate a thumbnail from image data.
///
/// Takes any supported image format and produces a small thumbnail
/// suitable for preview in Threema clients. If `png` is true, the
/// thumbnail is encoded as PNG; otherwise as JPEG.
fn generate_thumbnail(image_data: &[u8], png: bool) -> Result<Thumbnail, image::ImageError> {
    // Decode the image
    let img = ImageReader::new(Cursor::new(image_data))
        .with_guessed_format()?
        .decode()?;

    // Extract dimensions
    let original_width = img.width();
    let original_height = img.height();

    // Resize to thumbnail size (max 256x256, preserving aspect ratio)
    let thumbnail = img.thumbnail(256, 256);

    // Encode
    let mut bytes: Vec<u8> = Vec::new();
    if png {
        let encoder = PngEncoder::new(&mut bytes);
        encoder.write_image(
            thumbnail.as_bytes(),
            thumbnail.width(),
            thumbnail.height(),
            thumbnail.color().into(),
        )?;
        Ok(Thumbnail {
            data: bytes,
            media_type: "image/png".into(),
            original_width,
            original_height,
        })
    } else {
        let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 80);
        encoder.encode_image(&thumbnail)?;
        Ok(Thumbnail {
            data: bytes,
            media_type: "image/jpeg".into(),
            original_width,
            original_height,
        })
    }
}
