//! Error types for the Prescience SpiceDB client.
//!
//! The [`Error`] enum provides structured, matchable error variants covering
//! transport failures, gRPC status errors, local validation, serialization,
//! and conditional permission handling.
//!
//! ## gRPC Status Code Mapping
//!
//! | gRPC Code | Meaning | Retryable? |
//! |-----------|---------|------------|
//! | `UNAUTHENTICATED` | Invalid or missing bearer token | No |
//! | `PERMISSION_DENIED` | Token valid but insufficient permissions | No |
//! | `NOT_FOUND` | Resource or schema not found | No |
//! | `FAILED_PRECONDITION` | Write/delete precondition violated | No |
//! | `INVALID_ARGUMENT` | Server rejected request as malformed | No |
//! | `ALREADY_EXISTS` | Relationship already exists (with Create) | No |
//! | `UNAVAILABLE` | Server temporarily unavailable | Yes |
//! | `DEADLINE_EXCEEDED` | Request timed out | Yes |

use std::time::Duration;

/// Details extracted from SpiceDB-specific gRPC error metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpiceDbErrorDetails {
    /// SpiceDB ErrorReason enum value, if present.
    pub error_reason: Option<String>,
    /// Human-readable debug information from the server.
    pub debug_message: Option<String>,
    /// Suggested retry delay, if the server provided one.
    pub retry_info: Option<Duration>,
}

/// Errors returned by the Prescience SpiceDB client.
///
/// All public methods return `Result<T, Error>`. Use pattern matching
/// to handle specific failure modes, or [`Error::is_retryable`] for
/// simple retry logic.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Connection-level failures: connection refused, DNS resolution failure,
    /// TLS handshake errors, channel closed.
    #[error("transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    /// gRPC status errors returned by SpiceDB. Includes the status code,
    /// human-readable message, and optionally decoded SpiceDB-specific error details.
    #[error("SpiceDB error ({code:?}): {message}")]
    Status {
        /// The gRPC status code.
        code: tonic::Code,
        /// Human-readable error message from the server.
        message: String,
        /// Decoded SpiceDB-specific error details, if available.
        details: Option<SpiceDbErrorDetails>,
    },

    /// Local validation failures before a request is sent.
    ///
    /// Examples: empty `object_type`, empty `object_id`, empty schema string,
    /// empty relationship update list.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// Protobuf encode/decode failures. Indicates a bug or proto version mismatch.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Returned by [`PermissionResult::is_allowed()`](crate::PermissionResult::is_allowed)
    /// when the result is `Conditional`. Forces callers to handle the caveated
    /// case explicitly.
    #[error("conditional permission: missing context fields {missing_fields:?}")]
    ConditionalPermission {
        /// The context fields that were missing, preventing full caveat evaluation.
        missing_fields: Vec<String>,
    },
}

impl Error {
    /// Returns `true` if this error is likely transient and the request may
    /// succeed if retried.
    ///
    /// Currently considers `UNAVAILABLE` and `DEADLINE_EXCEEDED` as retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::Status {
                code: tonic::Code::Unavailable | tonic::Code::DeadlineExceeded,
                ..
            }
        )
    }

    /// Returns the gRPC status code if this is a `Status` error.
    pub fn code(&self) -> Option<tonic::Code> {
        match self {
            Error::Status { code, .. } => Some(*code),
            _ => None,
        }
    }

    pub(crate) fn from_status(status: tonic::Status) -> Self {
        // TODO: decode SpiceDB-specific error details from status metadata
        Error::Status {
            code: status.code(),
            message: status.message().to_string(),
            details: None,
        }
    }
}
