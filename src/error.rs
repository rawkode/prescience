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

use std::collections::HashMap;
use std::time::Duration;

use prost::Message;

/// Details extracted from SpiceDB-specific gRPC error metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiceDbErrorDetails {
    /// SpiceDB ErrorReason enum value, if present (e.g. `"ERROR_REASON_SCHEMA_PARSE_ERROR"`).
    pub error_reason: Option<String>,
    /// Human-readable debug information from the server.
    pub debug_message: Option<String>,
    /// Suggested retry delay, if the server provided one.
    pub retry_info: Option<Duration>,
    /// Additional metadata key-value pairs from the ErrorInfo, if present.
    pub metadata: HashMap<String, String>,
}

// Google RPC detail types for decoding from google.protobuf.Any.
// These are not compiled from proto since we only have status.proto in stubs.

/// google.rpc.ErrorInfo
#[derive(Clone, PartialEq, prost::Message)]
struct ErrorInfo {
    #[prost(string, tag = "1")]
    reason: String,
    #[prost(string, tag = "2")]
    domain: String,
    #[prost(map = "string, string", tag = "3")]
    metadata: HashMap<String, String>,
}

/// google.rpc.DebugInfo
#[derive(Clone, PartialEq, prost::Message)]
struct DebugInfo {
    #[prost(string, repeated, tag = "1")]
    stack_entries: Vec<String>,
    #[prost(string, tag = "2")]
    detail: String,
}

