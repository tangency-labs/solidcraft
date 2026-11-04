// SPDX-License-Identifier: Apache-2.0
//! Admit each recursive step while building a JSON value.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;

pub(in crate::design) struct TextSeed {
    pub(in crate::design) expected: &'static str,
}

impl<'de> DeserializeSeed<'de> for TextSeed {
    type Value = String;
    fn deserialize<D>(self, deserializer: D) -> Result<String, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for TextSeed {
    type Value = String;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expected)
    }
    fn visit_str<E>(self, text: &str) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Ok(text.to_owned())
    }
}

pub(in crate::design) struct ValueSeed<'a, 'b> {
    pub(in crate::design) ctx: &'a DecodeContext<'b>,
    pub(in crate::design) refusal: &'a mut Option<CodecError>,
}

impl ValueSeed<'_, '_> {
    fn admit<T, E: serde::de::Error>(&mut self, result: Result<T, CodecError>) -> Result<T, E> {
        result.map_err(|error| {
            *self.refusal = Some(error);
            E::custom("configuration JSON resource limit")
        })
    }
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_, '_> {
    type Value = Value;
    fn deserialize<D>(mut self, deserializer: D) -> Result<Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let ctx = self.ctx;
        let _depth = self.admit(ctx.enter_nested("f3d configuration JSON depth"))?;
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ValueSeed<'_, '_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any valid JSON value")
    }
    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_str<E>(self, text: &str) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::String(text.to_owned()))
    }
    fn visit_seq<S>(self, mut sequence: S) -> Result<Value, S::Error>
    where
        S: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(ValueSeed {
            ctx: self.ctx,
            refusal: &mut *self.refusal,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<M>(self, mut map: M) -> Result<Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut fields = serde_json::Map::new();
        while let Some(key) = map.next_key_seed(TextSeed {
            expected: "a string key",
        })? {
            // serde_json's raw-value feature interprets this first-key carrier.
            if fields.is_empty() && key == "$serde_json::private::RawValue" {
                let raw = map.next_value_seed(TextSeed {
                    expected: "raw value",
                })?;
                let mut deserializer = serde_json::Deserializer::from_str(&raw);
                let value = ValueSeed {
                    ctx: self.ctx,
                    refusal: &mut *self.refusal,
                }
                .deserialize(&mut deserializer)
                .map_err(serde::de::Error::custom)?;
                deserializer.end().map_err(serde::de::Error::custom)?;
                return Ok(value);
            }
            let value = map.next_value_seed(ValueSeed {
                ctx: self.ctx,
                refusal: &mut *self.refusal,
            })?;
            // discarded-value: duplicate JSON object keys retain the last value.
            let _ = fields.insert(key, value);
        }
        Ok(Value::Object(fields))
    }
}
