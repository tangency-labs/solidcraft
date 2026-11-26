//! Zero-DOM streaming raw pre-validation for current-version design JSON.
//!
//! Runs the same numeric, depth, anchor, sketch, offset, emboss, wire and body-transfer checks as
//! [`super::check_raw`] + [`super::validate_body_transfers`] directly over the JSON text without
//! building a whole-document `serde_json::Value` tree.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{Value, value::RawValue};

use crate::format::MAX_REAL;
use crate::json_budget::{BorrowedKey, MAX_SMALL_VALUE_BYTES, NumberDepthSeed, check_numbers_str, from_str_bounded};
use crate::{DocError, Result};

/// Keys that [`crate::face_anchor::validate_face_anchors_json`] skips when walking for face anchors.
fn skips_anchors(key: &str) -> bool {
    matches!(key, "sketch" | "meshes" | "step" | "configs" | "appearances" | "materials")
}

/// Validate a current-version (`version >= 10`, no upgrades needed) design JSON string in a single
/// streaming pass without building a whole-document `serde_json::Value`.
pub(super) fn validate_raw_streaming(s: &str) -> Result<()> {
    struct Root<'a> {
        error: &'a mut Option<DocError>,
    }
    impl<'de> Visitor<'de> for Root<'_> {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a SolidCraft design JSON object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
            let mut total_offset_roles = 0usize;
            let mut anchor_visited = 1usize;
            while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
                match key.0.as_ref() {
                    "metadata" | "drawings" => {
                        map.next_value::<IgnoredAny>()?;
                    }
                    "user_materials" => {
                        map.next_value_seed(UserMaterialsSeed { error: self.error, anchor_visited: &mut anchor_visited })?;
                    }
                    "configs" => {
                        map.next_value_seed(ConfigsSeed { error: self.error })?;
                    }
                    "appearances" => {
                        map.next_value_seed(AppearancesSeed { error: self.error })?;
                    }
                    "features" => {
                        map.next_value_seed(FeaturesSeed {
                            error: self.error,
                            total_offset_roles: &mut total_offset_roles,
                            anchor_visited: &mut anchor_visited,
                        })?;
                    }
                    "materials" => {
                        map.next_value_seed(NumberDepthSeed { noun: "design", depth: 1, error: self.error })?;
                    }
                    _ => {
                        map.next_value_seed(NumberDepthAnchorSeed {
                            depth: 1,
                            check_anchors: true,
                            anchor_visited: &mut anchor_visited,
                            error: self.error,
                        })?;
                    }
                }
            }
            Ok(())
        }
    }
    let mut error = None;
    let mut de = serde_json::Deserializer::from_str(s);
    let res = de.deserialize_map(Root { error: &mut error }).and_then(|()| de.end());
    if let Some(err) = error {
        return Err(err);
    }
    res.map_err(|e| DocError::Invalid(format!("not a SolidCraft design: {e}")))
}

fn fail<E: de::Error>(slot: &mut Option<DocError>, err: DocError) -> E {
    *slot = Some(err);
    E::custom("validation failed")
}

/// Streaming visitor enforcing depth `<= 64`, float range `<= MAX_REAL`, and face anchor validity.
struct NumberDepthAnchorSeed<'a> {
    depth: usize,
    check_anchors: bool,
    anchor_visited: &'a mut usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for NumberDepthAnchorSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        if self.depth > 64 {
            return Err(fail(self.error, DocError::Invalid("damaged design: nested too deeply".into())));
        }
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for NumberDepthAnchorSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any valid JSON value")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        if self.check_anchors {
            *self.anchor_visited = self.anchor_visited.saturating_add(1);
            if *self.anchor_visited > 2_000_000 {
                return Err(fail(self.error, DocError::Invalid("face anchor structure exceeds work limits".into())));
            }
        }
        while seq
            .next_element_seed(NumberDepthAnchorSeed {
                depth: self.depth + 1,
                check_anchors: self.check_anchors,
                anchor_visited: self.anchor_visited,
                error: self.error,
            })?
            .is_some()
        {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        if self.check_anchors {
            *self.anchor_visited = self.anchor_visited.saturating_add(1);
            if *self.anchor_visited > 2_000_000 {
                return Err(fail(self.error, DocError::Invalid("face anchor structure exceeds work limits".into())));
            }
        }
        let mut is_face_type = false;
        let mut raw_anchor: Option<&'de RawValue> = None;
        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            let k = key.0.as_ref();
            if self.check_anchors && k == "type" {
                let raw = map.next_value::<&'de RawValue>()?;
                if let Err(err) = check_slice_numbers_and_anchors(raw.get(), self.depth + 1, true, self.anchor_visited) {
                    return Err(fail(self.error, err));
                }
                is_face_type = serde_json::from_str::<String>(raw.get()).is_ok_and(|s| s == "face");
            } else if self.check_anchors && k == "anchor" {
                let raw = map.next_value::<&'de RawValue>()?;
                if let Err(err) = check_slice_numbers_and_anchors(raw.get(), self.depth + 1, true, self.anchor_visited) {
                    return Err(fail(self.error, err));
                }
                raw_anchor = Some(raw);
            } else {
                let child_anchors = self.check_anchors && !skips_anchors(k);
                map.next_value_seed(NumberDepthAnchorSeed {
                    depth: self.depth + 1,
                    check_anchors: child_anchors,
                    anchor_visited: self.anchor_visited,
                    error: self.error,
                })?;
            }
        }
        if is_face_type && let Some(raw) = raw_anchor && raw.get() != "null" {
            let anchor = from_str_bounded(raw.get(), 64 * 1024)
                .map_err(|_| fail::<A::Error>(self.error, DocError::Invalid("face anchor must be an object".into())))?;
            if let Err(err) = crate::face_anchor::validate_face_anchor_json(&anchor) {
                return Err(fail(self.error, err));
            }
        }
        Ok(())
    }
}

fn check_slice_numbers_and_anchors(s: &str, depth: usize, check_anchors: bool, anchor_visited: &mut usize) -> Result<()> {
    let mut error = None;
    let mut de = serde_json::Deserializer::from_str(s);
    let res = NumberDepthAnchorSeed { depth, check_anchors, anchor_visited, error: &mut error }
        .deserialize(&mut de)
        .and_then(|()| de.end());
    if let Some(err) = error {
        return Err(err);
    }
    res.map_err(|e| DocError::Invalid(format!("damaged design: {e}")))
}

struct UserMaterialsSeed<'a> {
    error: &'a mut Option<DocError>,
    anchor_visited: &'a mut usize,
}

impl<'de> DeserializeSeed<'de> for UserMaterialsSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for UserMaterialsSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("user_materials array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(), A::Error> {
        NumberDepthAnchorSeed { depth: 1, check_anchors: true, anchor_visited: self.anchor_visited, error: self.error }.visit_map(map)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        let mut count = 0usize;
        while seq.next_element_seed(UserMaterialItemSeed { error: self.error, anchor_visited: self.anchor_visited })?.is_some() {
            count += 1;
            if count > crate::MAX_USER_MATERIALS {
                return Err(fail(self.error, DocError::Invalid("document exceeds 1000 user materials".into())));
            }
        }
        Ok(())
    }
}