/// google.rpc.RetryInfo
#[derive(Clone, PartialEq, prost::Message)]
struct RetryInfo {
    #[prost(message, optional, tag = "1")]
    retry_delay: Option<prost_types::Duration>,
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
        details: Option<Box<SpiceDbErrorDetails>>,
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
        let details = Self::decode_details(&status);
        Error::Status {
            code: status.code(),
            message: status.message().to_string(),
            details,
        }
    }

    /// Attempts to decode SpiceDB error details from the `grpc-status-details-bin`
    /// metadata header. Returns `None` if the header is absent or cannot be decoded.
    fn decode_details(status: &tonic::Status) -> Option<Box<SpiceDbErrorDetails>> {
        let bin = status
            .metadata()
            .get_bin("grpc-status-details-bin")?
            .to_bytes()
            .ok()?;

        let rpc_status = crate::proto::google::rpc::Status::decode(bin.as_ref()).ok()?;

        let mut error_reason = None;
        let mut debug_message = None;
        let mut retry_info = None;
        let mut metadata = HashMap::new();

        for any in &rpc_status.details {
            match any.type_url.as_str() {
                "type.googleapis.com/google.rpc.ErrorInfo" => {
                    if let Ok(info) = ErrorInfo::decode(any.value.as_ref()) {
                        if !info.reason.is_empty() {
                            error_reason = Some(info.reason);
                        }
                        metadata = info.metadata;
                    }
                }
                "type.googleapis.com/google.rpc.DebugInfo" => {
                    if let Ok(info) = DebugInfo::decode(any.value.as_ref()) {
                        if !info.detail.is_empty() {
                            debug_message = Some(info.detail);
                        }
                    }
                }
                "type.googleapis.com/google.rpc.RetryInfo" => {
                    if let Ok(info) = RetryInfo::decode(any.value.as_ref()) {
                        if let Some(delay) = info.retry_delay {
                            let duration = Duration::new(
                                delay.seconds.max(0) as u64,
                                delay.nanos.max(0) as u32,
                            );
                            if !duration.is_zero() {
                                retry_info = Some(duration);
                            }
                        }
                    }
                }
                _ => {} // ignore unknown detail types
            }
        }

        // Only return Some if at least one field was populated
        if error_reason.is_some()
            || debug_message.is_some()
            || retry_info.is_some()
            || !metadata.is_empty()
        {
            Some(Box::new(SpiceDbErrorDetails {
                error_reason,
                debug_message,
                retry_info,
                metadata,
            }))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a tonic::Status with encoded google.rpc.Status details.
    fn status_with_details(
        code: tonic::Code,
        message: &str,
        details: Vec<prost_types::Any>,
    ) -> tonic::Status {
        let rpc_status = crate::proto::google::rpc::Status {
            code: code as i32,
            message: message.to_string(),
            details,
        };
        let mut buf = Vec::new();
        rpc_status.encode(&mut buf).unwrap();

        let mut status = tonic::Status::new(code, message);
        status.metadata_mut().insert_bin(
            "grpc-status-details-bin",
            tonic::metadata::MetadataValue::from_bytes(&buf),
        );
        status
    }

    fn encode_any<M: Message>(type_url: &str, msg: &M) -> prost_types::Any {
        prost_types::Any {
            type_url: type_url.to_string(),
            value: msg.encode_to_vec(),
        }
    }

    #[test]
    fn from_status_without_details() {
        let status = tonic::Status::not_found("thing not found");
        let err = Error::from_status(status);
        match &err {
            Error::Status {
                code,
                message,
                details,
            } => {
                assert_eq!(*code, tonic::Code::NotFound);
                assert_eq!(message, "thing not found");
                assert!(details.is_none());
            }
            _ => panic!("expected Status variant"),
        }
    }

    #[test]
    fn from_status_with_error_info() {
        let error_info = ErrorInfo {
            reason: "ERROR_REASON_SCHEMA_PARSE_ERROR".to_string(),
            domain: "authzed.com".to_string(),
            metadata: HashMap::from([
                ("start_line_number".to_string(), "1".to_string()),
                ("source_code".to_string(), "bad_def".to_string()),
            ]),
        };
        let status = status_with_details(
            tonic::Code::InvalidArgument,
            "schema parse error",
            vec![encode_any(
                "type.googleapis.com/google.rpc.ErrorInfo",
                &error_info,
            )],
        );

        let err = Error::from_status(status);
        match &err {
            Error::Status { details, .. } => {
                let d = details.as_ref().expect("should have details");
                assert_eq!(
                    d.error_reason.as_deref(),
                    Some("ERROR_REASON_SCHEMA_PARSE_ERROR")
                );
                assert_eq!(d.metadata.get("start_line_number").unwrap(), "1");
                assert_eq!(d.metadata.get("source_code").unwrap(), "bad_def");
                assert!(d.debug_message.is_none());
                assert!(d.retry_info.is_none());
            }
            _ => panic!("expected Status variant"),
        }
    }

    #[test]
    fn from_status_with_debug_info() {
        let debug_info = DebugInfo {
            stack_entries: vec!["frame1".into()],
            detail: "something went wrong internally".to_string(),
        };
        let status = status_with_details(
            tonic::Code::Internal,
            "internal",
            vec![encode_any(
                "type.googleapis.com/google.rpc.DebugInfo",
                &debug_info,
            )],
        );

        let err = Error::from_status(status);
        match &err {
            Error::Status { details, .. } => {
                let d = details.as_ref().expect("should have details");
                assert_eq!(
                    d.debug_message.as_deref(),
                    Some("something went wrong internally")
                );
                assert!(d.error_reason.is_none());
            }
            _ => panic!("expected Status variant"),
        }
    }

    #[test]
    fn from_status_with_retry_info() {
        let retry_info = RetryInfo {
            retry_delay: Some(prost_types::Duration {
                seconds: 5,
                nanos: 500_000_000,
            }),
        };
        let status = status_with_details(
            tonic::Code::Unavailable,
            "temporarily unavailable",
            vec![encode_any(
                "type.googleapis.com/google.rpc.RetryInfo",
                &retry_info,
            )],
        );

        let err = Error::from_status(status);
        match &err {
            Error::Status { details, .. } => {
                let d = details.as_ref().expect("should have details");
                assert_eq!(d.retry_info, Some(Duration::new(5, 500_000_000)));
            }
            _ => panic!("expected Status variant"),
        }
    }

    #[test]
    fn from_status_with_all_detail_types() {
        let error_info = ErrorInfo {
            reason: "ERROR_REASON_UNKNOWN_DEFINITION".to_string(),
            domain: "authzed.com".to_string(),
            metadata: HashMap::from([("definition_name".to_string(), "user".to_string())]),
        };
        let debug_info = DebugInfo {
            stack_entries: vec![],
            detail: "debug trace".to_string(),
        };
        let retry_info = RetryInfo {
            retry_delay: Some(prost_types::Duration {
                seconds: 2,
                nanos: 0,
            }),
        };
        let status = status_with_details(
            tonic::Code::InvalidArgument,
            "bad request",
            vec![
                encode_any("type.googleapis.com/google.rpc.ErrorInfo", &error_info),
                encode_any("type.googleapis.com/google.rpc.DebugInfo", &debug_info),
                encode_any("type.googleapis.com/google.rpc.RetryInfo", &retry_info),
            ],
        );

        let err = Error::from_status(status);
        match &err {
            Error::Status { details, .. } => {
                let d = details.as_ref().expect("should have details");
                assert_eq!(
                    d.error_reason.as_deref(),
                    Some("ERROR_REASON_UNKNOWN_DEFINITION")
                );
                assert_eq!(d.debug_message.as_deref(), Some("debug trace"));
                assert_eq!(d.retry_info, Some(Duration::from_secs(2)));
                assert_eq!(d.metadata.get("definition_name").unwrap(), "user");
            }
            _ => panic!("expected Status variant"),
        }
    }

    #[test]
    fn from_status_with_malformed_details_bin() {
        let mut status = tonic::Status::new(tonic::Code::Internal, "broken");
        status.metadata_mut().insert_bin(
            "grpc-status-details-bin",
            tonic::metadata::MetadataValue::from_bytes(b"not valid protobuf"),
        );
        let err = Error::from_status(status);
        match &err {
            Error::Status { details, .. } => {
                assert!(details.is_none(), "malformed bytes should yield None");
            }
            _ => panic!("expected Status variant"),
        }
    }

    #[test]
    fn from_status_ignores_unknown_any_types() {
        let unknown = prost_types::Any {
            type_url: "type.googleapis.com/some.Unknown".to_string(),
            value: vec![1, 2, 3],
        };
        let status = status_with_details(
            tonic::Code::Internal,
            "with unknown",
            vec![unknown],
        );
        let err = Error::from_status(status);
        match &err {
            Error::Status { details, .. } => {
                assert!(details.is_none(), "unknown types only should yield None");
            }
            _ => panic!("expected Status variant"),
        }
    }
}
