//! Consistency modes for SpiceDB reads.

use crate::types::ZedToken;

/// Controls the consistency guarantees for read operations.
///
/// When no consistency is specified, the library sends no preference to the
/// server, which defaults to `MinimizeLatency`.
///
/// # Examples
///
/// ```
/// use prescience::{Consistency, ZedToken};
///
/// // Strongest consistency — always read at latest
/// let c = Consistency::FullyConsistent;
///
/// // Read at least as fresh as a previous write
/// let token = ZedToken::new("some-token").unwrap();
/// let c = Consistency::AtLeastAsFresh(token);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Consistency {
    /// Server picks the fastest available snapshot. Lowest latency, weakest consistency.
    MinimizeLatency,
    /// All data must be at least as fresh as the given token.
    AtLeastAsFresh(ZedToken),
    /// All data must be at exactly the given token's snapshot.
    AtExactSnapshot(ZedToken),
    /// All data must be at the most recent snapshot. Strongest consistency, highest latency.
    FullyConsistent,
}

impl From<&Consistency> for crate::proto::Consistency {
    fn from(c: &Consistency) -> Self {
        use crate::proto::consistency::Requirement;
        crate::proto::Consistency {
            requirement: Some(match c {
                Consistency::MinimizeLatency => Requirement::MinimizeLatency(true),
                Consistency::AtLeastAsFresh(token) => Requirement::AtLeastAsFresh(token.into()),
                Consistency::AtExactSnapshot(token) => Requirement::AtExactSnapshot(token.into()),
                Consistency::FullyConsistent => Requirement::FullyConsistent(true),
            }),
        }
    }
}
