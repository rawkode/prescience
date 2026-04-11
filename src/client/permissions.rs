//! PermissionsService RPC implementations.

use std::collections::HashMap;
use std::time::Duration;

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
    with_tracing: bool,
    timeout: Option<Duration>,
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

    /// Enables request-level tracing metadata in the response.
    pub fn with_tracing(mut self, enabled: bool) -> Self {
        self.with_tracing = enabled;
        self
    }

    /// Sets a per-request timeout, overriding the client default for this request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

impl<'a> std::future::IntoFuture for CheckPermissionRequest<'a> {
    type Output = Result<PermissionResult, Error>;
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            if self.permission.trim().is_empty() {
                return Err(Error::InvalidArgument(
                    "permission must not be empty".into(),
                ));
            }

            let proto_req = proto::CheckPermissionRequest {
                consistency: self.consistency,
                resource: Some(self.resource),
                permission: self.permission,
                subject: Some(self.subject),
                context: self.context,
                with_tracing: self.with_tracing,
            };

            let mut req = tonic::Request::new(proto_req);
            if let Some(t) = self.timeout {
                req.set_timeout(t);
            }

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
    timeout: Option<Duration>,
}

impl<'a> WriteRelationshipsRequest<'a> {
    /// Adds preconditions that must be satisfied before the write commits.
    pub fn preconditions(mut self, preconditions: Vec<Precondition>) -> Self {
        self.preconditions = preconditions.iter().map(Into::into).collect();
        self
    }

    /// Sets a per-request timeout, overriding the client default for this request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
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

            let proto_req = proto::WriteRelationshipsRequest {
                updates: self.updates,
                optional_preconditions: self.preconditions,
                optional_transaction_metadata: None,
            };

            let mut req = tonic::Request::new(proto_req);
            if let Some(t) = self.timeout {
                req.set_timeout(t);
            }

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
    timeout: Option<Duration>,
}

impl<'a> DeleteRelationshipsRequest<'a> {
    /// Adds preconditions that must be satisfied before the delete commits.
    pub fn preconditions(mut self, preconditions: Vec<Precondition>) -> Self {
        self.preconditions = preconditions.iter().map(Into::into).collect();
        self
    }

    /// Sets a per-request timeout, overriding the client default for this request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

impl<'a> std::future::IntoFuture for DeleteRelationshipsRequest<'a> {
    type Output = Result<ZedToken, Error>;
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let proto_req = proto::DeleteRelationshipsRequest {
                relationship_filter: Some(self.filter),
                optional_preconditions: self.preconditions,
                optional_limit: 0,
                optional_allow_partial_deletions: false,
                optional_transaction_metadata: None,
            };

            let mut req = tonic::Request::new(proto_req);
            if let Some(t) = self.timeout {
                req.set_timeout(t);
            }

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
    timeout: Option<Duration>,
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

