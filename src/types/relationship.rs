//! Relationship, RelationshipUpdate, Caveat, and Precondition types.

use std::collections::HashMap;

use crate::error::Error;
use crate::types::{ContextValue, ObjectReference, SubjectReference};

/// A caveat attached to a relationship, with optional context for evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct Caveat {
    /// The caveat name as defined in the SpiceDB schema.
    pub name: String,
    /// Key-value context pairs for caveat evaluation.
    pub context: HashMap<String, ContextValue>,
}

impl Caveat {
    /// Creates a new caveat with the given name and context.
    pub fn new(name: impl Into<String>, context: HashMap<String, ContextValue>) -> Self {
        Self {
            name: name.into(),
            context,
        }
    }
}

/// A relationship between a resource and a subject via a relation.
#[derive(Debug, Clone, PartialEq)]
pub struct Relationship {
    /// The resource side of the relationship.
    pub resource: ObjectReference,
    /// The relation name (e.g., `"viewer"`, `"owner"`).
    pub relation: String,
    /// The subject side of the relationship.
    pub subject: SubjectReference,
    /// An optional caveat on this relationship.
    pub optional_caveat: Option<Caveat>,
}

impl Relationship {
    /// Creates a new relationship without a caveat.
    pub fn new(
        resource: ObjectReference,
        relation: impl Into<String>,
        subject: SubjectReference,
    ) -> Self {
        Self {
            resource,
            relation: relation.into(),
            subject,
            optional_caveat: None,
        }
    }

    /// Attaches a caveat to this relationship.
    pub fn with_caveat(mut self, caveat: Caveat) -> Self {
        self.optional_caveat = Some(caveat);
        self
    }
}

impl TryFrom<crate::proto::Relationship> for Relationship {
    type Error = Error;

    fn try_from(proto: crate::proto::Relationship) -> Result<Self, Error> {
        let resource = proto
            .resource
            .ok_or_else(|| Error::Serialization("missing resource".into()))?
            .try_into()?;
        let subject = proto
            .subject
            .ok_or_else(|| Error::Serialization("missing subject".into()))?
            .try_into()?;
        let optional_caveat = proto.optional_caveat.map(|c| Caveat {
            name: c.caveat_name,
            context: c
                .context
                .map(|s| s.fields.into_iter().map(|(k, v)| (k, v.into())).collect())
                .unwrap_or_default(),
        });
        Ok(Relationship {
            resource,
            relation: proto.relation,
            subject,
            optional_caveat,
        })
    }
}

impl From<&Relationship> for crate::proto::Relationship {
    fn from(r: &Relationship) -> Self {
        crate::proto::Relationship {
            resource: Some((&r.resource).into()),
            relation: r.relation.clone(),
            subject: Some((&r.subject).into()),
            optional_caveat: r.optional_caveat.as_ref().map(|c| {
                crate::proto::ContextualizedCaveat {
                    caveat_name: c.name.clone(),
                    context: if c.context.is_empty() {
                        None
                    } else {
                        Some(crate::types::context::context_to_struct(&c.context))
                    },
                }
            }),
            optional_expires_at: None,
        }
    }
}

/// The operation to perform on a relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    /// Create the relationship; error if it already exists.
    Create,
    /// Upsert the relationship; no error if it already exists.
    Touch,
    /// Delete the relationship; no-op if it doesn't exist.
    Delete,
}

/// A relationship mutation (create, touch, or delete).
#[derive(Debug, Clone, PartialEq)]
pub struct RelationshipUpdate {
    /// The operation to perform.
    pub operation: Operation,
    /// The relationship to mutate.
    pub relationship: Relationship,
}

impl RelationshipUpdate {
    /// Creates a CREATE update for the given relationship.
    pub fn create(relationship: Relationship) -> Self {
        Self {
            operation: Operation::Create,
            relationship,
        }
    }

    /// Creates a TOUCH (upsert) update for the given relationship.
    pub fn touch(relationship: Relationship) -> Self {
        Self {
            operation: Operation::Touch,
            relationship,
        }
    }

