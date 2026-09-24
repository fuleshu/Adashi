//! Closed, deterministic wire format. Database rows never supply executable SQL.
use super::*;
use serde::de::{MapAccess, SeqAccess, Visitor};
use std::fmt;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Record {
    pub schema_version: u32,
    pub identity: String,
    pub collection: String,
    pub deleted: bool,
    pub data: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Format {
    pub schema_version: u32,
    pub relational_schema: i64,
}

pub(super) fn bytes(value: &impl Serialize) -> StorageResult<Vec<u8>> {
    // serde_json's default map is ordered; round-tripping also sorts struct keys.
    let value = serde_json::to_value(value).map_err(StorageError::backend)?;
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(StorageError::backend)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// serde_json normally accepts duplicate keys. A merge must not silently choose
/// one of two definitions, including in nested objects.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut values = Vec::new();
                while let Some(v) = a.next_element::<Unique>()? {
                    values.push(v.0);
                }
                Ok(Unique(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((k, v)) = a.next_entry::<String, Unique>()? {
                    if values.insert(k.clone(), v.0).is_some() {
                        return Err(serde::de::Error::custom(format!("duplicate field {k}")));
                    }
                }
                Ok(Unique(Value::Object(values)))
            }
        }
        d.deserialize_any(V)
    }
}

pub(super) fn parse<T: serde::de::DeserializeOwned>(data: &[u8], path: &str) -> StorageResult<T> {
    let text = std::str::from_utf8(data).map_err(|_| invalid(path, "expected UTF-8"))?;
    if text.lines().any(|l| {
        ["<<<<<<<", "=======", ">>>>>>>", "|||||||"]
            .iter()
            .any(|m| l.starts_with(m))
    }) {
        return Err(invalid(path, "unresolved Git conflict markers"));
    }
    let unique: Unique =
        serde_json::from_str(text).map_err(|e| invalid(path, &format!("invalid JSON: {e}")))?;
    serde_json::from_value(unique.0).map_err(|e| invalid(path, &format!("invalid schema: {e}")))
}

pub(super) fn encode(value: &Value) -> Value {
    match value {
        Value::String(s) if s.contains('\n') => {
            serde_json::json!({"lines":s.split_inclusive('\n').collect::<Vec<_>>()})
        }
        _ => value.clone(),
    }
}

pub(super) fn decode(value: &Value, path: &str) -> StorageResult<Value> {
    if let Value::Object(o) = value {
        if o.len() == 1 {
            if let Some(Value::Array(lines)) = o.get("lines") {
                let lines = lines
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .ok_or_else(|| invalid(path, "lines must contain strings"))
                    })
                    .collect::<StorageResult<Vec<_>>>()?;
                return Ok(Value::String(lines.concat()));
            }
        }
        return Err(invalid(
            path,
            "expected a scalar, lines, or a valid foreign-key reference",
        ));
    }
    if value.is_array() {
        return Err(invalid(path, "record fields must be scalars"));
    }
    Ok(value.clone())
}

pub(super) fn valid_identity(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|u| u.hyphenated().to_string() == id)
}

pub(super) fn deterministic_identity(key: &str) -> String {
    let hash = Sha256::digest(key.as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 15) | 0x80;
    bytes[8] = (bytes[8] & 63) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}