    /// Sets a per-request timeout, overriding the client default for this request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sends the request and returns a stream of results.
    pub async fn send(
        self,
    ) -> Result<impl Stream<Item = Result<LookupResourceResult, Error>>, Error> {
        if self.resource_type.trim().is_empty() {
            return Err(Error::InvalidArgument(
                "resource_type must not be empty".into(),
            ));
        }
        if self.permission.trim().is_empty() {
            return Err(Error::InvalidArgument(
                "permission must not be empty".into(),
            ));
        }

        let proto_req = proto::LookupResourcesRequest {
            consistency: self.consistency,
            resource_object_type: self.resource_type,
            permission: self.permission,
            subject: Some(self.subject),
            context: self.context,
            optional_limit: 0,
            optional_cursor: None,
        };

        let mut req = tonic::Request::new(proto_req);
        if let Some(t) = self.timeout {
            req.set_timeout(t);
        }

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
    timeout: Option<Duration>,
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

    /// Sets a per-request timeout, overriding the client default for this request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sends the request and returns a stream of results.
    pub async fn send(
        self,
    ) -> Result<impl Stream<Item = Result<LookupSubjectResult, Error>>, Error> {
        if self.permission.trim().is_empty() {
            return Err(Error::InvalidArgument(
                "permission must not be empty".into(),
            ));
        }
        if self.subject_type.trim().is_empty() {
            return Err(Error::InvalidArgument(
                "subject_type must not be empty".into(),
            ));
        }

        let proto_req = proto::LookupSubjectsRequest {
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

        let mut req = tonic::Request::new(proto_req);
        if let Some(t) = self.timeout {
            req.set_timeout(t);
        }

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
    timeout: Option<Duration>,
}

impl<'a> ReadRelationshipsRequest<'a> {
    /// Sets the consistency mode.
    pub fn consistency(mut self, c: Consistency) -> Self {
        self.consistency = Some((&c).into());
        self
    }

    /// Sets a per-request timeout, overriding the client default for this request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sends the request and returns a stream of results.
    pub async fn send(
        self,
    ) -> Result<impl Stream<Item = Result<ReadRelationshipResult, Error>>, Error> {
        let proto_req = proto::ReadRelationshipsRequest {
            consistency: self.consistency,
            relationship_filter: Some(self.filter),
            optional_limit: 0,
            optional_cursor: None,
        };

        let mut req = tonic::Request::new(proto_req);
        if let Some(t) = self.timeout {
            req.set_timeout(t);
        }

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
    timeout: Option<Duration>,
}

impl<'a> ExpandPermissionTreeRequest<'a> {
    /// Sets the consistency mode.
    pub fn consistency(mut self, c: Consistency) -> Self {
        self.consistency = Some((&c).into());
        self
    }

    /// Sets a per-request timeout, overriding the client default for this request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

impl<'a> std::future::IntoFuture for ExpandPermissionTreeRequest<'a> {
    type Output = Result<PermissionTree, Error>;
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            if self.permission.trim().is_empty() {
                return Err(Error::InvalidArgument(
                    "permission must not be empty".into(),
                ));
            }

            let proto_req = proto::ExpandPermissionTreeRequest {
                consistency: self.consistency,
                resource: Some(self.resource),
                permission: self.permission,
            };

            let mut req = tonic::Request::new(proto_req);
            if let Some(t) = self.timeout {
                req.set_timeout(t);
            }

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
            with_tracing: false,
            timeout: None,
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
            timeout: None,
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
            timeout: None,
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
            timeout: None,
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
            timeout: None,
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
            timeout: None,
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
            timeout: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Client, ObjectReference, SubjectReference};

    /// Creates a throwaway `Client` pointed at a dummy address using a lazy
    /// channel that does not connect until the first RPC attempt.
    fn dummy_client() -> Client {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1")
            .connect_lazy();
        Client::from_channel(channel, "test-token").expect("client construction failed")
    }

    #[tokio::test]
    async fn check_permission_empty_permission_rejected() {
        let c = dummy_client();
        let resource = ObjectReference::new("document", "doc1").unwrap();
        let subject = SubjectReference::new(
            ObjectReference::new("user", "alice").unwrap(),
            None::<String>,
        )
        .unwrap();
        let err = c.check_permission(&resource, "", &subject).await.unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn check_permission_whitespace_permission_rejected() {
        let c = dummy_client();
        let resource = ObjectReference::new("document", "doc1").unwrap();
        let subject = SubjectReference::new(
            ObjectReference::new("user", "alice").unwrap(),
            None::<String>,
        )
        .unwrap();
        let err = c.check_permission(&resource, "  ", &subject).await.unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn lookup_resources_empty_permission_rejected() {
        let c = dummy_client();
        let subject = SubjectReference::new(
            ObjectReference::new("user", "alice").unwrap(),
            None::<String>,
        )
        .unwrap();
        let result = c.lookup_resources("document", "", &subject).send().await;
        assert!(matches!(result, Err(Error::InvalidArgument(_))));
    }

    #[tokio::test]
    async fn lookup_resources_empty_resource_type_rejected() {
        let c = dummy_client();
        let subject = SubjectReference::new(
            ObjectReference::new("user", "alice").unwrap(),
            None::<String>,
        )
        .unwrap();
        let result = c.lookup_resources("", "view", &subject).send().await;
        assert!(matches!(result, Err(Error::InvalidArgument(_))));
    }

    #[tokio::test]
    async fn lookup_subjects_empty_permission_rejected() {
        let c = dummy_client();
        let resource = ObjectReference::new("document", "doc1").unwrap();
        let result = c.lookup_subjects(&resource, "", "user").send().await;
        assert!(matches!(result, Err(Error::InvalidArgument(_))));
    }

    #[tokio::test]
    async fn lookup_subjects_empty_subject_type_rejected() {
        let c = dummy_client();
        let resource = ObjectReference::new("document", "doc1").unwrap();
        let result = c.lookup_subjects(&resource, "view", "").send().await;
        assert!(matches!(result, Err(Error::InvalidArgument(_))));
    }

    #[tokio::test]
    async fn expand_permission_tree_empty_permission_rejected() {
        let c = dummy_client();
        let resource = ObjectReference::new("document", "doc1").unwrap();
        let err = c
            .expand_permission_tree(&resource, "")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn expand_permission_tree_whitespace_permission_rejected() {
        let c = dummy_client();
        let resource = ObjectReference::new("document", "doc1").unwrap();
        let err = c
            .expand_permission_tree(&resource, "   ")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
    }
}
