//! PermissionsService RPC implementations.

use std::collections::HashMap;

use futures_core::Stream;
use tokio_stream::StreamExt;

use crate::error::Error;
use crate::proto;
use crate::types::context::context_to_struct;
use crate::types::*;

use super::Client;

// ── CheckPermission ──────────────────────────────────────────────

/// Builder for a CheckPermission request.
pub struct CheckPermissionRequest<'a> {
    client: &'a Client,
    resource: proto::ObjectReference,
    permission: String,
    subject: proto::SubjectReference,
    consistency: Option<proto::Consistency>,
    context: Option<prost_types::Struct>,
}

impl<'a> CheckPermissionRequest<'a> {
    /// Sets the consistency mode for this request.
    pub fn consistency(mut self, c: Consistency) -> Self {
        self.consistency = Some((&c).into());
        self
    }

    /// Sets the caveat evaluation context for this request.
    pub fn context(mut self, ctx: HashMap<String, ContextValue>) -> Self {
        self.context = Some(context_to_struct(&ctx));
        self
    }
}

impl<'a> std::future::IntoFuture for CheckPermissionRequest<'a> {
    type Output = Result<PermissionResult, Error>;
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let req = proto::CheckPermissionRequest {
                consistency: self.consistency,
                resource: Some(self.resource),
                permission: self.permission,
                subject: Some(self.subject),
                context: self.context,
                with_tracing: false,
            };

            let response = self
                .client
                .permissions
                .clone()
                .check_permission(req)
                .await
                .map_err(Error::from_status)?;

            let inner = response.into_inner();
            PermissionResult::from_check_response(inner.permissionship, inner.partial_caveat_info)
        })
    }
}

// ── WriteRelationships ──────────────────────────────────────────

/// Builder for a WriteRelationships request.
pub struct WriteRelationshipsRequest<'a> {
    client: &'a Client,
    updates: Vec<proto::RelationshipUpdate>,
    preconditions: Vec<proto::Precondition>,
}

impl<'a> WriteRelationshipsRequest<'a> {
    /// Adds preconditions that must be satisfied before the write commits.
    pub fn preconditions(mut self, preconditions: Vec<Precondition>) -> Self {
        self.preconditions = preconditions.iter().map(Into::into).collect();
        self
    }
}

impl<'a> std::future::IntoFuture for WriteRelationshipsRequest<'a> {
    type Output = Result<ZedToken, Error>;
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            if self.updates.is_empty() {
                return Err(Error::InvalidArgument("updates must not be empty".into()));
            }

            let req = proto::WriteRelationshipsRequest {
                updates: self.updates,
                optional_preconditions: self.preconditions,
                optional_transaction_metadata: None,
            };

            let response = self
                .client
                .permissions
                .clone()
                .write_relationships(req)
                .await
                .map_err(Error::from_status)?;

            let inner = response.into_inner();
            inner
                .written_at
                .ok_or_else(|| Error::Serialization("missing written_at token".into()))?
                .try_into()
        })
    }
}

// ── DeleteRelationships ──────────────────────────────────────────

/// Builder for a DeleteRelationships request.
pub struct DeleteRelationshipsRequest<'a> {
    client: &'a Client,
    filter: proto::RelationshipFilter,
    preconditions: Vec<proto::Precondition>,
}

impl<'a> DeleteRelationshipsRequest<'a> {
    /// Adds preconditions that must be satisfied before the delete commits.
    pub fn preconditions(mut self, preconditions: Vec<Precondition>) -> Self {
        self.preconditions = preconditions.iter().map(Into::into).collect();
        self
    }
}

impl<'a> std::future::IntoFuture for DeleteRelationshipsRequest<'a> {
    type Output = Result<ZedToken, Error>;
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let req = proto::DeleteRelationshipsRequest {
                relationship_filter: Some(self.filter),
                optional_preconditions: self.preconditions,
                optional_limit: 0,
                optional_allow_partial_deletions: false,
                optional_transaction_metadata: None,
            };

            let response = self
                .client
                .permissions
                .clone()
                .delete_relationships(req)
                .await
                .map_err(Error::from_status)?;

            let inner = response.into_inner();
            inner
                .deleted_at
                .ok_or_else(|| Error::Serialization("missing deleted_at token".into()))?
                .try_into()
        })
    }
}

// ── LookupResources ──────────────────────────────────────────────

