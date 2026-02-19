//! Caveat context value types.

use std::collections::HashMap;

/// A typed value for caveat context evaluation.
///
/// Maps to/from `prost_types::Value` internally.
///
/// # Examples
///
/// ```
/// use prescience::ContextValue;
///
/// let v = ContextValue::String("hello".into());
/// let n = ContextValue::Number(42.0);
/// let b = ContextValue::Bool(true);
/// let list = ContextValue::List(vec![
///     ContextValue::String("a".into()),
///     ContextValue::String("b".into()),
/// ]);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum ContextValue {
    /// JSON null.
    Null,
    /// A boolean value.
    Bool(bool),
    /// A numeric value (f64).
    Number(f64),
    /// A string value.
    String(String),
    /// A list of values.
    List(Vec<ContextValue>),
    /// A nested key-value structure.
    Struct(HashMap<String, ContextValue>),
}

impl From<&ContextValue> for prost_types::Value {
    fn from(cv: &ContextValue) -> Self {
        use prost_types::value::Kind;
        prost_types::Value {
            kind: Some(match cv {
                ContextValue::Null => Kind::NullValue(0),
                ContextValue::Bool(b) => Kind::BoolValue(*b),
                ContextValue::Number(n) => Kind::NumberValue(*n),
                ContextValue::String(s) => Kind::StringValue(s.clone()),
                ContextValue::List(items) => Kind::ListValue(prost_types::ListValue {
                    values: items.iter().map(Into::into).collect(),
                }),
                ContextValue::Struct(fields) => Kind::StructValue(prost_types::Struct {
                    fields: fields
                        .iter()
                        .map(|(k, v)| (k.clone(), v.into()))
                        .collect(),
                }),
            }),
        }
    }
}

impl From<prost_types::Value> for ContextValue {
    fn from(v: prost_types::Value) -> Self {
        match v.kind {
            Some(prost_types::value::Kind::NullValue(_)) => ContextValue::Null,
            Some(prost_types::value::Kind::BoolValue(b)) => ContextValue::Bool(b),
            Some(prost_types::value::Kind::NumberValue(n)) => ContextValue::Number(n),
            Some(prost_types::value::Kind::StringValue(s)) => ContextValue::String(s),
            Some(prost_types::value::Kind::ListValue(list)) => {
                ContextValue::List(list.values.into_iter().map(Into::into).collect())
            }
            Some(prost_types::value::Kind::StructValue(s)) => ContextValue::Struct(
                s.fields
                    .into_iter()
                    .map(|(k, v)| (k, v.into()))
                    .collect(),
            ),
            None => ContextValue::Null,
        }
    }
}

/// Convert a HashMap of ContextValues to a prost_types::Struct.
pub(crate) fn context_to_struct(
    context: &HashMap<String, ContextValue>,
) -> prost_types::Struct {
    prost_types::Struct {
        fields: context
            .iter()
            .map(|(k, v)| (k.clone(), v.into()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_null() {
        let orig = ContextValue::Null;
        let proto: prost_types::Value = (&orig).into();
        let back: ContextValue = proto.into();
        assert_eq!(orig, back);
    }

    #[test]
    fn roundtrip_bool() {
        let orig = ContextValue::Bool(true);
        let proto: prost_types::Value = (&orig).into();
        let back: ContextValue = proto.into();
        assert_eq!(orig, back);
    }

    #[test]
    fn roundtrip_number() {
        let orig = ContextValue::Number(42.5);
        let proto: prost_types::Value = (&orig).into();
        let back: ContextValue = proto.into();
        assert_eq!(orig, back);
    }

    #[test]
    fn roundtrip_string() {
        let orig = ContextValue::String("hello".into());
        let proto: prost_types::Value = (&orig).into();
        let back: ContextValue = proto.into();
        assert_eq!(orig, back);
    }

    #[test]
    fn roundtrip_list() {
        let orig = ContextValue::List(vec![
            ContextValue::Number(1.0),
            ContextValue::String("two".into()),
        ]);
        let proto: prost_types::Value = (&orig).into();
        let back: ContextValue = proto.into();
        assert_eq!(orig, back);
    }

    #[test]
    fn roundtrip_nested_struct() {
        let mut fields = HashMap::new();
        fields.insert("key".into(), ContextValue::Bool(false));
        let orig = ContextValue::Struct(fields);
        let proto: prost_types::Value = (&orig).into();
        let back: ContextValue = proto.into();
        assert_eq!(orig, back);
    }
}
