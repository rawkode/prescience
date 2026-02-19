//! SchemaService RPC implementations.

use crate::error::Error;
use crate::proto;
use crate::types::ZedToken;

use super::Client;

impl Client {
    /// Reads the current SpiceDB schema.
    ///
    /// Returns the schema text and the ZedToken at which it was read.
    pub async fn read_schema(&self) -> Result<(String, ZedToken), Error> {
        let response = self
            .schema
            .clone()
            .read_schema(proto::ReadSchemaRequest {})
            .await
            .map_err(Error::from_status)?;

        let inner = response.into_inner();
        let token = inner
            .read_at
            .ok_or_else(|| Error::Serialization("missing read_at token".into()))?
            .try_into()?;
        Ok((inner.schema_text, token))
    }

    /// Writes (upserts) the SpiceDB schema.
    ///
    /// Returns `Err(InvalidArgument)` if the schema string is empty.
    pub async fn write_schema(&self, schema: impl Into<String>) -> Result<ZedToken, Error> {
        let schema = schema.into();
        if schema.is_empty() {
            return Err(Error::InvalidArgument(
                "schema must not be empty".into(),
            ));
        }

        let response = self
            .schema
            .clone()
            .write_schema(proto::WriteSchemaRequest { schema })
            .await
            .map_err(Error::from_status)?;

        let inner = response.into_inner();
        inner
            .written_at
            .ok_or_else(|| Error::Serialization("missing written_at token".into()))?
            .try_into()
    }
}