/// Builder for a LookupResources streaming request.
pub struct LookupResourcesRequest<'a> {
    client: &'a Client,
    resource_type: String,
    permission: String,
    subject: proto::SubjectReference,
    consistency: Option<proto::Consistency>,
    context: Option<prost_types::Struct>,
}

impl<'a> LookupResourcesRequest<'a> {
    /// Sets the consistency mode.
    pub fn consistency(mut self, c: Consistency) -> Self {
        self.consistency = Some((&c).into());
        self
    }

    /// Sets the caveat evaluation context.
    pub fn context(mut self, ctx: HashMap<String, ContextValue>) -> Self {
        self.context = Some(context_to_struct(&ctx));
        self
    }

    /// Sends the request and returns a stream of results.
    pub async fn send(
        self,
    ) -> Result<impl Stream<Item = Result<LookupResourceResult, Error>>, Error> {
        let req = proto::LookupResourcesRequest {
            consistency: self.consistency,
            resource_object_type: self.resource_type,
            permission: self.permission,
            subject: Some(self.subject),
            context: self.context,
            optional_limit: 0,
            optional_cursor: None,
        };

        let response = self
            .client
            .permissions
            .clone()
            .lookup_resources(req)
            .await
            .map_err(Error::from_status)?;

        Ok(response.into_inner().map(|r| match r {
            Ok(proto) => LookupResourceResult::from_proto(proto),
            Err(status) => Err(Error::from_status(status)),
        }))
    }
}

// ── LookupSubjects ──────────────────────────────────────────────

/// Builder for a LookupSubjects streaming request.
pub struct LookupSubjectsRequest<'a> {
    client: &'a Client,
    resource: proto::ObjectReference,
    permission: String,
    subject_type: String,
    optional_subject_relation: String,
    consistency: Option<proto::Consistency>,
    context: Option<prost_types::Struct>,
}

impl<'a> LookupSubjectsRequest<'a> {
    /// Sets the consistency mode.
    pub fn consistency(mut self, c: Consistency) -> Self {
        self.consistency = Some((&c).into());
        self
    }

    /// Sets the caveat evaluation context.
    pub fn context(mut self, ctx: HashMap<String, ContextValue>) -> Self {
        self.context = Some(context_to_struct(&ctx));
        self
    }

    /// Sends the request and returns a stream of results.
    pub async fn send(
        self,
    ) -> Result<impl Stream<Item = Result<LookupSubjectResult, Error>>, Error> {
        let req = proto::LookupSubjectsRequest {
            consistency: self.consistency,
            resource: Some(self.resource),
            permission: self.permission,
            subject_object_type: self.subject_type,
            optional_subject_relation: self.optional_subject_relation,
            context: self.context,
            optional_concrete_limit: 0,
            optional_cursor: None,
            wildcard_option: 0,
        };

        let response = self
            .client
            .permissions
            .clone()
            .lookup_subjects(req)
            .await
            .map_err(Error::from_status)?;

        Ok(response.into_inner().map(|r| match r {
            Ok(proto) => LookupSubjectResult::from_proto(proto),
            Err(status) => Err(Error::from_status(status)),
        }))
    }
}

// ── ReadRelationships ──────────────────────────────────────────────

/// Builder for a ReadRelationships streaming request.
pub struct ReadRelationshipsRequest<'a> {
    client: &'a Client,
    filter: proto::RelationshipFilter,
    consistency: Option<proto::Consistency>,
}

impl<'a> ReadRelationshipsRequest<'a> {
    /// Sets the consistency mode.
    pub fn consistency(mut self, c: Consistency) -> Self {
        self.consistency = Some((&c).into());
        self
    }

    /// Sends the request and returns a stream of results.
    pub async fn send(
        self,
    ) -> Result<impl Stream<Item = Result<ReadRelationshipResult, Error>>, Error> {
        let req = proto::ReadRelationshipsRequest {
            consistency: self.consistency,
            relationship_filter: Some(self.filter),
            optional_limit: 0,
            optional_cursor: None,
        };

        let response = self
            .client
            .permissions
            .clone()
            .read_relationships(req)
            .await
            .map_err(Error::from_status)?;

        Ok(response.into_inner().map(|r| match r {
            Ok(proto) => ReadRelationshipResult::from_proto(proto),
            Err(status) => Err(Error::from_status(status)),
        }))
    }
}

// ── ExpandPermissionTree ──────────────────────────────────────────────

