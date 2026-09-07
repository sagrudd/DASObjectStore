use super::{PlanDenial, MAX_INPUT_BYTES};
use serde::de::{DeserializeOwned, Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("strict JSON")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut map = serde_json::Map::new();
                while let Some((key, value)) = a.next_entry::<String, Strict>()? {
                    if map.insert(key, value.0).is_some() {
                        return Err(A::Error::custom("duplicate key"));
                    }
                }
                Ok(Strict(Value::Object(map)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = a.next_element::<Strict>()? {
                    values.push(value.0);
                }
                Ok(Strict(Value::Array(values)))
            }
            fn visit_str<E: Error>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(Value::String(v.to_owned())))
            }
            fn visit_bool<E: Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(Value::Bool(v)))
            }
            fn visit_u64<E: Error>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(Value::Number(v.into())))
            }
            fn visit_i64<E: Error>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(Value::Number(v.into())))
            }
            fn visit_unit<E: Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
        }
        d.deserialize_any(StrictVisitor)
    }
}

pub(super) fn decode<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T, PlanDenial> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
        return Err(PlanDenial::Encoding);
    }
    let strict: Strict = serde_json::from_slice(bytes).map_err(|_| PlanDenial::Encoding)?;
    let value: T = serde_json::from_value(strict.0.clone()).map_err(|_| PlanDenial::Encoding)?;
    // Existing nested custody types predate deny_unknown_fields. Round-trip
    // equality rejects ignored fields recursively without changing those APIs.
    if serde_json::to_value(&value).map_err(|_| PlanDenial::Encoding)? != strict.0 {
        return Err(PlanDenial::Encoding);
    }
    Ok(value)
}
