//! Client builder for configuring connections.

use std::time::Duration;

use tonic::metadata::MetadataValue;
use tonic::transport::Endpoint;

use crate::error::Error;

use super::{BearerTokenInterceptor, Client};
use crate::proto::permissions_service_client::PermissionsServiceClient;
use crate::proto::schema_service_client::SchemaServiceClient;
#[cfg(feature = "watch")]
use crate::proto::watch_service_client::WatchServiceClient;

/// A builder for configuring and creating a [`Client`].
///
/// # Examples
///
/// ```rust,no_run
/// use std::time::Duration;
/// use prescience::Client;
///
/// # async fn example() -> Result<(), prescience::Error> {
/// let client = Client::builder("https://spicedb.prod.internal:50051", "my-token")
///     .connect_timeout(Duration::from_secs(5))
///     .default_timeout(Duration::from_secs(10))
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct ClientBuilder {
    endpoint: String,
    token: String,
    insecure: bool,
    connect_timeout: Option<Duration>,
    default_timeout: Option<Duration>,
}

impl ClientBuilder {
    pub(crate) fn new(endpoint: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            token: token.into(),
            insecure: false,
            connect_timeout: None,
            default_timeout: None,
        }
    }

    /// Allow insecure (plaintext) connections to non-loopback addresses.
    ///
    /// By default, `http://` to a non-loopback address returns an error.
    /// Set this to `true` to allow it.
    pub fn insecure(mut self, insecure: bool) -> Self {
        self.insecure = insecure;
        self
    }

    /// Sets the connection timeout.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Sets a default timeout applied to all RPCs unless overridden per-request.
    pub fn default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
        self
    }

    /// Builds and connects the client.
    pub async fn build(self) -> Result<Client, Error> {
        // Validate insecure connections (FR-1.3)
        // Parse the URI to extract the host for proper loopback validation.
        // Substring matching (e.g. contains("localhost")) is unsafe because
        // it would allow hosts like "localhost.evil.com".
        if self.endpoint.starts_with("http://") && !self.insecure {
            let uri: http::Uri = self
                .endpoint
                .parse()
                .map_err(|e: http::uri::InvalidUri| {
                    Error::InvalidArgument(format!("invalid endpoint URI: {}", e))
                })?;
            let host = uri.host().unwrap_or("");
            let is_loopback = host == "localhost"
                || host == "127.0.0.1"
                || host == "::1"
                || host == "[::1]";

            if !is_loopback {
                return Err(Error::InvalidArgument(
                    format!(
                        "insecure connection to non-loopback address '{}' requires \
                         .insecure(true) on the builder. Use https:// for production.",
                        self.endpoint
                    ),
                ));
            }
        }

        let mut endpoint = Endpoint::from_shared(self.endpoint.clone())
            .map_err(|e| Error::InvalidArgument(format!("invalid endpoint: {}", e)))?;

        if let Some(timeout) = self.connect_timeout {
            endpoint = endpoint.connect_timeout(timeout);
        }

        if let Some(timeout) = self.default_timeout {
            endpoint = endpoint.timeout(timeout);
        }

        let channel = endpoint.connect().await?;

        let header_value = format!("Bearer {}", self.token);
        let meta_value: MetadataValue<tonic::metadata::Ascii> = header_value
            .parse()
            .map_err(|_| Error::InvalidArgument("invalid bearer token".into()))?;
        let interceptor = BearerTokenInterceptor { token: meta_value };

        let permissions =
            PermissionsServiceClient::with_interceptor(channel.clone(), interceptor.clone());
        let schema = SchemaServiceClient::with_interceptor(channel.clone(), interceptor.clone());
        #[cfg(feature = "watch")]
        let watch = WatchServiceClient::with_interceptor(channel, interceptor);

        Ok(Client {
            permissions,
            schema,
            #[cfg(feature = "watch")]
            watch,
            default_timeout: self.default_timeout,
        })
    }
}