struct UserMaterialItemSeed<'a> {
    error: &'a mut Option<DocError>,
    anchor_visited: &'a mut usize,
}

impl<'de> DeserializeSeed<'de> for UserMaterialItemSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for UserMaterialItemSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("user material entry")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<(), A::Error> {
        NumberDepthAnchorSeed { depth: 2, check_anchors: true, anchor_visited: self.anchor_visited, error: self.error }.visit_seq(seq)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            if key.0.as_ref() == "physical_card" {
                let raw = map.next_value::<&'de RawValue>()?;
                if raw.get() != "null" {
                    let card = from_str_bounded(raw.get(), MAX_SMALL_VALUE_BYTES)
                        .map_err(|_| fail::<A::Error>(self.error, DocError::Invalid("material card exceeds JSON node/depth budget".into())))?;
                    if let Err(err) = crate::physical_material::validate_card(&card) {
                        return Err(fail(self.error, err));
                    }
                }
            } else {
                map.next_value_seed(NumberDepthAnchorSeed {
                    depth: 3,
                    check_anchors: !skips_anchors(key.0.as_ref()),
                    anchor_visited: self.anchor_visited,
                    error: self.error,
                })?;
            }
        }
        Ok(())
    }
}

struct ConfigsSeed<'a> {
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for ConfigsSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ConfigsSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("configs object")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 1, error: self.error }.visit_seq(seq)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        let mut raw_rules: Option<&'de RawValue> = None;
        let mut raw_columns: Option<&'de RawValue> = None;
        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            match key.0.as_ref() {
                "rules" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 2) {
                        return Err(fail(self.error, err));
                    }
                    raw_rules = Some(raw);
                }
                "columns" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 2) {
                        return Err(fail(self.error, err));
                    }
                    raw_columns = Some(raw);
                }
                _ => {
                    map.next_value_seed(NumberDepthSeed { noun: "design", depth: 2, error: self.error })?;
                }
            }
        }
        if let Some(raw) = raw_rules {
            let val = from_str_bounded(raw.get(), MAX_SMALL_VALUE_BYTES)
                .map_err(|_| fail::<A::Error>(self.error, DocError::Invalid("configuration rules: too many rules".into())))?;
            if let Err(err) = crate::config::validate_rules_value(&val) {
                return Err(fail(self.error, err));
            }
        }
        if let Some(raw) = raw_columns {
            let val = from_str_bounded(raw.get(), MAX_SMALL_VALUE_BYTES)
                .map_err(|_| fail::<A::Error>(self.error, DocError::Invalid("too many configuration columns".into())))?;
            if let Err(err) = crate::config::validate_columns_value(&val) {
                return Err(fail(self.error, err));
            }
        }
        Ok(())
    }
}

struct AppearancesSeed<'a> {
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for AppearancesSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for AppearancesSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("appearances object")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 1, error: self.error }.visit_seq(seq)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            if key.0.as_ref() == "faces" {
                map.next_value_seed(AppearanceFacesSeed { error: self.error })?;
            } else {
                map.next_value_seed(NumberDepthSeed { noun: "design", depth: 2, error: self.error })?;
            }
        }
        Ok(())
    }
}

struct AppearanceFacesSeed<'a> {
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for AppearanceFacesSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for AppearanceFacesSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("appearances.faces array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 2, error: self.error }.visit_map(map)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        while let Some(raw) = seq.next_element::<&'de RawValue>()? {
            if let Err(err) = check_numbers_str(raw.get(), "design", 3) {
                return Err(fail(self.error, err));
            }
            if raw.get().contains("certificate") || raw.get().contains("\\u") {
                let val = from_str_bounded(raw.get(), MAX_SMALL_VALUE_BYTES)
                    .map_err(|_| fail::<A::Error>(self.error, DocError::Invalid("invalid certified face appearance".into())))?;
                if let Err(err) = crate::config::validate_face_appearance_value(&val) {
                    return Err(fail(self.error, err));
                }
            }
        }
        Ok(())
    }
}

struct FeaturesSeed<'a> {
    error: &'a mut Option<DocError>,
    total_offset_roles: &'a mut usize,
    anchor_visited: &'a mut usize,
}

impl<'de> DeserializeSeed<'de> for FeaturesSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for FeaturesSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("features array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(), A::Error> {
        NumberDepthAnchorSeed { depth: 1, check_anchors: true, anchor_visited: self.anchor_visited, error: self.error }.visit_map(map)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        *self.anchor_visited = self.anchor_visited.saturating_add(1);
        if *self.anchor_visited > 2_000_000 {
            return Err(fail(self.error, DocError::Invalid("face anchor structure exceeds work limits".into())));
        }
        let mut count = 0usize;
        while seq
            .next_element_seed(FeatureItemSeed {
                error: self.error,
                total_offset_roles: self.total_offset_roles,
                anchor_visited: self.anchor_visited,
            })?
            .is_some()
        {
            count += 1;
            if count > crate::document::MAX_FEATURES {
                return Err(fail(self.error, DocError::Invalid("document too large".into())));
            }
        }
        Ok(())
    }
}

struct FeatureItemSeed<'a> {
    error: &'a mut Option<DocError>,
    total_offset_roles: &'a mut usize,
    anchor_visited: &'a mut usize,
}