/// Builder for an ExpandPermissionTree request.
pub struct ExpandPermissionTreeRequest<'a> {
    client: &'a Client,
    resource: proto::ObjectReference,
    permission: String,
    consistency: Option<proto::Consistency>,
}

impl<'a> ExpandPermissionTreeRequest<'a> {
    /// Sets the consistency mode.
    pub fn consistency(mut self, c: Consistency) -> Self {
        self.consistency = Some((&c).into());
        self
    }
}

impl<'a> std::future::IntoFuture for ExpandPermissionTreeRequest<'a> {
    type Output = Result<PermissionTree, Error>;
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let req = proto::ExpandPermissionTreeRequest {
                consistency: self.consistency,
                resource: Some(self.resource),
                permission: self.permission,
            };

            let response = self
                .client
                .permissions
                .clone()
                .expand_permission_tree(req)
                .await
                .map_err(Error::from_status)?;

            let inner = response.into_inner();
            let tree = inner
                .tree_root
                .ok_or_else(|| Error::Serialization("missing tree_root".into()))?;
            PermissionTree::from_proto(tree)
        })
    }
}

// ── Client methods ──────────────────────────────────────────────

impl Client {
    /// Checks whether a subject has a permission on a resource.
    ///
    /// Returns a [`PermissionResult`] with three possible states.
    /// Use `.consistency()` and `.context()` on the returned builder.
    pub fn check_permission(
        &self,
        resource: &ObjectReference,
        permission: impl Into<String>,
        subject: &SubjectReference,
    ) -> CheckPermissionRequest<'_> {
        CheckPermissionRequest {
            client: self,
            resource: resource.into(),
            permission: permission.into(),
            subject: subject.into(),
            consistency: None,
            context: None,
        }
    }

    /// Writes a batch of relationship updates atomically.
    ///
    /// Returns `Err(InvalidArgument)` if `updates` is empty.
    pub fn write_relationships(
        &self,
        updates: Vec<RelationshipUpdate>,
    ) -> WriteRelationshipsRequest<'_> {
        // FR-10.1: empty vec validation is checked in IntoFuture
        WriteRelationshipsRequest {
            client: self,
            updates: updates.iter().map(Into::into).collect(),
            preconditions: vec![],
        }
    }

    /// Deletes all relationships matching the given filter.
    pub fn delete_relationships(
        &self,
        filter: RelationshipFilter,
    ) -> DeleteRelationshipsRequest<'_> {
        DeleteRelationshipsRequest {
            client: self,
            filter: (&filter).into(),
            preconditions: vec![],
        }
    }

    /// Looks up all resources of a given type that a subject can access.
    ///
    /// Returns a streaming builder. Call `.send().await?` to get the stream.
    pub fn lookup_resources(
        &self,
        resource_type: impl Into<String>,
        permission: impl Into<String>,
        subject: &SubjectReference,
    ) -> LookupResourcesRequest<'_> {
        LookupResourcesRequest {
            client: self,
            resource_type: resource_type.into(),
            permission: permission.into(),
            subject: subject.into(),
            consistency: None,
            context: None,
        }
    }

    /// Looks up all subjects of a given type that have access to a resource.
    ///
    /// Returns a streaming builder. Call `.send().await?` to get the stream.
    pub fn lookup_subjects(
        &self,
        resource: &ObjectReference,
        permission: impl Into<String>,
        subject_type: impl Into<String>,
    ) -> LookupSubjectsRequest<'_> {
        LookupSubjectsRequest {
            client: self,
            resource: resource.into(),
            permission: permission.into(),
            subject_type: subject_type.into(),
            optional_subject_relation: String::new(),
            consistency: None,
            context: None,
        }
    }

    /// Reads relationships matching the given filter.
    ///
    /// Returns a streaming builder. Call `.send().await?` to get the stream.
    pub fn read_relationships(&self, filter: RelationshipFilter) -> ReadRelationshipsRequest<'_> {
        ReadRelationshipsRequest {
            client: self,
            filter: (&filter).into(),
            consistency: None,
        }
    }

    /// Expands the permission tree for a resource and permission.
    pub fn expand_permission_tree(
        &self,
        resource: &ObjectReference,
        permission: impl Into<String>,
    ) -> ExpandPermissionTreeRequest<'_> {
        ExpandPermissionTreeRequest {
            client: self,
            resource: resource.into(),
            permission: permission.into(),
            consistency: None,
        }
    }
}
