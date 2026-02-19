//! Experimental/Bulk API implementations (behind `experimental` feature).
//!
//! These wrap the bulk RPCs that have been promoted to PermissionsService
//! in the SpiceDB API but are feature-gated in this library since they
//! may still evolve.

use std::collections::HashMap;

use futures_core::Stream;
use tokio_stream::StreamExt;

use crate::error::Error;
use crate::proto;
use crate::types::context::context_to_struct;
use crate::types::*;

use super::Client;

// ── BulkCheckItem ──────────────────────────────────────────────

/// A single item in a bulk check request.
#[derive(Debug, Clone)]
pub struct BulkCheckItem {
    /// The resource to check.
    pub resource: ObjectReference,
    /// The permission to check.
    pub permission: String,
    /// The subject to check.
    pub subject: SubjectReference,
    /// Optional caveat context.
    pub context: Option<HashMap<String, ContextValue>>,
}

impl BulkCheckItem {
    /// Creates a new bulk check item.
    pub fn new(
        resource: ObjectReference,
        permission: impl Into<String>,
        subject: SubjectReference,
    ) -> Self {
        Self {
            resource,
            permission: permission.into(),
            subject,
            context: None,
        }
    }

    /// Sets caveat context for this check item.
    pub fn with_context(mut self, context: HashMap<String, ContextValue>) -> Self {
        self.context = Some(context);
        self
    }
}

// ── BulkCheckPermissions ──────────────────────────────────────────

/// Builder for a BulkCheckPermissions request.
pub struct BulkCheckPermissionsRequest<'a> {
    client: &'a Client,
    items: Vec<proto::CheckBulkPermissionsRequestItem>,
    consistency: Option<proto::Consistency>,
}

impl<'a> BulkCheckPermissionsRequest<'a> {
    /// Sets the consistency mode.
    pub fn consistency(mut self, c: Consistency) -> Self {
        self.consistency = Some((&c).into());
        self
    }
}

impl<'a> std::future::IntoFuture for BulkCheckPermissionsRequest<'a> {
    type Output = Result<Vec<CheckResult>, Error>;
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let req = proto::CheckBulkPermissionsRequest {
                consistency: self.consistency,
                items: self.items,
                with_tracing: false,
            };

            let response = self
                .client
                .permissions
                .clone()
                .check_bulk_permissions(req)
                .await
                .map_err(Error::from_status)?;

            let inner = response.into_inner();
            let results: Vec<CheckResult> = inner
                .pairs
                .into_iter()
                .map(|pair| match pair.response {
                    Some(proto::check_bulk_permissions_pair::Response::Item(item)) => {
                        PermissionResult::from_check_response(
                            item.permissionship,
                            item.partial_caveat_info,
                        )
                    }
                    Some(proto::check_bulk_permissions_pair::Response::Error(status)) => {
                        Err(Error::Status {
                            code: tonic::Code::from_i32(status.code),
                            message: status.message,
                            details: None,
                        })
                    }
                    None => Err(Error::Serialization(
                        "missing response in bulk check pair".into(),
                    )),
                })
                .collect();
            Ok(results)
        })
    }
}

// ── BulkImportRelationships ──────────────────────────────────────────

/// Builder for a BulkImportRelationships request.
pub struct BulkImportRelationshipsRequest<'a, S> {
    client: &'a Client,
    stream: S,
}

impl<'a, S> BulkImportRelationshipsRequest<'a, S>
where
    S: Stream<Item = Relationship> + Send + 'static,
{
    /// Sends the client-streaming import request and returns the number of relationships loaded.
    pub async fn send(self) -> Result<u64, Error> {
        // Each ImportBulkRelationshipsRequest can contain multiple relationships.
        // We send one relationship per message for simplicity; SpiceDB handles batching internally.
        let request_stream = self.stream.map(|rel: Relationship| {
            let proto_rel: proto::Relationship = (&rel).into();
            proto::ImportBulkRelationshipsRequest {
                relationships: vec![proto_rel],
            }
        });

        let response = self
            .client
            .permissions
            .clone()
            .import_bulk_relationships(request_stream)
            .await
            .map_err(Error::from_status)?;

        Ok(response.into_inner().num_loaded)
    }
}

// ── BulkExportRelationships ──────────────────────────────────────────

/// Builder for a BulkExportRelationships streaming request.
pub struct BulkExportRelationshipsRequest<'a> {
    client: &'a Client,
    filter: Option<proto::RelationshipFilter>,
    consistency: Option<proto::Consistency>,
}

impl<'a> BulkExportRelationshipsRequest<'a> {
    /// Sets the consistency mode.
    pub fn consistency(mut self, c: Consistency) -> Self {
        self.consistency = Some((&c).into());
        self
    }

    /// Sends the request and returns a stream of relationships.
    pub async fn send(
        self,
    ) -> Result<
        impl Stream<Item = Result<Relationship, Error>>,
        Error,
    > {
        let req = proto::ExportBulkRelationshipsRequest {
            consistency: self.consistency,
            optional_limit: 0,
            optional_cursor: None,
            optional_relationship_filter: self.filter,
        };

        let response = self
            .client
            .permissions
            .clone()
            .export_bulk_relationships(req)
            .await
            .map_err(Error::from_status)?;

        // Each response batch contains multiple relationships; flatten into individual items.
        let inner = response.into_inner();
        let stream = async_stream::try_stream! {
            let mut inner = inner;
            while let Some(result) = inner.next().await {
                match result {
                    Ok(batch) => {
                        for rel in batch.relationships {
                            let r: Relationship = rel.try_into()?;
                            yield r;
                        }
                    }
                    Err(status) => {
                        Err(Error::from_status(status))?;
                    }
                }
            }
        };
        Ok(stream)
    }
}

// ── Client methods ──────────────────────────────────────────────

impl Client {
    /// Checks permissions for a batch of items in a single round-trip.
    ///
    /// Returns a `Vec<CheckResult>` where each item is either a
    /// `PermissionResult` or a per-item `Error`.
    pub fn bulk_check_permissions(
        &self,
        items: Vec<BulkCheckItem>,
    ) -> BulkCheckPermissionsRequest<'_> {
        let proto_items = items
            .into_iter()
            .map(|item| proto::CheckBulkPermissionsRequestItem {
                resource: Some((&item.resource).into()),
                permission: item.permission,
                subject: Some((&item.subject).into()),
                context: item.context.as_ref().map(context_to_struct),
            })
            .collect();

        BulkCheckPermissionsRequest {
            client: self,
            items: proto_items,
            consistency: None,
        }
    }

    /// Bulk imports relationships via client-streaming.
    ///
    /// Accepts any `Stream<Item = Relationship>`. Returns the number of
    /// relationships loaded.
    pub fn bulk_import_relationships<S>(
        &self,
        stream: S,
    ) -> BulkImportRelationshipsRequest<'_, S>
    where
        S: Stream<Item = Relationship> + Send + 'static,
    {
        BulkImportRelationshipsRequest {
            client: self,
            stream,
        }
    }

    /// Bulk exports relationships via server-streaming.
    ///
    /// Returns a streaming builder. Call `.send().await?` to get the stream.
    pub fn bulk_export_relationships(
        &self,
        filter: RelationshipFilter,
    ) -> BulkExportRelationshipsRequest<'_> {
        BulkExportRelationshipsRequest {
            client: self,
            filter: Some((&filter).into()),
            consistency: None,
        }
    }
}