impl<'de> DeserializeSeed<'de> for FeatureItemSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for FeatureItemSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a feature object")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<(), A::Error> {
        NumberDepthAnchorSeed { depth: 2, check_anchors: true, anchor_visited: self.anchor_visited, error: self.error }.visit_seq(seq)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        *self.anchor_visited = self.anchor_visited.saturating_add(1);
        if *self.anchor_visited > 2_000_000 {
            return Err(fail(self.error, DocError::Invalid("face anchor structure exceeds work limits".into())));
        }
        let mut feature_type: Option<String> = None;
        let mut raw_anchor: Option<&'de RawValue> = None;
        let mut plane: Option<&'de RawValue> = None;
        let mut face: Option<&'de RawValue> = None;
        let mut profiles: Option<&'de RawValue> = None;
        let mut sections: Option<&'de RawValue> = None;
        let mut convention: Option<&'de RawValue> = None;
        let mut bodies: Option<&'de RawValue> = None;
        let mut from: Option<&'de RawValue> = None;
        let mut to: Option<&'de RawValue> = None;
        let mut component: Option<&'de RawValue> = None;
        let mut transform: Option<&'de RawValue> = None;
        let mut sketch: Option<SketchSummary<'de>> = None;

        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            let k = key.0.as_ref();
            match k {
                "type" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_slice_numbers_and_anchors(raw.get(), 3, true, self.anchor_visited) {
                        return Err(fail(self.error, err));
                    }
                    feature_type = serde_json::from_str::<String>(raw.get()).ok();
                }
                "anchor" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_slice_numbers_and_anchors(raw.get(), 3, true, self.anchor_visited) {
                        return Err(fail(self.error, err));
                    }
                    raw_anchor = Some(raw);
                }
                "plane" | "face" | "profiles" | "sections" | "convention" | "bodies" | "from" | "to" | "component" | "transform" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_slice_numbers_and_anchors(raw.get(), 3, true, self.anchor_visited) {
                        return Err(fail(self.error, err));
                    }
                    match k {
                        "plane" => plane = Some(raw),
                        "face" => face = Some(raw),
                        "profiles" => profiles = Some(raw),
                        "sections" => sections = Some(raw),
                        "convention" => convention = Some(raw),
                        "bodies" => bodies = Some(raw),
                        "from" => from = Some(raw),
                        "to" => to = Some(raw),
                        "component" => component = Some(raw),
                        "transform" => transform = Some(raw),
                        _ => {}
                    }
                }
                "sketch" => {
                    sketch = Some(map.next_value_seed(SketchSeed { error: self.error })?);
                }
                "step" | "meshes" => {
                    map.next_value_seed(NumberDepthSeed { noun: "design", depth: 3, error: self.error })?;
                }
                _ => {
                    map.next_value_seed(NumberDepthAnchorSeed {
                        depth: 3,
                        check_anchors: !skips_anchors(k),
                        anchor_visited: self.anchor_visited,
                        error: self.error,
                    })?;
                }
            }
        }

        if feature_type.as_deref() == Some("face") && let Some(raw) = raw_anchor && raw.get() != "null" {
            let anchor = from_str_bounded(raw.get(), 64 * 1024)
                .map_err(|_| fail::<A::Error>(self.error, DocError::Invalid("face anchor must be an object".into())))?;
            if let Err(err) = crate::face_anchor::validate_face_anchor_json(&anchor) {
                return Err(fail(self.error, err));
            }
        }

        match feature_type.as_deref() {
            Some("revolve") => {
                if let Some(raw) = convention
                    && !serde_json::from_str::<String>(raw.get()).is_ok_and(|c| matches!(c.as_str(), "legacy_axis_start" | "sketch_normal"))
                {
                    return Err(fail(self.error, DocError::Invalid("damaged design: invalid periodic spline or revolve convention".into())));
                }
            }
            Some("emboss") => {
                if let Some(raw) = face && raw.get() != "null" {
                    let target = from_str_bounded(raw.get(), MAX_SMALL_VALUE_BYTES).map_err(|_| {
                        fail::<A::Error>(
                            self.error,
                            DocError::Invalid("damaged design: an emboss face needs a bounded body name and three finite coordinates".into()),
                        )
                    })?;
                    if let Err(err) = super::validate_emboss_face(&target) {
                        return Err(fail(self.error, err));
                    }
                }
            }
            Some("body_transfer") => {
                if let Err(err) = validate_body_transfer_slices(bodies, from, to, component, transform) {
                    return Err(fail(self.error, err));
                }
            }
            Some("sketch") => {
                if let Some(summary) = sketch {
                    if let Err(err) = summary.finish(plane, self.total_offset_roles) {
                        return Err(fail(self.error, err));
                    }
                } else if let Some(raw) = plane
                    && let Err(err) = validate_plane_placement_str(raw.get())
                {
                    return Err(fail(self.error, err));
                }
            }
            _ => {}
        }

        if let Some(raw) = profiles
            && let Err(err) = validate_profile_slice(raw.get())
        {
            return Err(fail(self.error, err));
        }
        if let Some(raw) = sections
            && let Err(err) = validate_sections_slice(raw.get())
        {
            return Err(fail(self.error, err));
        }
        Ok(())
    }
}

fn validate_profile_slice(s: &str) -> Result<()> {
    if !s.contains("whole_boundaries") && !s.contains("offset_boundaries") && !s.contains("\\u") {
        return Ok(());
    }
    let val = from_str_bounded(s, MAX_SMALL_VALUE_BYTES)
        .map_err(|_| DocError::Invalid("damaged design: invalid analytic offset topology or boundary reference".into()))?;
    crate::format_offsets::validate_profile_selection(&val)
}

fn validate_sections_slice(s: &str) -> Result<()> {
    struct SectionsSeed<'a>(&'a mut Option<DocError>);
    impl<'de> DeserializeSeed<'de> for SectionsSeed<'_> {
        type Value = ();
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
            d.deserialize_any(self)
        }
    }
    impl<'de> Visitor<'de> for SectionsSeed<'_> {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("sections array")
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
            let mut count = 0usize;
            while seq.next_element_seed(SectionItemSeed(self.0))?.is_some() {
                count += 1;
                if count > 10_000 {
                    return Err(fail(self.0, DocError::Invalid("damaged design: invalid analytic offset topology or boundary reference".into())));
                }
            }
            Ok(())
        }
    }
    struct SectionItemSeed<'a>(&'a mut Option<DocError>);
    impl<'de> DeserializeSeed<'de> for SectionItemSeed<'_> {
        type Value = ();
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
            d.deserialize_any(self)
        }
    }
    impl<'de> Visitor<'de> for SectionItemSeed<'_> {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("section object")
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(())
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
            while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
                if key.0.as_ref() == "profiles" {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = validate_profile_slice(raw.get()) {
                        return Err(fail(self.0, err));
                    }
                } else {
                    map.next_value::<IgnoredAny>()?;
                }
            }
            Ok(())
        }
    }
    let mut error = None;
    let mut de = serde_json::Deserializer::from_str(s);
    let _ = SectionsSeed(&mut error).deserialize(&mut de);
    if let Some(err) = error {
        return Err(err);
    }
    Ok(())
}

fn validate_plane_placement_str(s: &str) -> Result<()> {
    let invalid = || DocError::Invalid("damaged design: invalid captured construction plane placement".into());
    let mut current = Some(s);
    let mut depth = 0usize;
    while let Some(slice) = current {
        depth += 1;
        if depth > 64 {
            return Err(invalid());
        }
        #[derive(serde::Deserialize)]
        struct PlaneHeader<'a> {
            #[serde(rename = "type", borrow, default)]
            kind: Option<&'a RawValue>,
            #[serde(borrow, default)]
            placement: Option<&'a RawValue>,
            #[serde(borrow, default)]
            base: Option<&'a RawValue>,
        }
        let Ok(header) = serde_json::from_str::<PlaneHeader<'_>>(slice) else {
            return Ok(());
        };
        if header.kind.is_some_and(|k| serde_json::from_str::<String>(k.get()).is_ok_and(|s| s == "construction"))
            && let Some(placement) = header.placement.filter(|p| p.get() != "null")
        {
            let mat: crate::Mat = serde_json::from_str(placement.get()).map_err(|_| invalid())?;
            if !crate::is_placement(&mat) {
                return Err(invalid());
            }
        }
        current = header.base.map(RawValue::get);
    }
    Ok(())
}

