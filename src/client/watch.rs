//! WatchService RPC implementation (behind `watch` feature).

use futures_core::Stream;
use tokio_stream::StreamExt;

use crate::error::Error;
use crate::proto;
use crate::types::{WatchEvent, ZedToken};

use super::Client;

/// Builder for a Watch streaming request.
pub struct WatchRequest<'a> {
    client: &'a Client,
    object_types: Vec<String>,
    start_cursor: Option<proto::ZedToken>,
}

impl<'a> WatchRequest<'a> {
    /// Resume watching from a specific token (e.g., from a previous WatchEvent checkpoint).
    pub fn after_token(mut self, token: ZedToken) -> Self {
        self.start_cursor = Some((&token).into());
        self
    }

    /// Sends the request and returns a long-lived stream of watch events.
    ///
    /// The stream does NOT auto-reconnect. On server disconnect, it yields
    /// `Err(Error::Status { code: UNAVAILABLE, .. })` then terminates.
    /// Use the checkpoint `ZedToken` from the last `WatchEvent` to resume.
    pub async fn send(self) -> Result<impl Stream<Item = Result<WatchEvent, Error>>, Error> {
        let req = proto::WatchRequest {
            optional_object_types: self.object_types,
            optional_start_cursor: self.start_cursor,
            optional_relationship_filters: vec![],
            optional_update_kinds: vec![],
        };

        let response = self
            .client
            .watch
            .clone()
            .watch(req)
            .await
            .map_err(Error::from_status)?;

        Ok(response.into_inner().map(|r| match r {
            Ok(proto) => WatchEvent::from_proto(proto),
            Err(status) => Err(Error::from_status(status)),
        }))
    }
}

impl Client {
    /// Watches for relationship changes, optionally filtered by object types.
    ///
    /// Pass an empty vec to watch all types. Returns a streaming builder —
    /// call `.send().await?` to get the stream.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use prescience::Client;
    /// # async fn example(client: &Client) -> Result<(), prescience::Error> {
    /// use tokio_stream::StreamExt;
    ///
    /// let mut stream = client
    ///     .watch(vec!["document", "user"])
    ///     .send()
    ///     .await?;
    ///
    /// while let Some(event) = stream.next().await {
    ///     let watch_event = event?;
    ///     println!("{} updates, checkpoint: {}", watch_event.updates.len(), watch_event.checkpoint);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn watch(&self, object_types: Vec<impl Into<String>>) -> WatchRequest<'_> {
        WatchRequest {
            client: self,
            object_types: object_types.into_iter().map(Into::into).collect(),
            start_cursor: None,
        }
    }
}
