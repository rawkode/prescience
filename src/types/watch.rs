//! Watch event types (behind `watch` feature).

use crate::error::Error;
use crate::types::{RelationshipUpdate, ZedToken};

/// An event from the SpiceDB Watch stream.
///
/// Contains relationship changes and a checkpoint token for resumption.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchEvent {
    /// The relationship updates in this event.
    pub updates: Vec<RelationshipUpdate>,
    /// Checkpoint token for resuming the watch stream.
    pub checkpoint: ZedToken,
}

impl WatchEvent {
    pub(crate) fn from_proto(proto: crate::proto::WatchResponse) -> Result<Self, Error> {
        let updates: Result<Vec<RelationshipUpdate>, Error> =
            proto.updates.into_iter().map(TryInto::try_into).collect();
        let checkpoint = proto
            .changes_through
            .ok_or_else(|| Error::Serialization("missing changes_through token".into()))?
            .try_into()?;
        Ok(Self {
            updates: updates?,
            checkpoint,
        })
    }
}