fn validate_body_transfer_slices(
    bodies: Option<&RawValue>,
    from: Option<&RawValue>,
    to: Option<&RawValue>,
    component: Option<&RawValue>,
    transform: Option<&RawValue>,
) -> Result<()> {
    let invalid = || DocError::Invalid("damaged design: body transfer needs bounded unique bodies, components and a rigid placement".into());
    let bodies_raw = bodies.ok_or_else(invalid)?;
    if bodies_raw.get().len() > 1_000_000 {
        return Err(invalid());
    }
    let bodies_list: Vec<String> = serde_json::from_str(bodies_raw.get()).map_err(|_| invalid())?;
    if bodies_list.is_empty() || bodies_list.len() > 1000 {
        return Err(invalid());
    }
    let mut names = BTreeSet::new();
    for body in &bodies_list {
        if body.trim().is_empty() || body.len() > 512 || body.chars().any(char::is_control) || !names.insert(body.as_str()) {
            return Err(invalid());
        }
    }
    let _from: u64 = serde_json::from_str(from.ok_or_else(invalid)?.get()).map_err(|_| invalid())?;
    let to_id: u64 = serde_json::from_str(to.ok_or_else(invalid)?.get()).map_err(|_| invalid())?;
    let owner: u64 = match component {
        None => 0,
        Some(raw) => serde_json::from_str(raw.get()).map_err(|_| invalid())?,
    };
    if to_id != owner {
        return Err(DocError::Invalid("damaged design: body transfer owner must match its destination".into()));
    }
    let placement: crate::Mat = serde_json::from_str(transform.ok_or_else(invalid)?.get()).map_err(|_| invalid())?;
    if !crate::body_transfer::valid_transfer_matrix(&placement) {
        return Err(invalid());
    }
    Ok(())
}

struct PeriodicSplineInfo {
    max_index: Option<u64>,
    bad_index: bool,
}

#[derive(Default)]
struct SketchSummary<'de> {
    has_points: bool,
    points_len: usize,
    curves_periodic: Vec<(String, bool)>,
    periodic_splines: Vec<PeriodicSplineInfo>,
    versioned_err: Option<DocError>,
    size_err: Option<DocError>,
    wires_err: Option<DocError>,
    offsets: Option<&'de RawValue>,
    freeform_offsets: Option<&'de RawValue>,
    freeform_chain_offsets: Option<&'de RawValue>,
}

impl SketchSummary<'_> {
    fn finish(self, plane: Option<&RawValue>, total_offset_roles: &mut usize) -> Result<()> {
        // 1. validate_versioned_geometry
        if let Some(err) = self.versioned_err {
            return Err(err);
        }
        for spline in &self.periodic_splines {
            let invalid = || DocError::Invalid("damaged design: invalid periodic spline or revolve convention".into());
            if !self.has_points || spline.bad_index || spline.max_index.is_some_and(|idx| idx >= self.points_len as u64) {
                return Err(invalid());
            }
        }
        // 2. validate_freeform_offsets
        if let Some(raw) = self.freeform_offsets {
            let records = from_str_bounded(raw.get(), MAX_SMALL_VALUE_BYTES)
                .map_err(|_| DocError::Invalid("damaged design: invalid free-form offset metadata".into()))?;
            validate_freeform_offsets_from_summary(&records, &self.curves_periodic)?;
        }
        // 3. validate_freeform_chain_offsets
        if let Some(raw) = self.freeform_chain_offsets {
            let records = from_str_bounded(raw.get(), MAX_SMALL_VALUE_BYTES)
                .map_err(|_| DocError::Invalid("damaged design: invalid trimmed chain offset metadata".into()))?;
            super::validate_freeform_chain_offsets_value(&records)?;
        }
        // 4. validate_sketch_sizes
        if let Some(raw) = plane {
            validate_plane_placement_str(raw.get())?;
        }
        if let Some(err) = self.size_err {
            return Err(err);
        }
        // 5. format_offsets
        if let Some(raw) = self.offsets {
            let offsets = from_str_bounded(raw.get(), MAX_SMALL_VALUE_BYTES)
                .map_err(|_| DocError::Invalid("damaged design: invalid analytic offset topology or boundary reference".into()))?;
            crate::format_offsets::validate_sketch_offsets(&offsets, total_offset_roles)?;
        }
        // 6. validate_wires
        if let Some(err) = self.wires_err {
            return Err(err);
        }
        Ok(())
    }
}

fn validate_freeform_offsets_from_summary(records: &Value, curves_periodic: &[(String, bool)]) -> Result<()> {
    let invalid = || DocError::Invalid("damaged design: invalid free-form offset metadata".into());
    let records = records.as_array().ok_or_else(invalid)?;
    if records.len() > 128 {
        return Err(invalid());
    }
    let periodic_outputs: BTreeSet<&str> = records
        .iter()
        .filter(|record| record.get("certificate").and_then(Value::as_u64) == Some(2))
        .filter_map(|record| record.get("output").and_then(Value::as_str))
        .collect();
    let mut periodic_curves: BTreeMap<&str, bool> = BTreeMap::new();
    if !periodic_outputs.is_empty() {
        for (id, is_periodic_spline) in curves_periodic {
            if periodic_outputs.contains(id.as_str()) && periodic_curves.insert(id.as_str(), *is_periodic_spline).is_some() {
                return Err(invalid());
            }
        }
    }
    let mut outputs = BTreeSet::new();
    for record in records {
        let object = record.as_object().ok_or_else(invalid)?;
        if object.len() != 9
            || object.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "source" | "output" | "forward" | "distance" | "divisions" | "pieces" | "closed" | "tolerance" | "certificate"
                )
            })
        {
            return Err(invalid());
        }
        let source = record.get("source").and_then(Value::as_str).ok_or_else(invalid)?;
        let output = record.get("output").and_then(Value::as_str).ok_or_else(invalid)?;
        if [source, output].iter().any(|id| id.is_empty() || id.len() > 512 || id.chars().any(char::is_control))
            || source == output
            || !outputs.insert(output)
        {
            return Err(invalid());
        }
        let divisions = record.get("divisions").and_then(Value::as_u64).ok_or_else(invalid)?;
        let pieces = record.get("pieces").and_then(Value::as_u64).ok_or_else(invalid)?;
        let distance = record.get("distance").and_then(Value::as_f64).ok_or_else(invalid)?;
        let tolerance = record.get("tolerance").and_then(Value::as_f64).ok_or_else(invalid)?;
        if !(1..=256).contains(&divisions)
            || !divisions.is_power_of_two()
            || !(1..=256).contains(&pieces)
            || pieces * divisions >= solidcraft_sketch::MAX_SPLINE_POINTS as u64
            || !distance.is_finite()
            || !(1e-9..=MAX_REAL).contains(&distance.abs())
            || !tolerance.is_finite()
            || !(tolerance > 0.0 && tolerance <= 0.002)
            || !matches!(record.get("certificate").and_then(Value::as_u64), Some(1 | 2))
            || record.get("forward").and_then(Value::as_bool).is_none()
            || record.get("closed").and_then(Value::as_bool).is_none()
        {
            return Err(invalid());
        }
        if record.get("certificate").and_then(Value::as_u64) == Some(2)
            && (record.get("closed").and_then(Value::as_bool) != Some(true) || periodic_curves.get(output) != Some(&true))
        {
            return Err(invalid());
        }
    }
    let mut resolved = BTreeSet::new();
    for record in records {
        let source = record.get("source").and_then(Value::as_str).ok_or_else(invalid)?;
        let output = record.get("output").and_then(Value::as_str).ok_or_else(invalid)?;
        if outputs.contains(source) && !resolved.contains(source) {
            return Err(invalid());
        }
        resolved.insert(output);
    }
    Ok(())
}

