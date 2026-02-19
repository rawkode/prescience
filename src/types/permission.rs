//! Permission result types, permission tree, and lookup result types.

use crate::error::Error;
use crate::types::{ObjectReference, SubjectReference, ZedToken};

/// The result of a permission check.
///
/// SpiceDB returns a 3-state result: the subject definitively has or lacks
/// the permission, or the permission is conditional on unresolved caveat context.
///
/// Use [`is_allowed()`](PermissionResult::is_allowed) for a convenience boolean,
/// but note that it returns `Err` for `Conditional` to force explicit handling.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PermissionResult {
    /// The subject definitively has the permission.
    Allowed,
    /// The subject definitively does not have the permission.
    Denied,
    /// The permission depends on unresolved caveat context.
    Conditional {
        /// Context fields needed to fully evaluate the caveat.
        missing_fields: Vec<String>,
    },
}

impl PermissionResult {
    /// Returns `Ok(true)` for `Allowed`, `Ok(false)` for `Denied`, and
    /// `Err(Error::ConditionalPermission)` for `Conditional`.
    ///
    /// This forces callers to handle the conditional case explicitly rather
    /// than silently dropping it.
    pub fn is_allowed(&self) -> Result<bool, Error> {
        match self {
            PermissionResult::Allowed => Ok(true),
            PermissionResult::Denied => Ok(false),
            PermissionResult::Conditional { missing_fields } => Err(Error::ConditionalPermission {
                missing_fields: missing_fields.clone(),
            }),
        }
    }

    /// Returns `true` only for `Denied`.
    pub fn is_denied(&self) -> bool {
        matches!(self, PermissionResult::Denied)
    }

    /// Returns `true` only for `Conditional`.
    pub fn is_conditional(&self) -> bool {
        matches!(self, PermissionResult::Conditional { .. })
    }

    pub(crate) fn from_check_response(
        permissionship: i32,
        partial_caveat_info: Option<crate::proto::PartialCaveatInfo>,
    ) -> Result<Self, Error> {
        match permissionship {
            2 => Ok(PermissionResult::Allowed),
            1 => Ok(PermissionResult::Denied),
            3 => Ok(PermissionResult::Conditional {
                missing_fields: partial_caveat_info
                    .map(|info| info.missing_required_context)
                    .unwrap_or_default(),
            }),
            other => Err(Error::Serialization(format!(
                "unknown permissionship: {}",
                other
            ))),
        }
    }

    pub(crate) fn from_lookup_permissionship(
        permissionship: i32,
        partial_caveat_info: Option<crate::proto::PartialCaveatInfo>,
    ) -> Result<Self, Error> {
        match permissionship {
            1 => Ok(PermissionResult::Allowed),
            2 => Ok(PermissionResult::Conditional {
                missing_fields: partial_caveat_info
                    .map(|info| info.missing_required_context)
                    .unwrap_or_default(),
            }),
            other => Err(Error::Serialization(format!(
                "unknown lookup permissionship: {}",
                other
            ))),
        }
    }
}

/// A resource found by LookupResources, with its permission status and token.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LookupResourceResult {
    /// The resource object ID.
    pub resource_id: String,
    /// The permission status for this resource.
    pub permission: PermissionResult,
    /// The ZedToken at which this resource was looked up.
    pub looked_up_at: ZedToken,
}

impl LookupResourceResult {
    pub(crate) fn from_proto(proto: crate::proto::LookupResourcesResponse) -> Result<Self, Error> {
        let permission = PermissionResult::from_lookup_permissionship(
            proto.permissionship,
            proto.partial_caveat_info,
        )?;
        let looked_up_at = proto
            .looked_up_at
            .ok_or_else(|| Error::Serialization("missing looked_up_at".into()))?
            .try_into()?;
        Ok(Self {
            resource_id: proto.resource_object_id,
            permission,
            looked_up_at,
        })
    }
}

/// A subject found by LookupSubjects, with its permission status and token.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LookupSubjectResult {
    /// The resolved subject.
    pub subject_id: String,
    /// Subjects excluded from a wildcard match.
    pub excluded_subject_ids: Vec<String>,
    /// The permission status for this subject.
    pub permission: PermissionResult,
    /// The ZedToken at which this subject was looked up.
    pub looked_up_at: ZedToken,
}

impl LookupSubjectResult {
    pub(crate) fn from_proto(proto: crate::proto::LookupSubjectsResponse) -> Result<Self, Error> {
        let looked_up_at = proto
            .looked_up_at
            .ok_or_else(|| Error::Serialization("missing looked_up_at".into()))?
            .try_into()?;

        // Use the new `subject` field if available, fall back to deprecated fields
        if let Some(resolved) = proto.subject {
            let permission = PermissionResult::from_lookup_permissionship(
                resolved.permissionship,
                resolved.partial_caveat_info,
            )?;
            let excluded_ids: Vec<String> = proto
                .excluded_subjects
                .into_iter()
                .map(|s| s.subject_object_id)
                .collect();
            Ok(Self {
                subject_id: resolved.subject_object_id,
                excluded_subject_ids: excluded_ids,
                permission,
                looked_up_at,
            })
        } else {
            // Fallback to deprecated fields
            #[allow(deprecated)]
            let permission = PermissionResult::from_lookup_permissionship(
                proto.permissionship,
                proto.partial_caveat_info,
            )?;
            #[allow(deprecated)]
            Ok(Self {
                subject_id: proto.subject_object_id,
                excluded_subject_ids: proto.excluded_subject_ids,
                permission,
                looked_up_at,
            })
        }
    }
}

