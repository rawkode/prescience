//! SpiceDB client implementation.

mod builder;
mod permissions;
mod schema;
#[cfg(feature = "watch")]
mod watch;

use std::time::Duration;

use tonic::metadata::MetadataValue;
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::Channel;

pub use builder::ClientBuilder;

use crate::proto::permissions_service_client::PermissionsServiceClient;
use crate::proto::schema_service_client::SchemaServiceClient;
#[cfg(feature = "watch")]
use crate::proto::watch_service_client::WatchServiceClient;

/// Bearer token interceptor that attaches auth to every request.
#[derive(Clone)]
struct BearerTokenInterceptor {
    token: MetadataValue<tonic::metadata::Ascii>,
}

impl Interceptor for BearerTokenInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        request
            .metadata_mut()
            .insert("authorization", self.token.clone());
        Ok(request)
    }
}

type AuthChannel = InterceptedService<Channel, BearerTokenInterceptor>;

/// An idiomatic Rust client for SpiceDB.
///
/// `Client` is cheap to clone — it wraps a `tonic::Channel` which is
/// reference-counted internally. Clone it freely to share across tasks.
///
/// # Examples
///
/// ```rust,no_run
/// use prescience::Client;
///
/// # async fn example() -> Result<(), prescience::Error> {
/// let client = Client::new("http://localhost:50051", "my-token").await?;
///
/// // Clone is cheap — share across tasks
/// let client2 = client.clone();
/// tokio::spawn(async move {
///     // use client2
/// });
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    permissions: PermissionsServiceClient<AuthChannel>,
    schema: SchemaServiceClient<AuthChannel>,
    #[cfg(feature = "watch")]
    watch: WatchServiceClient<AuthChannel>,
    default_timeout: Option<Duration>,
}

impl Client {
    /// Creates a new client connected to the given SpiceDB endpoint.
    ///
    /// For `http://` endpoints, only loopback addresses are allowed unless
    /// you use [`Client::builder`] with `.insecure(true)`.
    pub async fn new(
        endpoint: impl Into<String>,
        token: impl Into<String>,
    ) -> Result<Self, crate::Error> {
        ClientBuilder::new(endpoint, token).build().await
    }

    /// Creates a builder for configuring a client connection.
    pub fn builder(
        endpoint: impl Into<String>,
        token: impl Into<String>,
    ) -> ClientBuilder {
        ClientBuilder::new(endpoint, token)
    }

    /// Creates a client from a pre-built tonic `Channel`.
    ///
    /// Use this for advanced TLS configurations (custom CA certs,
    /// client certificates, mTLS, etc.).
    pub fn from_channel(channel: Channel, token: impl Into<String>) -> Result<Self, crate::Error> {
        let token_str = token.into();
        let header_value = format!("Bearer {}", token_str);
        let meta_value: MetadataValue<tonic::metadata::Ascii> = header_value
            .parse()
            .map_err(|_| crate::Error::InvalidArgument("invalid bearer token".into()))?;
        let interceptor = BearerTokenInterceptor { token: meta_value };

        let permissions =
            PermissionsServiceClient::with_interceptor(channel.clone(), interceptor.clone());
        let schema = SchemaServiceClient::with_interceptor(channel.clone(), interceptor.clone());
        #[cfg(feature = "watch")]
        let watch = WatchServiceClient::with_interceptor(channel, interceptor);

        Ok(Self {
            permissions,
            schema,
            #[cfg(feature = "watch")]
            watch,
            default_timeout: None,
        })
    }

    /// Returns the default timeout applied to RPCs, if set.
    pub fn default_timeout(&self) -> Option<Duration> {
        self.default_timeout
    }
}

// Compile-time assertions for FR-1.7: Client must be Clone + Send + Sync
#[cfg(test)]
mod trait_tests {
    use super::*;
    fn _assert_clone<T: Clone>() {}
    fn _assert_send<T: Send>() {}
    fn _assert_sync<T: Sync>() {}
    fn _assert_all() {
        _assert_clone::<Client>();
        _assert_send::<Client>();
        _assert_sync::<Client>();
    }
}