struct SketchSeed<'a> {
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for SketchSeed<'_> {
    type Value = SketchSummary<'de>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<SketchSummary<'de>, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for SketchSeed<'_> {
    type Value = SketchSummary<'de>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sketch object")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<SketchSummary<'de>, E> {
        Ok(SketchSummary::default())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<SketchSummary<'de>, E> {
        Ok(SketchSummary::default())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<SketchSummary<'de>, E> {
        Ok(SketchSummary::default())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<SketchSummary<'de>, E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(SketchSummary::default())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<SketchSummary<'de>, E> {
        Ok(SketchSummary::default())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<SketchSummary<'de>, E> {
        Ok(SketchSummary::default())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<SketchSummary<'de>, E> {
        Ok(SketchSummary::default())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<SketchSummary<'de>, A::Error> {
        NumberDepthSeed { noun: "design", depth: 3, error: self.error }.visit_seq(seq)?;
        Ok(SketchSummary::default())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<SketchSummary<'de>, A::Error> {
        let mut summary = SketchSummary::default();
        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            match key.0.as_ref() {
                "points" => {
                    let (is_arr, len) = map.next_value_seed(CountArraySeed { depth: 4, error: self.error })?;
                    if is_arr {
                        summary.has_points = true;
                        summary.points_len = len;
                        if len > solidcraft_sketch::CEILING_POINTS && summary.size_err.is_none() {
                            summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
                        }
                    }
                }
                "constraints" | "wire_constraints" => {
                    let (is_arr, len) = map.next_value_seed(CountArraySeed { depth: 4, error: self.error })?;
                    if is_arr && len > solidcraft_sketch::CEILING_CONSTRAINTS && summary.size_err.is_none() {
                        summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
                    }
                }
                "curves" => {
                    map.next_value_seed(SketchCurvesSeed { summary: &mut summary, error: self.error })?;
                }
                "patterns" => {
                    map.next_value_seed(SketchPatternsSeed { summary: &mut summary, error: self.error })?;
                }
                "links" => {
                    map.next_value_seed(SketchLinksSeed { summary: &mut summary, error: self.error })?;
                }
                "wires" => {
                    map.next_value_seed(SketchWiresSeed { summary: &mut summary, error: self.error })?;
                }
                "offsets" | "freeform_offsets" | "freeform_chain_offsets" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 4) {
                        return Err(fail(self.error, err));
                    }
                    match key.0.as_ref() {
                        "offsets" => summary.offsets = Some(raw),
                        "freeform_offsets" => summary.freeform_offsets = Some(raw),
                        "freeform_chain_offsets" => summary.freeform_chain_offsets = Some(raw),
                        _ => {}
                    }
                }
                _ => {
                    map.next_value_seed(NumberDepthSeed { noun: "design", depth: 4, error: self.error })?;
                }
            }
        }
        Ok(summary)
    }
}

/// Counts array length while streaming number/depth checks, allocating zero `Value`s.
struct CountArraySeed<'a> {
    depth: usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for CountArraySeed<'_> {
    type Value = (bool, usize);
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(bool, usize), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for CountArraySeed<'_> {
    type Value = (bool, usize);
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(bool, usize), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok((false, 0))
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(bool, usize), A::Error> {
        NumberDepthSeed { noun: "design", depth: self.depth, error: self.error }.visit_map(map)?;
        Ok((false, 0))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(bool, usize), A::Error> {
        let mut count = 0usize;
        while seq.next_element_seed(NumberDepthSeed { noun: "design", depth: self.depth + 1, error: self.error })?.is_some() {
            count += 1;
        }
        Ok((true, count))
    }
}

struct SketchCurvesSeed<'a, 'de> {
    summary: &'a mut SketchSummary<'de>,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for SketchCurvesSeed<'_, 'de> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for SketchCurvesSeed<'_, 'de> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sketch.curves array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 4, error: self.error }.visit_map(map)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        let mut curve_count = 0usize;
        let mut point_references = 0usize;
        let mut periodic_work = 0usize;
        while seq
            .next_element_seed(CurveItemSeed {
                summary: self.summary,
                point_references: &mut point_references,
                periodic_work: &mut periodic_work,
                record_id: curve_count <= solidcraft_sketch::CEILING_CURVES,
                error: self.error,
            })?
            .is_some()
        {
            curve_count += 1;
            if curve_count > solidcraft_sketch::CEILING_CURVES && self.summary.size_err.is_none() {
                self.summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
            }
        }
        Ok(())
    }
}