/// Per-item result from a bulk check operation.
pub type CheckResult = Result<PermissionResult, Error>;

/// A recursive permission tree returned by ExpandPermissionTree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PermissionTree {
    /// The object that was expanded.
    pub expanded_object: ObjectReference,
    /// The relation that was expanded.
    pub expanded_relation: String,
    /// The root node of the permission tree.
    pub node: PermissionTreeNode,
}

/// A node in a permission tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PermissionTreeNode {
    /// A leaf node containing direct subjects.
    Leaf {
        /// The subjects at this leaf.
        subjects: Vec<SubjectReference>,
    },
    /// A union of child nodes.
    Union {
        /// Child nodes whose subjects are unioned.
        children: Vec<PermissionTreeNode>,
    },
    /// An intersection of child nodes.
    Intersection {
        /// Child nodes whose subjects are intersected.
        children: Vec<PermissionTreeNode>,
    },
    /// An exclusion: base minus excluded.
    Exclusion {
        /// The base set.
        base: Box<PermissionTreeNode>,
        /// The set to exclude.
        excluded: Box<PermissionTreeNode>,
    },
}

impl PermissionTree {
    pub(crate) fn from_proto(
        proto: crate::proto::PermissionRelationshipTree,
    ) -> Result<Self, Error> {
        let expanded_object = proto
            .expanded_object
            .ok_or_else(|| Error::Serialization("missing expanded_object".into()))?
            .try_into()?;
        let node = PermissionTreeNode::from_proto_tree(proto.tree_type)?;
        Ok(Self {
            expanded_object,
            expanded_relation: proto.expanded_relation,
            node,
        })
    }
}

impl PermissionTreeNode {
    fn from_proto_tree(
        tree_type: Option<crate::proto::permission_relationship_tree::TreeType>,
    ) -> Result<Self, Error> {
        match tree_type {
            Some(crate::proto::permission_relationship_tree::TreeType::Leaf(leaf)) => {
                let subjects: Result<Vec<SubjectReference>, Error> =
                    leaf.subjects.into_iter().map(TryInto::try_into).collect();
                Ok(PermissionTreeNode::Leaf {
                    subjects: subjects?,
                })
            }
            Some(crate::proto::permission_relationship_tree::TreeType::Intermediate(alg)) => {
                let children: Result<Vec<PermissionTreeNode>, Error> = alg
                    .children
                    .into_iter()
                    .map(|child| PermissionTreeNode::from_proto_tree(child.tree_type))
                    .collect();
                let children = children?;

                match alg.operation {
                    1 => Ok(PermissionTreeNode::Union { children }),
                    2 => Ok(PermissionTreeNode::Intersection { children }),
                    3 => {
                        // Exclusion: exactly 2 children — base and excluded
                        if children.len() != 2 {
                            return Err(Error::Serialization(format!(
                                "exclusion requires exactly 2 children, got {}",
                                children.len()
                            )));
                        }
                        let mut iter = children.into_iter();
                        let base = Box::new(iter.next().unwrap());
                        let excluded = Box::new(iter.next().unwrap());
                        Ok(PermissionTreeNode::Exclusion { base, excluded })
                    }
                    other => Err(Error::Serialization(format!(
                        "unknown algebraic operation: {}",
                        other
                    ))),
                }
            }
            None => Err(Error::Serialization("missing tree type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_result_allowed() {
        let r = PermissionResult::Allowed;
        assert_eq!(r.is_allowed().unwrap(), true);
        assert!(!r.is_denied());
        assert!(!r.is_conditional());
    }

    #[test]
    fn permission_result_denied() {
        let r = PermissionResult::Denied;
        assert_eq!(r.is_allowed().unwrap(), false);
        assert!(r.is_denied());
        assert!(!r.is_conditional());
    }

    #[test]
    fn permission_result_conditional() {
        let r = PermissionResult::Conditional {
            missing_fields: vec!["ip_address".into()],
        };
        let err = r.is_allowed().unwrap_err();
        assert!(matches!(err, Error::ConditionalPermission { .. }));
        assert!(!r.is_denied());
        assert!(r.is_conditional());
    }

    #[test]
    fn from_check_response_allowed() {
        let r = PermissionResult::from_check_response(2, None).unwrap();
        assert_eq!(r, PermissionResult::Allowed);
    }

    #[test]
    fn from_check_response_denied() {
        let r = PermissionResult::from_check_response(1, None).unwrap();
        assert_eq!(r, PermissionResult::Denied);
    }

    #[test]
    fn from_check_response_conditional() {
        let info = crate::proto::PartialCaveatInfo {
            missing_required_context: vec!["field1".into()],
        };
        let r = PermissionResult::from_check_response(3, Some(info)).unwrap();
        assert_eq!(
            r,
            PermissionResult::Conditional {
                missing_fields: vec!["field1".into()]
            }
        );
    }
}
