//! Relationship filters and subject filters.

use crate::error::Error;
use crate::types::{Relationship, ZedToken};

/// A filter for selecting relationships by resource type, ID, relation, and/or subject.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RelationshipFilter {
    /// Resource type to filter on.
    pub resource_type: String,
    /// Optional resource ID.
    pub optional_resource_id: Option<String>,
    /// Optional relation name.
    pub optional_relation: Option<String>,
    /// Optional subject filter.
    pub optional_subject_filter: Option<SubjectFilter>,
}

impl RelationshipFilter {
    /// Creates a new filter for the given resource type.
    pub fn new(resource_type: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            optional_resource_id: None,
            optional_relation: None,
            optional_subject_filter: None,
        }
    }

    /// Adds a resource ID filter.
    pub fn resource_id(mut self, id: impl Into<String>) -> Self {
        self.optional_resource_id = Some(id.into());
        self
    }

    /// Adds a relation filter.
    pub fn relation(mut self, relation: impl Into<String>) -> Self {
        self.optional_relation = Some(relation.into());
        self
    }

    /// Adds a subject filter.
    pub fn subject_filter(mut self, filter: SubjectFilter) -> Self {
        self.optional_subject_filter = Some(filter);
        self
    }
}

impl From<&RelationshipFilter> for crate::proto::RelationshipFilter {
    fn from(f: &RelationshipFilter) -> Self {
        crate::proto::RelationshipFilter {
            resource_type: f.resource_type.clone(),
            optional_resource_id: f.optional_resource_id.clone().unwrap_or_default(),
            optional_resource_id_prefix: String::new(),
            optional_relation: f.optional_relation.clone().unwrap_or_default(),
            optional_subject_filter: f.optional_subject_filter.as_ref().map(Into::into),
        }
    }
}

/// A filter on the subject side of a relationship.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubjectFilter {
    /// The subject object type.
    pub subject_type: String,
    /// Optional subject object ID.
    pub optional_subject_id: Option<String>,
    /// Optional relation on the subject.
    pub optional_relation: Option<String>,
}

impl SubjectFilter {
    /// Creates a new subject filter for the given type.
    pub fn new(subject_type: impl Into<String>) -> Self {
        Self {
            subject_type: subject_type.into(),
            optional_subject_id: None,
            optional_relation: None,
        }
    }

    /// Adds a subject ID filter.
    pub fn subject_id(mut self, id: impl Into<String>) -> Self {
        self.optional_subject_id = Some(id.into());
        self
    }

    /// Adds a relation filter on the subject.
    pub fn relation(mut self, relation: impl Into<String>) -> Self {
        self.optional_relation = Some(relation.into());
        self
    }
}

impl From<&SubjectFilter> for crate::proto::SubjectFilter {
    fn from(f: &SubjectFilter) -> Self {
        crate::proto::SubjectFilter {
            subject_type: f.subject_type.clone(),
            optional_subject_id: f.optional_subject_id.clone().unwrap_or_default(),
            optional_relation: f.optional_relation.as_ref().map(|r| {
                crate::proto::subject_filter::RelationFilter {
                    relation: r.clone(),
                }
            }),
        }
    }
}

/// A relationship with the ZedToken at which it was read.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadRelationshipResult {
    /// The relationship.
    pub relationship: Relationship,
    /// The ZedToken at which this relationship was read.
    pub read_at: ZedToken,
}

impl ReadRelationshipResult {
    pub(crate) fn from_proto(
        proto: crate::proto::ReadRelationshipsResponse,
    ) -> Result<Self, Error> {
        let relationship = proto
            .relationship
            .ok_or_else(|| Error::Serialization("missing relationship".into()))?
            .try_into()?;
        let read_at = proto
            .read_at
            .ok_or_else(|| Error::Serialization("missing read_at token".into()))?
            .try_into()?;
        Ok(Self {
            relationship,
            read_at,
        })
    }
}