    /// Creates a DELETE update for the given relationship.
    pub fn delete(relationship: Relationship) -> Self {
        Self {
            operation: Operation::Delete,
            relationship,
        }
    }
}

impl From<&RelationshipUpdate> for crate::proto::RelationshipUpdate {
    fn from(u: &RelationshipUpdate) -> Self {
        crate::proto::RelationshipUpdate {
            operation: match u.operation {
                Operation::Create => crate::proto::relationship_update::Operation::Create as i32,
                Operation::Touch => crate::proto::relationship_update::Operation::Touch as i32,
                Operation::Delete => crate::proto::relationship_update::Operation::Delete as i32,
            },
            relationship: Some((&u.relationship).into()),
        }
    }
}

impl TryFrom<crate::proto::RelationshipUpdate> for RelationshipUpdate {
    type Error = Error;

    fn try_from(proto: crate::proto::RelationshipUpdate) -> Result<Self, Error> {
        let operation = match proto.operation {
            1 => Operation::Create,
            2 => Operation::Touch,
            3 => Operation::Delete,
            other => {
                return Err(Error::Serialization(format!(
                    "unknown operation: {}",
                    other
                )))
            }
        };
        let relationship = proto
            .relationship
            .ok_or_else(|| Error::Serialization("missing relationship".into()))?
            .try_into()?;
        Ok(RelationshipUpdate {
            operation,
            relationship,
        })
    }
}

/// The operation for a precondition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreconditionOp {
    /// The filter must match at least one existing relationship.
    MustExist,
    /// The filter must not match any existing relationships.
    MustNotExist,
}

/// A precondition on a write or delete operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Precondition {
    /// The precondition operation.
    pub operation: PreconditionOp,
    /// The filter that must (or must not) match.
    pub filter: crate::types::RelationshipFilter,
}

impl Precondition {
    /// Creates a precondition that requires matching relationships to exist.
    pub fn must_exist(filter: crate::types::RelationshipFilter) -> Self {
        Self {
            operation: PreconditionOp::MustExist,
            filter,
        }
    }

    /// Creates a precondition that requires no matching relationships to exist.
    pub fn must_not_exist(filter: crate::types::RelationshipFilter) -> Self {
        Self {
            operation: PreconditionOp::MustNotExist,
            filter,
        }
    }
}

impl From<&Precondition> for crate::proto::Precondition {
    fn from(p: &Precondition) -> Self {
        crate::proto::Precondition {
            operation: match p.operation {
                PreconditionOp::MustExist => {
                    crate::proto::precondition::Operation::MustMatch as i32
                }
                PreconditionOp::MustNotExist => {
                    crate::proto::precondition::Operation::MustNotMatch as i32
                }
            },
            filter: Some((&p.filter).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationship_create_update() {
        let rel = Relationship::new(
            ObjectReference::new("doc", "1").unwrap(),
            "viewer",
            SubjectReference::new(
                ObjectReference::new("user", "alice").unwrap(),
                None::<String>,
            )
            .unwrap(),
        );
        let update = RelationshipUpdate::create(rel);
        assert_eq!(update.operation, Operation::Create);
    }

    #[test]
    fn relationship_with_caveat() {
        let rel = Relationship::new(
            ObjectReference::new("doc", "1").unwrap(),
            "viewer",
            SubjectReference::new(
                ObjectReference::new("user", "alice").unwrap(),
                None::<String>,
            )
            .unwrap(),
        )
        .with_caveat(Caveat::new("ip_check", HashMap::new()));
        assert!(rel.optional_caveat.is_some());
        assert_eq!(rel.optional_caveat.unwrap().name, "ip_check");
    }

    #[test]
    fn precondition_must_exist() {
        use crate::types::RelationshipFilter;
        let p = Precondition::must_exist(RelationshipFilter::new("document"));
        assert_eq!(p.operation, PreconditionOp::MustExist);
    }
}
