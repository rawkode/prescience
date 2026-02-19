//! Object and subject references.

use crate::error::Error;

/// A reference to a specific object in the SpiceDB system.
///
/// Consists of an object type (e.g., `"document"`) and an object ID (e.g., `"doc-123"`).
/// Both fields must be non-empty.
///
/// # Examples
///
/// ```
/// use prescience::ObjectReference;
///
/// let obj = ObjectReference::new("document", "doc-123").unwrap();
/// assert_eq!(obj.object_type(), "document");
/// assert_eq!(obj.object_id(), "doc-123");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectReference {
    object_type: String,
    object_id: String,
}

impl ObjectReference {
    /// Creates a new `ObjectReference` with the given type and ID.
    ///
    /// Returns `Err` if either `object_type` or `object_id` is empty.
    pub fn new(
        object_type: impl Into<String>,
        object_id: impl Into<String>,
    ) -> Result<Self, Error> {
        let object_type = object_type.into();
        let object_id = object_id.into();

        if object_type.is_empty() {
            return Err(Error::InvalidArgument(
                "object_type must not be empty".into(),
            ));
        }
        if object_id.is_empty() {
            return Err(Error::InvalidArgument("object_id must not be empty".into()));
        }

        Ok(Self {
            object_type,
            object_id,
        })
    }

    /// Returns the object type.
    pub fn object_type(&self) -> &str {
        &self.object_type
    }

    /// Returns the object ID.
    pub fn object_id(&self) -> &str {
        &self.object_id
    }
}

impl From<&ObjectReference> for crate::proto::ObjectReference {
    fn from(r: &ObjectReference) -> Self {
        crate::proto::ObjectReference {
            object_type: r.object_type.clone(),
            object_id: r.object_id.clone(),
        }
    }
}

impl TryFrom<crate::proto::ObjectReference> for ObjectReference {
    type Error = Error;

    fn try_from(proto: crate::proto::ObjectReference) -> Result<Self, Error> {
        ObjectReference::new(proto.object_type, proto.object_id)
    }
}

/// A reference to a subject in a relationship.
///
/// Consists of an [`ObjectReference`] and an optional relation name
/// (e.g., `group:eng#member`).
///
/// # Examples
///
/// ```
/// use prescience::{ObjectReference, SubjectReference};
///
/// // Simple subject
/// let subject = SubjectReference::new(
///     ObjectReference::new("user", "alice").unwrap(),
///     None::<String>,
/// ).unwrap();
///
/// // Subject with relation
/// let subject = SubjectReference::new(
///     ObjectReference::new("group", "eng").unwrap(),
///     Some("member"),
/// ).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubjectReference {
    object: ObjectReference,
    optional_relation: Option<String>,
}

impl SubjectReference {
    /// Creates a new `SubjectReference`.
    ///
    /// Returns `Err` if `optional_relation` is `Some("")` (empty string).
    /// Use `None` instead to indicate no relation.
    pub fn new(
        object: ObjectReference,
        optional_relation: Option<impl Into<String>>,
    ) -> Result<Self, Error> {
        let optional_relation = optional_relation.map(Into::into);
        if let Some(ref rel) = optional_relation {
            if rel.is_empty() {
                return Err(Error::InvalidArgument(
                    "optional_relation must not be empty; use None instead".into(),
                ));
            }
        }
        Ok(Self {
            object,
            optional_relation,
        })
    }

    /// Returns the subject's object reference.
    pub fn object(&self) -> &ObjectReference {
        &self.object
    }

    /// Returns the optional relation on the subject.
    pub fn optional_relation(&self) -> Option<&str> {
        self.optional_relation.as_deref()
    }
}

impl From<&SubjectReference> for crate::proto::SubjectReference {
    fn from(r: &SubjectReference) -> Self {
        crate::proto::SubjectReference {
            object: Some((&r.object).into()),
            optional_relation: r.optional_relation.clone().unwrap_or_default(),
        }
    }
}

impl TryFrom<crate::proto::SubjectReference> for SubjectReference {
    type Error = Error;

    fn try_from(proto: crate::proto::SubjectReference) -> Result<Self, Error> {
        let object = proto
            .object
            .ok_or_else(|| Error::Serialization("missing subject object".into()))?
            .try_into()?;
        let relation = if proto.optional_relation.is_empty() {
            None
        } else {
            Some(proto.optional_relation)
        };
        SubjectReference::new(object, relation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_reference_valid() {
        let obj = ObjectReference::new("document", "doc-123").unwrap();
        assert_eq!(obj.object_type(), "document");
        assert_eq!(obj.object_id(), "doc-123");
    }

    #[test]
    fn object_reference_empty_type() {
        let err = ObjectReference::new("", "doc-123").unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
    }

    #[test]
    fn object_reference_empty_id() {
        let err = ObjectReference::new("document", "").unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
    }

    #[test]
    fn object_reference_equality_and_hash() {
        use std::collections::HashSet;
        let a = ObjectReference::new("doc", "1").unwrap();
        let b = ObjectReference::new("doc", "1").unwrap();
        let c = ObjectReference::new("doc", "2").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut set = HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&b));
        assert!(!set.contains(&c));
    }

    #[test]
    fn subject_reference_without_relation() {
        let obj = ObjectReference::new("user", "alice").unwrap();
        let sub = SubjectReference::new(obj, None::<String>).unwrap();
        assert_eq!(sub.object().object_type(), "user");
        assert_eq!(sub.optional_relation(), None);
    }

    #[test]
    fn subject_reference_with_relation() {
        let obj = ObjectReference::new("group", "eng").unwrap();
        let sub = SubjectReference::new(obj, Some("member")).unwrap();
        assert_eq!(sub.optional_relation(), Some("member"));
    }

    #[test]
    fn subject_reference_empty_relation_rejected() {
        let obj = ObjectReference::new("group", "eng").unwrap();
        let err = SubjectReference::new(obj, Some("")).unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
    }

    #[test]
    fn proto_roundtrip_object_reference() {
        let orig = ObjectReference::new("document", "doc-123").unwrap();
        let proto: crate::proto::ObjectReference = (&orig).into();
        let back: ObjectReference = proto.try_into().unwrap();
        assert_eq!(orig, back);
    }

    #[test]
    fn proto_roundtrip_subject_reference() {
        let obj = ObjectReference::new("user", "alice").unwrap();
        let orig = SubjectReference::new(obj, Some("member")).unwrap();
        let proto: crate::proto::SubjectReference = (&orig).into();
        let back: SubjectReference = proto.try_into().unwrap();
        assert_eq!(orig, back);
    }
}
