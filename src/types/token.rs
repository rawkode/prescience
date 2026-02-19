//! ZedToken — represents a point in time / revision in SpiceDB.

use crate::error::Error;
use std::fmt;

/// A ZedToken represents a point in time (revision) in SpiceDB.
///
/// ZedTokens are returned by mutating operations and can be passed to
/// read operations via [`Consistency`](crate::Consistency) to ensure
/// causal consistency.
///
/// The token value is redacted in `Debug` output for security.
///
/// # Examples
///
/// ```
/// use prescience::ZedToken;
///
/// let token = ZedToken::new("some-opaque-token-value").unwrap();
/// // Debug output redacts the value
/// assert_eq!(format!("{:?}", token), r#"ZedToken("***")"#);
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ZedToken {
    token: String,
}

impl ZedToken {
    /// Creates a new `ZedToken` from a token string.
    ///
    /// Returns `Err` if the token string is empty.
    pub fn new(token: impl Into<String>) -> Result<Self, Error> {
        let token = token.into();
        if token.is_empty() {
            return Err(Error::InvalidArgument("ZedToken must not be empty".into()));
        }
        Ok(Self { token })
    }

    /// Returns the raw token string.
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// Debug output redacts the token value for security.
impl fmt::Debug for ZedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(r#"ZedToken("***")"#)
    }
}

impl fmt::Display for ZedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ZedToken(***)")
    }
}

impl From<&ZedToken> for crate::proto::ZedToken {
    fn from(t: &ZedToken) -> Self {
        crate::proto::ZedToken {
            token: t.token.clone(),
        }
    }
}

impl TryFrom<crate::proto::ZedToken> for ZedToken {
    type Error = Error;

    fn try_from(proto: crate::proto::ZedToken) -> Result<Self, Error> {
        ZedToken::new(proto.token)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ZedToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.token)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ZedToken {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ZedToken::new(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_token() {
        let token = ZedToken::new("abc123").unwrap();
        assert_eq!(token.token(), "abc123");
    }

    #[test]
    fn empty_token_rejected() {
        let err = ZedToken::new("").unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
    }

    #[test]
    fn debug_redacts_value() {
        let token = ZedToken::new("secret-token").unwrap();
        let debug = format!("{:?}", token);
        assert!(!debug.contains("secret-token"));
        assert!(debug.contains("***"));
    }

    #[test]
    fn equality_and_hash() {
        use std::collections::HashSet;
        let a = ZedToken::new("tok1").unwrap();
        let b = ZedToken::new("tok1").unwrap();
        let c = ZedToken::new("tok2").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut set = HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&b));
    }

    #[test]
    fn proto_roundtrip() {
        let orig = ZedToken::new("test-token").unwrap();
        let proto: crate::proto::ZedToken = (&orig).into();
        let back: ZedToken = proto.try_into().unwrap();
        assert_eq!(orig, back);
    }
}