struct CurveItemSeed<'a, 'de> {
    summary: &'a mut SketchSummary<'de>,
    point_references: &'a mut usize,
    periodic_work: &'a mut usize,
    record_id: bool,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for CurveItemSeed<'_, 'de> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for CurveItemSeed<'_, 'de> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("curve object")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 5, error: self.error }.visit_seq(seq)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        let mut id: Option<String> = None;
        let mut kind: Option<String> = None;
        let mut periodic: Option<Option<bool>> = None;
        let mut control: Option<Option<bool>> = None;
        let mut degree: Option<Option<u8>> = None;
        let mut pts_info: Option<(bool, usize, Option<u64>, bool)> = None;
        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            match key.0.as_ref() {
                "id" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 6) {
                        return Err(fail(self.error, err));
                    }
                    id = serde_json::from_str::<String>(raw.get()).ok();
                }
                "type" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 6) {
                        return Err(fail(self.error, err));
                    }
                    kind = serde_json::from_str::<String>(raw.get()).ok();
                }
                "periodic" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 6) {
                        return Err(fail(self.error, err));
                    }
                    periodic = Some(serde_json::from_str::<bool>(raw.get()).ok());
                }
                "control" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 6) {
                        return Err(fail(self.error, err));
                    }
                    control = Some(serde_json::from_str::<bool>(raw.get()).ok());
                }
                "degree" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 6) {
                        return Err(fail(self.error, err));
                    }
                    degree = Some(serde_json::from_str::<u64>(raw.get()).ok().and_then(|d| u8::try_from(d).ok()));
                }
                "pts" => {
                    let info = map.next_value_seed(IndexArraySeed { depth: 6, error: self.error })?;
                    if !info.0 {
                        if self.summary.size_err.is_none() {
                            self.summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
                        }
                    } else {
                        *self.point_references = self.point_references.saturating_add(info.1);
                        if (info.1 > solidcraft_sketch::MAX_SPLINE_POINTS || *self.point_references > 1_000_000) && self.summary.size_err.is_none() {
                            self.summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
                        }
                    }
                    pts_info = Some(info);
                }
                "handles" => {
                    let (is_arr, len) = map.next_value_seed(CountArraySeed { depth: 6, error: self.error })?;
                    if !is_arr {
                        if self.summary.size_err.is_none() {
                            self.summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
                        }
                    } else {
                        *self.point_references = self.point_references.saturating_add(len);
                        if (len > solidcraft_sketch::MAX_SPLINE_POINTS || *self.point_references > 1_000_000) && self.summary.size_err.is_none() {
                            self.summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
                        }
                    }
                }
                "spline_handle_of" | "spline_control_polygon_of" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 6) {
                        return Err(fail(self.error, err));
                    }
                    if serde_json::from_str::<String>(raw.get()).is_ok_and(|s| s.len() > 512) && self.summary.size_err.is_none() {
                        self.summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
                    }
                }
                _ => {
                    map.next_value_seed(NumberDepthSeed { noun: "design", depth: 6, error: self.error })?;
                }
            }
        }
        let is_spline = kind.as_deref() == Some("spline");
        let is_periodic_spline = is_spline && periodic == Some(Some(true));
        if self.record_id
            && let Some(curve_id) = id
        {
            self.summary.curves_periodic.push((curve_id, is_periodic_spline));
        }
        if is_spline && let Some(marker) = periodic {
            let invalid = || DocError::Invalid("damaged design: invalid periodic spline or revolve convention".into());
            match marker {
                None => {
                    if self.summary.versioned_err.is_none() {
                        self.summary.versioned_err = Some(invalid());
                    }
                }
                Some(false) => {}
                Some(true) => {
                    let ok = (|| -> Result<PeriodicSplineInfo> {
                        let (true, count, max_index, bad_index) = pts_info.ok_or_else(invalid)? else {
                            return Err(invalid());
                        };
                        *self.periodic_work = self.periodic_work.saturating_add(count);
                        let is_control = match control {
                            None => false,
                            Some(Some(b)) => b,
                            Some(None) => return Err(invalid()),
                        };
                        let deg = match degree {
                            None => 3,
                            Some(Some(d)) => d,
                            Some(None) => return Err(invalid()),
                        };
                        if *self.periodic_work > 1_000_000 {
                            return Err(invalid());
                        }
                        if is_control {
                            solidcraft_sketch::validate_closed_control(count, deg).map_err(|_| invalid())?;
                        } else if !(4..=solidcraft_sketch::MAX_SPLINE_POINTS).contains(&count) || deg != 3 {
                            return Err(invalid());
                        }
                        Ok(PeriodicSplineInfo { max_index, bad_index })
                    })();
                    match ok {
                        Ok(info) => self.summary.periodic_splines.push(info),
                        Err(err) => {
                            if self.summary.versioned_err.is_none() {
                                self.summary.versioned_err = Some(err);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Stream-scans an array of point indices: returns `(is_array, len, max_u64, any_non_u64)`.
struct IndexArraySeed<'a> {
    depth: usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for IndexArraySeed<'_> {
    type Value = (bool, usize, Option<u64>, bool);
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for IndexArraySeed<'_> {
    type Value = (bool, usize, Option<u64>, bool);
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("index array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
        Ok((false, 0, None, false))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
        Ok((false, 0, None, false))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
        Ok((false, 0, None, false))
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<Self::Value, E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok((false, 0, None, false))
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
        Ok((false, 0, None, false))
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok((false, 0, None, false))
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok((false, 0, None, false))
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<Self::Value, A::Error> {
        NumberDepthSeed { noun: "design", depth: self.depth, error: self.error }.visit_map(map)?;
        Ok((false, 0, None, false))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
        let mut len = 0usize;
        let mut max_idx: Option<u64> = None;
        let mut bad = false;
        while let Some(item) = seq.next_element_seed(U64ElemSeed { depth: self.depth + 1, error: self.error })? {
            len += 1;
            match item {
                Some(idx) => max_idx = Some(max_idx.map_or(idx, |m| m.max(idx))),
                None => bad = true,
            }
        }
        Ok((true, len, max_idx, bad))
    }
}

struct U64ElemSeed<'a> {
    depth: usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for U64ElemSeed<'_> {
    type Value = Option<u64>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Option<u64>, D::Error> {
        if self.depth > 64 {
            return Err(fail(self.error, DocError::Invalid("damaged design: nested too deeply".into())));
        }
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for U64ElemSeed<'_> {
    type Value = Option<u64>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("u64 index")
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Option<u64>, E> {
        Ok(Some(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Option<u64>, E> {
        Ok(u64::try_from(v).ok())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<Option<u64>, E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(None)
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Option<u64>, E> {
        Ok(None)
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Option<u64>, E> {
        Ok(None)
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Option<u64>, E> {
        Ok(None)
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<Option<u64>, E> {
        Ok(None)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<Option<u64>, A::Error> {
        NumberDepthSeed { noun: "design", depth: self.depth, error: self.error }.visit_seq(seq)?;
        Ok(None)
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<Option<u64>, A::Error> {
        NumberDepthSeed { noun: "design", depth: self.depth, error: self.error }.visit_map(map)?;
        Ok(None)
    }
}

struct SketchPatternsSeed<'a, 'de> {
    summary: &'a mut SketchSummary<'de>,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for SketchPatternsSeed<'_, 'de> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for SketchPatternsSeed<'_, 'de> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sketch.patterns array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 4, error: self.error }.visit_map(map)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        let mut count = 0usize;
        let (mut points, mut curves, mut constraints) = (0usize, 0usize, 0usize);
        while seq
            .next_element_seed(PatternItemSeed {
                summary: self.summary,
                points: &mut points,
                curves: &mut curves,
                constraints: &mut constraints,
                error: self.error,
            })?
            .is_some()
        {
            count += 1;
            if count > solidcraft_sketch::CEILING_CONSTRAINTS && self.summary.size_err.is_none() {
                self.summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
            }
        }
        Ok(())
    }
}

struct PatternItemSeed<'a, 'de> {
    summary: &'a mut SketchSummary<'de>,
    points: &'a mut usize,
    curves: &'a mut usize,
    constraints: &'a mut usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for PatternItemSeed<'_, 'de> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for PatternItemSeed<'_, 'de> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("pattern object")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        self.mark_too_large();
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        self.mark_too_large();
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        self.mark_too_large();
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        self.mark_too_large();
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        self.mark_too_large();
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        self.mark_too_large();
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        self.mark_too_large();
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 5, error: self.error }.visit_seq(seq)?;
        self.mark_too_large();
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        let mut entities: Option<(bool, usize)> = None;
        let mut instances: Option<(bool, usize)> = None;
        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            match key.0.as_ref() {
                "entities" => {
                    entities = Some(map.next_value_seed(CountArraySeed { depth: 6, error: self.error })?);
                }
                "instances" => {
                    instances = Some(map.next_value_seed(PatternInstancesSeed {
                        summary: self.summary,
                        points: self.points,
                        curves: self.curves,
                        constraints: self.constraints,
                        error: self.error,
                    })?);
                }
                _ => {
                    map.next_value_seed(NumberDepthSeed { noun: "design", depth: 6, error: self.error })?;
                }
            }
        }
        let ok = matches!(entities, Some((true, e)) if e <= solidcraft_sketch::CEILING_CURVES + solidcraft_sketch::CEILING_POINTS)
            && matches!(instances, Some((true, i)) if i < 500);
        if !ok {
            self.mark_too_large();
        }
        Ok(())
    }
}

impl PatternItemSeed<'_, '_> {
    fn mark_too_large(self) {
        if self.summary.size_err.is_none() {
            self.summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
        }
    }
}

struct PatternInstancesSeed<'a, 'de> {
    summary: &'a mut SketchSummary<'de>,
    points: &'a mut usize,
    curves: &'a mut usize,
    constraints: &'a mut usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for PatternInstancesSeed<'_, 'de> {
    type Value = (bool, usize);
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(bool, usize), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for PatternInstancesSeed<'_, 'de> {
    type Value = (bool, usize);
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("pattern instances array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(bool, usize), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok((false, 0))
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(bool, usize), A::Error> {
        NumberDepthSeed { noun: "design", depth: 6, error: self.error }.visit_map(map)?;
        Ok((false, 0))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(bool, usize), A::Error> {
        let mut count = 0usize;
        while seq
            .next_element_seed(PatternInstanceItemSeed {
                summary: self.summary,
                points: self.points,
                curves: self.curves,
                constraints: self.constraints,
                error: self.error,
            })?
            .is_some()
        {
            count += 1;
        }
        Ok((true, count))
    }
}

struct PatternInstanceItemSeed<'a, 'de> {
    summary: &'a mut SketchSummary<'de>,
    points: &'a mut usize,
    curves: &'a mut usize,
    constraints: &'a mut usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for PatternInstanceItemSeed<'_, 'de> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for PatternInstanceItemSeed<'_, 'de> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("pattern instance object")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 7, error: self.error }.visit_seq(seq)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            match key.0.as_ref() {
                "points" => {
                    let (is_arr, len) = map.next_value_seed(CountArraySeed { depth: 8, error: self.error })?;
                    if is_arr {
                        *self.points = self.points.saturating_add(len);
                    }
                }
                "curves" => {
                    let (is_arr, len) = map.next_value_seed(CountArraySeed { depth: 8, error: self.error })?;
                    if is_arr {
                        *self.curves = self.curves.saturating_add(len);
                    }
                }
                "constraints" => {
                    let (is_arr, len) = map.next_value_seed(CountArraySeed { depth: 8, error: self.error })?;
                    if is_arr {
                        *self.constraints = self.constraints.saturating_add(len);
                    }
                }
                _ => {
                    map.next_value_seed(NumberDepthSeed { noun: "design", depth: 8, error: self.error })?;
                }
            }
        }
        if (*self.points > solidcraft_sketch::CEILING_POINTS
            || *self.curves > solidcraft_sketch::CEILING_CURVES
            || *self.constraints > solidcraft_sketch::CEILING_CONSTRAINTS)
            && self.summary.size_err.is_none()
        {
            self.summary.size_err = Some(DocError::Invalid("damaged design: sketch too large".into()));
        }
        Ok(())
    }
}

struct SketchLinksSeed<'a, 'de> {
    summary: &'a mut SketchSummary<'de>,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for SketchLinksSeed<'_, 'de> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for SketchLinksSeed<'_, 'de> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sketch.links array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 4, error: self.error }.visit_map(map)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        while let Some(raw) = seq.next_element::<&'de RawValue>()? {
            if let Err(err) = check_numbers_str(raw.get(), "design", 5) {
                return Err(fail(self.error, err));
            }
            if self.summary.versioned_err.is_some() {
                continue;
            }
            if raw.get().contains("auto_face") || raw.get().contains("sketch_placements") || raw.get().contains("\\u") {
                #[derive(serde::Deserialize)]
                struct LinkCheck<'a> {
                    #[serde(borrow, default)]
                    auto_face: Option<&'a RawValue>,
                    #[serde(borrow, default)]
                    sketch_placements: Option<&'a RawValue>,
                }
                if let Ok(check) = serde_json::from_str::<LinkCheck<'_>>(raw.get()) {
                    if let Some(af) = check.auto_face
                        && serde_json::from_str::<bool>(af.get()).is_err()
                    {
                        self.summary.versioned_err = Some(DocError::Invalid("damaged design: invalid periodic spline or revolve convention".into()));
                        continue;
                    }
                    if let Some(pins) = check.sketch_placements {
                        let invalid = || DocError::Invalid("damaged design: invalid captured sketch projection placement".into());
                        let valid_pins = serde_json::from_str::<Vec<&RawValue>>(pins.get())
                            .is_ok_and(|list| list.len() <= crate::project::MAX_SKETCH_PLACEMENTS);
                        if !valid_pins {
                            self.summary.versioned_err = Some(invalid());
                            continue;
                        }
                        match serde_json::from_str::<solidcraft_sketch::Link>(raw.get()) {
                            Ok(link) => {
                                if let Err(err) = crate::project::validate_link_sketch_placements(&link) {
                                    self.summary.versioned_err = Some(err);
                                }
                            }
                            Err(_) => {
                                self.summary.versioned_err = Some(invalid());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

struct SketchWiresSeed<'a, 'de> {
    summary: &'a mut SketchSummary<'de>,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for SketchWiresSeed<'_, 'de> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for SketchWiresSeed<'_, 'de> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sketch.wires array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        self.mark_wire_invalid();
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        self.mark_wire_invalid();
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        self.mark_wire_invalid();
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        self.mark_wire_invalid();
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        self.mark_wire_invalid();
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        self.mark_wire_invalid();
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        self.mark_wire_invalid();
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 4, error: self.error }.visit_map(map)?;
        self.mark_wire_invalid();
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        let mut count = 0usize;
        let mut ids = BTreeSet::new();
        let mut work = 0usize;
        while seq
            .next_element_seed(WireItemSeed { summary: self.summary, ids: &mut ids, work: &mut work, error: self.error })?
            .is_some()
        {
            count += 1;
            if count > 20_000 && self.summary.wires_err.is_none() {
                self.summary.wires_err = Some(DocError::Invalid("damaged design: 3D curve arrays, ids or coordinate spaces are invalid".into()));
            }
        }
        Ok(())
    }
}

impl SketchWiresSeed<'_, '_> {
    fn mark_wire_invalid(self) {
        if self.summary.wires_err.is_none() {
            self.summary.wires_err = Some(DocError::Invalid("damaged design: 3D curve arrays, ids or coordinate spaces are invalid".into()));
        }
    }
}

struct WireItemSeed<'a, 'de> {
    summary: &'a mut SketchSummary<'de>,
    ids: &'a mut BTreeSet<String>,
    work: &'a mut usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for WireItemSeed<'_, 'de> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for WireItemSeed<'_, 'de> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("wire object")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        self.mark_invalid();
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        self.mark_invalid();
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        self.mark_invalid();
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        self.mark_invalid();
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        self.mark_invalid();
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        self.mark_invalid();
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(), E> {
        self.mark_invalid();
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<(), A::Error> {
        NumberDepthSeed { noun: "design", depth: 5, error: self.error }.visit_seq(seq)?;
        self.mark_invalid();
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        let mut id: Option<String> = None;
        let mut link_bad = false;
        let mut space_bad = false;
        let mut pts: Option<(bool, usize)> = None;
        let mut fit: Option<(bool, usize)> = None;
        while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
            match key.0.as_ref() {
                "id" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 6) {
                        return Err(fail(self.error, err));
                    }
                    id = serde_json::from_str::<String>(raw.get()).ok();
                }
                "link" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 6) {
                        return Err(fail(self.error, err));
                    }
                    if raw.get() != "null" && !serde_json::from_str::<String>(raw.get()).is_ok_and(|s| !s.is_empty() && s.len() <= 512) {
                        link_bad = true;
                    }
                }
                "space" => {
                    let raw = map.next_value::<&'de RawValue>()?;
                    if let Err(err) = check_numbers_str(raw.get(), "design", 6) {
                        return Err(fail(self.error, err));
                    }
                    if !serde_json::from_str::<String>(raw.get()).is_ok_and(|s| matches!(s.as_str(), "world" | "component")) {
                        space_bad = true;
                    }
                }
                "pts" => {
                    pts = Some(map.next_value_seed(Point3ArraySeed { depth: 6, error: self.error })?);
                }
                "fit" => {
                    fit = Some(map.next_value_seed(Point3ArraySeed { depth: 6, error: self.error })?);
                }
                _ => {
                    map.next_value_seed(NumberDepthSeed { noun: "design", depth: 6, error: self.error })?;
                }
            }
        }
        if self.summary.wires_err.is_some() {
            return Ok(());
        }
        let id_ok = id.is_some_and(|s| !s.is_empty() && s.len() <= 512 && self.ids.insert(s));
        let pts_ok = matches!(pts, Some((true, n)) if n != 0 && n <= 100_000);
        let pts_len = pts.map_or(0, |(_, n)| n);
        let fit_ok = match fit {
            None => true,
            Some((true, n)) => n <= 500,
            Some((false, _)) => false,
        };
        let fit_len = fit.map_or(0, |(_, n)| n);
        *self.work = self.work.saturating_add(pts_len).saturating_add(fit_len);
        if !id_ok || link_bad || space_bad || !pts_ok || !fit_ok || *self.work > 1_000_000 || (pts_len == 1 && fit_len != 1) {
            self.mark_invalid();
        }
        Ok(())
    }
}

impl WireItemSeed<'_, '_> {
    fn mark_invalid(self) {
        if self.summary.wires_err.is_none() {
            self.summary.wires_err = Some(DocError::Invalid("damaged design: 3D curve arrays, ids or coordinate spaces are invalid".into()));
        }
    }
}

/// Validates an array of `[f64; 3]` points with `abs < MAX_REAL` in streaming mode.
struct Point3ArraySeed<'a> {
    depth: usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for Point3ArraySeed<'_> {
    type Value = (bool, usize);
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(bool, usize), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Point3ArraySeed<'_> {
    type Value = (bool, usize);
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("array of 3D points")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<(bool, usize), E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok((false, 0))
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<(bool, usize), E> {
        Ok((false, 0))
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<(bool, usize), A::Error> {
        NumberDepthSeed { noun: "design", depth: self.depth, error: self.error }.visit_map(map)?;
        Ok((false, 0))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(bool, usize), A::Error> {
        let mut all_valid = true;
        let mut len = 0usize;
        while let Some(ok) = seq.next_element_seed(Point3Seed { depth: self.depth + 1, error: self.error })? {
            len += 1;
            all_valid &= ok;
        }
        Ok((all_valid, len))
    }
}

struct Point3Seed<'a> {
    depth: usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for Point3Seed<'_> {
    type Value = bool;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<bool, D::Error> {
        if self.depth > 64 {
            return Err(fail(self.error, DocError::Invalid("damaged design: nested too deeply".into())));
        }
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Point3Seed<'_> {
    type Value = bool;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("3D point array")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<bool, E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(false)
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<bool, A::Error> {
        NumberDepthSeed { noun: "design", depth: self.depth, error: self.error }.visit_map(map)?;
        Ok(false)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<bool, A::Error> {
        let mut count = 0usize;
        let mut ok = true;
        while let Some(coord_ok) = seq.next_element_seed(CoordSeed { depth: self.depth + 1, error: self.error })? {
            count += 1;
            ok &= coord_ok;
        }
        Ok(ok && count == 3)
    }
}

struct CoordSeed<'a> {
    depth: usize,
    error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for CoordSeed<'_> {
    type Value = bool;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<bool, D::Error> {
        if self.depth > 64 {
            return Err(fail(self.error, DocError::Invalid("damaged design: nested too deeply".into())));
        }
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for CoordSeed<'_> {
    type Value = bool;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("finite coordinate")
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<bool, E> {
        let x = v as f64;
        Ok(x.is_finite() && x.abs() < MAX_REAL)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<bool, E> {
        let x = v as f64;
        Ok(x.is_finite() && x.abs() < MAX_REAL)
    }
    fn visit_f64<E: de::Error>(self, x: f64) -> std::result::Result<bool, E> {
        if !(x.is_finite() && x.abs() <= MAX_REAL) {
            return Err(fail(self.error, DocError::Invalid(format!("damaged design: the number {x} is out of range"))));
        }
        Ok(x.abs() < MAX_REAL)
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<bool, E> {
        Ok(false)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<bool, A::Error> {
        NumberDepthSeed { noun: "design", depth: self.depth, error: self.error }.visit_seq(seq)?;
        Ok(false)
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<bool, A::Error> {
        NumberDepthSeed { noun: "design", depth: self.depth, error: self.error }.visit_map(map)?;
        Ok(false)
    }
}
