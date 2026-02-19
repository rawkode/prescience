//! Domain types for the Prescience SpiceDB client.
//!
//! These are idiomatic Rust types wrapping the generated protobuf types.
//! The proto types are internal implementation details and are never exposed.

mod consistency;
pub(crate) mod context;
mod filter;
mod permission;
mod reference;
mod relationship;
mod token;
#[cfg(feature = "watch")]
mod watch;

pub use consistency::Consistency;
pub use context::ContextValue;
pub use filter::{RelationshipFilter, SubjectFilter};
pub use permission::{PermissionResult, PermissionTree, PermissionTreeNode};
pub use reference::{ObjectReference, SubjectReference};
pub use relationship::{
    Caveat, Operation, Precondition, PreconditionOp, Relationship, RelationshipUpdate,
};
pub use token::ZedToken;
#[cfg(feature = "watch")]
pub use watch::WatchEvent;

// Re-export streaming result types
pub use filter::ReadRelationshipResult;
pub use permission::CheckResult;
pub use permission::{LookupResourceResult, LookupSubjectResult};
