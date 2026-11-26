//! Bounded JSON value deserialization, zero-allocation header peeking, and streaming numeric checks.
//!
//! Reading a design or drawing JSON file into an untyped `serde_json::Value` tree expands compact
//! objects and arrays by up to ~80× in heap memory (for example, each single-entry `BTreeMap`
//! allocates a full `LeafNode` of 11 slots). Current-version files use the zero-allocation
//! streaming helpers here to peek headers, validate numeric/depth bounds, and deserialize directly
//! into typed structs; legacy migrations, newer-format salvage, and opaque metadata maps use
//! [`from_str_bounded`] / [`deserialize_bounded_map`] so crafted inputs cannot exhaust memory.

use std::borrow::Cow;
use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
    Serializer,
};
use serde_json::{Map, Value, value::RawValue};

use crate::format::{Family, MAX_REAL};
use crate::{DocError, Result};

/// Estimated heap memory budget for any untyped `serde_json::Value` tree built during file read
/// (legacy migrations, newer-format salvage, or opaque metadata maps). Generous enough for real
/// legacy files and large metadata arrays while stopping crafted 80× expansion bombs well before
/// exhausting system memory.
pub const MAX_VALUE_MEMORY_BYTES: usize = 256 << 20;

/// Tighter estimated heap memory budget for small structured sub-objects (configuration rules,
/// material cards, face anchors, offset metadata, importer options).
pub const MAX_SMALL_VALUE_BYTES: usize = 4 << 20;

/// Estimated heap memory budget for opaque importer options carried by a design link.
pub const MAX_OPTIONS_VALUE_BYTES: usize = 16 << 20;

const VALUE_SIZE: usize = std::mem::size_of::<Value>();
/// A non-empty `BTreeMap<String, Value>` allocates a `LeafNode` of 11 key/value pairs (~632 B).
const MAP_FIRST_ENTRY_OVERHEAD: usize = 640;
const MAP_EXTRA_ENTRY_OVERHEAD: usize = 64;
/// A non-empty `Vec<Value>` initially allocates capacity 4 (4 × 32 B = 128 B).
const SEQ_FIRST_ELEM_OVERHEAD: usize = 128;

fn charge<E: de::Error>(remaining: &mut usize, bytes: usize) -> std::result::Result<(), E> {
    if bytes > *remaining {
        return Err(E::custom("JSON expands past its memory budget"));
    }
    *remaining -= bytes;
    Ok(())
}

/// Deserialize a `serde_json::Value` from `s`, aborting before heap usage can exceed `budget`.
pub fn from_str_bounded(s: &str, budget: usize) -> std::result::Result<Value, serde_json::Error> {
    let mut remaining = budget;
    let mut de = serde_json::Deserializer::from_str(s);
    let value = BoundedValueSeed(&mut remaining).deserialize(&mut de)?;
    de.end()?;
    Ok(value)
}

/// Deserialize a `serde_json::Value` from UTF-8 `bytes`, aborting before heap usage can exceed `budget`.
pub fn from_slice_bounded(bytes: &[u8], budget: usize) -> std::result::Result<Value, serde_json::Error> {
    let mut remaining = budget;
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let value = BoundedValueSeed(&mut remaining).deserialize(&mut de)?;
    de.end()?;
    Ok(value)
}

/// Deserialize a `serde_json::Value` with an explicit estimated heap byte budget.
pub fn deserialize_bounded_value<'de, D: Deserializer<'de>>(deserializer: D, budget: usize) -> std::result::Result<Value, D::Error> {
    let mut remaining = budget;
    BoundedValueSeed(&mut remaining).deserialize(deserializer)
}

/// Deserialize a `serde_json::Map<String, Value>` with an explicit estimated heap byte budget.
pub fn deserialize_bounded_map<'de, D: Deserializer<'de>>(deserializer: D, budget: usize) -> std::result::Result<Map<String, Value>, D::Error> {
    let mut remaining = budget;
    deserializer.deserialize_map(BoundedMapVisitor(&mut remaining))
}

/// Serde `deserialize_with` helper for [`crate::LinkImporter::options`].
pub(crate) fn deserialize_options_value<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Value, D::Error> {
    deserialize_bounded_value(deserializer, MAX_OPTIONS_VALUE_BYTES)
}

struct BoundedValueSeed<'a>(&'a mut usize);

impl<'de> DeserializeSeed<'de> for BoundedValueSeed<'_> {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> std::result::Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for BoundedValueSeed<'_> {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any valid JSON value within memory budget")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Value, E> {
        charge(self.0, VALUE_SIZE)?;
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Value, E> {
        charge(self.0, VALUE_SIZE)?;
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Value, E> {
        charge(self.0, VALUE_SIZE)?;
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Value, E> {
        charge(self.0, VALUE_SIZE)?;
        serde_json::Number::from_f64(value).map(Value::Number).ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Value, E> {
        charge(self.0, VALUE_SIZE.saturating_add(value.len()))?;
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> std::result::Result<Value, E> {
        charge(self.0, VALUE_SIZE.saturating_add(value.len()))?;
        Ok(Value::String(value))
    }

    fn visit_none<E: de::Error>(self) -> std::result::Result<Value, E> {
        charge(self.0, VALUE_SIZE)?;
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> std::result::Result<Value, D::Error> {
        self.deserialize(deserializer)
    }

    fn visit_unit<E: de::Error>(self) -> std::result::Result<Value, E> {
        charge(self.0, VALUE_SIZE)?;
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Value, A::Error> {
        charge(self.0, VALUE_SIZE)?;
        let mut vec = Vec::new();
        let mut first = true;
        while let Some(elem) = seq.next_element_seed(BoundedValueSeed(self.0))? {
            if first {
                charge(self.0, SEQ_FIRST_ELEM_OVERHEAD)?;
                first = false;
            }
            vec.push(elem);
        }
        Ok(Value::Array(vec))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<Value, A::Error> {
        charge(self.0, VALUE_SIZE)?;
        BoundedMapVisitor(self.0).visit_map(map).map(Value::Object)
    }
}

struct BoundedMapVisitor<'a>(&'a mut usize);

impl<'de> Visitor<'de> for BoundedMapVisitor<'_> {
    type Value = Map<String, Value>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object within memory budget")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> std::result::Result<Map<String, Value>, A::Error> {
        let mut map = Map::new();
        let mut first = true;
        while let Some(key) = access.next_key::<String>()? {
            let overhead = if first {
                first = false;
                MAP_FIRST_ENTRY_OVERHEAD
            } else {
                MAP_EXTRA_ENTRY_OVERHEAD
            };
            charge(self.0, overhead.saturating_add(key.len()))?;
            let value = access.next_value_seed(BoundedValueSeed(self.0))?;
            map.insert(key, value);
        }
        Ok(map)
    }
}

/// Borrowed-when-unescaped JSON map key so streaming visitors avoid heap allocations for plain keys.
pub(crate) struct BorrowedKey<'de>(pub Cow<'de, str>);

impl<'de> de::Deserialize<'de> for BorrowedKey<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct KeyVisitor;
        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = BorrowedKey<'de>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object key")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> std::result::Result<Self::Value, E> {
                Ok(BorrowedKey(Cow::Borrowed(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
                Ok(BorrowedKey(Cow::Owned(v.to_owned())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Self::Value, E> {
                Ok(BorrowedKey(Cow::Owned(v)))
            }
        }
        deserializer.deserialize_str(KeyVisitor)
    }
}

/// Format header peeked directly from JSON text without allocating a `serde_json::Value` tree.
pub(crate) struct Header<'de> {
    pub version: u32,
    pub applied: Option<Vec<String>>,
    pub drawings: Option<&'de RawValue>,
}

enum FormatField {
    Absent,
    NotString,
    Str(String),
}

enum PendingField {
    Absent,
    Invalid,
    Valid(Vec<String>),
}

/// Validate JSON syntax and extract a family's version header (and optional top-level `drawings`
/// raw slice) in one zero-DOM streaming pass, matching [`Family::take_version`]'s exact errors.
pub(crate) fn peek_header<'de>(family: &Family<'_>, s: &'de str, capture_drawings: bool) -> Result<Header<'de>> {
    struct RawHeader<'de> {
        is_object: bool,
        format: FormatField,
        pending: PendingField,
        drawings: Option<&'de RawValue>,
    }
    struct FormatSeed;
    impl<'de> DeserializeSeed<'de> for FormatSeed {
        type Value = FormatField;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<FormatField, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = FormatField;
                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("a format string")
                }
                fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<FormatField, E> {
                    Ok(FormatField::Str(v.to_owned()))
                }
                fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<FormatField, E> {
                    Ok(FormatField::Str(v))
                }
                fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<FormatField, E> {
                    Ok(FormatField::NotString)
                }
                fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<FormatField, E> {
                    Ok(FormatField::NotString)
                }
                fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<FormatField, E> {
                    Ok(FormatField::NotString)
                }
                fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<FormatField, E> {
                    Ok(FormatField::NotString)
                }
                fn visit_unit<E: de::Error>(self) -> std::result::Result<FormatField, E> {
                    Ok(FormatField::NotString)
                }
                fn visit_none<E: de::Error>(self) -> std::result::Result<FormatField, E> {
                    Ok(FormatField::NotString)
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<FormatField, A::Error> {
                    while seq.next_element::<IgnoredAny>()?.is_some() {}
                    Ok(FormatField::NotString)
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<FormatField, A::Error> {
                    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                    Ok(FormatField::NotString)
                }
            }
            d.deserialize_any(V)
        }
    }
    struct PendingSeed;
    impl<'de> DeserializeSeed<'de> for PendingSeed {
        type Value = PendingField;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<PendingField, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = PendingField;
                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("a short list of pending format names")
                }
                fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<PendingField, E> {
                    Ok(PendingField::Invalid)
                }
                fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<PendingField, E> {
                    Ok(PendingField::Invalid)
                }
                fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<PendingField, E> {
                    Ok(PendingField::Invalid)
                }
                fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<PendingField, E> {
                    Ok(PendingField::Invalid)
                }
                fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<PendingField, E> {
                    Ok(PendingField::Invalid)
                }
                fn visit_unit<E: de::Error>(self) -> std::result::Result<PendingField, E> {
                    Ok(PendingField::Invalid)
                }
                fn visit_none<E: de::Error>(self) -> std::result::Result<PendingField, E> {
                    Ok(PendingField::Invalid)
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<PendingField, A::Error> {
                    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                    Ok(PendingField::Invalid)
                }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<PendingField, A::Error> {
                    let mut names = Vec::new();
                    let mut valid = true;
                    while let Some(item) = seq.next_element_seed(FormatSeed)? {
                        if valid {
                            match item {
                                FormatField::Str(name) if name.len() <= 128 && names.len() < 1000 => names.push(name),
                                _ => {
                                    valid = false;
                                    names.clear();
                                }
                            }
                        }
                    }
                    Ok(if valid { PendingField::Valid(names) } else { PendingField::Invalid })
                }
            }
            d.deserialize_any(V)
        }
    }
    struct RootVisitor {
        capture_drawings: bool,
    }
    impl<'de> Visitor<'de> for RootVisitor {
        type Value = RawHeader<'de>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("any valid JSON value")
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
            Ok(RawHeader { is_object: false, format: FormatField::Absent, pending: PendingField::Absent, drawings: None })
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
            Ok(RawHeader { is_object: false, format: FormatField::Absent, pending: PendingField::Absent, drawings: None })
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
            Ok(RawHeader { is_object: false, format: FormatField::Absent, pending: PendingField::Absent, drawings: None })
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
            Ok(RawHeader { is_object: false, format: FormatField::Absent, pending: PendingField::Absent, drawings: None })
        }
        fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
            Ok(RawHeader { is_object: false, format: FormatField::Absent, pending: PendingField::Absent, drawings: None })
        }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(RawHeader { is_object: false, format: FormatField::Absent, pending: PendingField::Absent, drawings: None })
        }
        fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(RawHeader { is_object: false, format: FormatField::Absent, pending: PendingField::Absent, drawings: None })
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(RawHeader { is_object: false, format: FormatField::Absent, pending: PendingField::Absent, drawings: None })
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
            let mut format = FormatField::Absent;
            let mut pending = PendingField::Absent;
            let mut drawings = None;
            while let Some(key) = map.next_key::<BorrowedKey<'de>>()? {
                match key.0.as_ref() {
                    "format" => format = map.next_value_seed(FormatSeed)?,
                    "format_pending" => pending = map.next_value_seed(PendingSeed)?,
                    "drawings" if self.capture_drawings => drawings = Some(map.next_value::<&'de RawValue>()?),
                    _ => {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
            }
            Ok(RawHeader { is_object: true, format, pending, drawings })
        }
    }

    let (app, noun) = (family.app, family.noun);
    let mut de = serde_json::Deserializer::from_str(s);
    let raw = de
        .deserialize_any(RootVisitor { capture_drawings })
        .and_then(|raw| de.end().map(|()| raw))
        .map_err(|e| DocError::Invalid(format!("not a {app} {noun}: {e}")))?;
    let not_ours = |what: String| DocError::Invalid(format!("not a {app} {noun} ({what})"));
    if !raw.is_object {
        return Err(not_ours("expected a JSON object".into()));
    }
    let version = match raw.format {
        FormatField::Absent => family.untagged.ok_or_else(|| not_ours("no format".into()))?,
        FormatField::NotString => return Err(not_ours("format is not a string".into())),
        FormatField::Str(f) => family.version_of(&f).ok_or_else(|| not_ours(format!("format `{f}`")))?,
    };
    let applied = match raw.pending {
        PendingField::Absent => None,
        PendingField::Invalid => return Err(DocError::Invalid(format!("damaged {noun}: `format_pending` is not a short list of names"))),
        PendingField::Valid(names) => Some(names),
    };
    if version == 0 {
        return Err(not_ours("format 0".into()));
    }
    Ok(Header { version, applied, drawings: raw.drawings })
}

/// Zero-allocation streaming visitor that enforces the same depth (`<= 64`) and float range
/// (`<= MAX_REAL`) rules as `format::check_numbers`, storing the exact `DocError` into `error`.
pub(crate) struct NumberDepthSeed<'a> {
    pub noun: &'static str,
    pub depth: usize,
    pub error: &'a mut Option<DocError>,
}

impl<'de> DeserializeSeed<'de> for NumberDepthSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> std::result::Result<(), D::Error> {
        if self.depth > 64 {
            *self.error = Some(DocError::Invalid(format!("damaged {}: nested too deeply", self.noun)));
            return Err(de::Error::custom("nested too deeply"));
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for NumberDepthSeed<'_> {
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
            *self.error = Some(DocError::Invalid(format!("damaged {}: the number {x} is out of range", self.noun)));
            return Err(de::Error::custom("number out of range"));
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
        while seq.next_element_seed(NumberDepthSeed { noun: self.noun, depth: self.depth + 1, error: self.error })?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        while map.next_key::<IgnoredAny>()?.is_some() {
            map.next_value_seed(NumberDepthSeed { noun: self.noun, depth: self.depth + 1, error: self.error })?;
        }
        Ok(())
    }
}

/// Stream-check depth and float ranges in a JSON slice without allocating any `Value`.
pub(crate) fn check_numbers_str(s: &str, noun: &'static str, depth: usize) -> Result<()> {
    let mut error = None;
    let mut de = serde_json::Deserializer::from_str(s);
    let res = NumberDepthSeed { noun, depth, error: &mut error }.deserialize(&mut de).and_then(|()| de.end());
    if let Some(err) = error {
        return Err(err);
    }
    res.map_err(|e| DocError::Invalid(format!("damaged {noun}: {e}")))
}

/// Zero-allocation `serde::Serializer` checking that every serialized float in a typed `Document`
/// (outside top-level `metadata` and `user_materials[*].physical_card`) is finite and within
/// `±MAX_REAL`, and that nesting stays within depth 64.
pub(crate) fn check_typed_design_numbers(doc: &crate::Document) -> Result<()> {
    doc.serialize(NumberCheckSerializer { depth: 0, mode: CheckMode::Root }).map_err(|e| e.0)
}

#[derive(Debug)]
struct CheckError(DocError);

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for CheckError {}

impl ser::Error for CheckError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self(DocError::Invalid(format!("design file: {msg}")))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CheckMode {
    Root,
    UserMaterials,
    UserMaterial,
    Skip,
    Normal,
}

#[derive(Clone, Copy)]
struct NumberCheckSerializer {
    depth: usize,
    mode: CheckMode,
}

impl NumberCheckSerializer {
    fn check_depth(self) -> std::result::Result<(), CheckError> {
        if self.mode != CheckMode::Skip && self.depth > 64 {
            return Err(CheckError(DocError::Invalid("damaged design: nested too deeply".into())));
        }
        Ok(())
    }

    fn check_float(self, v: f64) -> std::result::Result<(), CheckError> {
        self.check_depth()?;
        if self.mode != CheckMode::Skip && !(v.is_finite() && v.abs() <= MAX_REAL) {
            return Err(CheckError(DocError::Invalid(format!("damaged design: the number {v} is out of range"))));
        }
        Ok(())
    }

    fn child(self, mode: CheckMode) -> Self {
        Self { depth: self.depth + 1, mode }
    }
}

struct SeqCheck {
    depth: usize,
    mode: CheckMode,
    count: usize,
}

impl SerializeSeq for SeqCheck {
    type Ok = ();
    type Error = CheckError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> std::result::Result<(), CheckError> {
        if self.mode == CheckMode::Skip {
            return Ok(());
        }
        let child_mode = if self.mode == CheckMode::UserMaterials {
            self.count += 1;
            if self.count > crate::MAX_USER_MATERIALS {
                return Err(CheckError(DocError::Invalid("document exceeds 1000 user materials".into())));
            }
            CheckMode::UserMaterial
        } else {
            CheckMode::Normal
        };
        value.serialize(NumberCheckSerializer { depth: self.depth + 1, mode: child_mode })
    }

    fn end(self) -> std::result::Result<(), CheckError> {
        Ok(())
    }
}

impl SerializeTuple for SeqCheck {
    type Ok = ();
    type Error = CheckError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> std::result::Result<(), CheckError> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> std::result::Result<(), CheckError> {
        Ok(())
    }
}

impl SerializeTupleStruct for SeqCheck {
    type Ok = ();
    type Error = CheckError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> std::result::Result<(), CheckError> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> std::result::Result<(), CheckError> {
        Ok(())
    }
}

impl SerializeTupleVariant for SeqCheck {
    type Ok = ();
    type Error = CheckError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> std::result::Result<(), CheckError> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> std::result::Result<(), CheckError> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum MapKeyTag {
    Normal,
    Metadata,
    UserMaterials,
    PhysicalCard,
}

struct MapKeyProbe;

impl Serializer for MapKeyProbe {
    type Ok = MapKeyTag;
    type Error = CheckError;
    type SerializeSeq = ser::Impossible<MapKeyTag, CheckError>;
    type SerializeTuple = ser::Impossible<MapKeyTag, CheckError>;
    type SerializeTupleStruct = ser::Impossible<MapKeyTag, CheckError>;
    type SerializeTupleVariant = ser::Impossible<MapKeyTag, CheckError>;
    type SerializeMap = ser::Impossible<MapKeyTag, CheckError>;
    type SerializeStruct = ser::Impossible<MapKeyTag, CheckError>;
    type SerializeStructVariant = ser::Impossible<MapKeyTag, CheckError>;

    fn serialize_str(self, v: &str) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(match v {
            "metadata" => MapKeyTag::Metadata,
            "user_materials" => MapKeyTag::UserMaterials,
            "physical_card" => MapKeyTag::PhysicalCard,
            _ => MapKeyTag::Normal,
        })
    }

    fn serialize_bool(self, _: bool) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_i8(self, _: i8) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_i16(self, _: i16) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_i32(self, _: i32) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_i64(self, _: i64) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_u8(self, _: u8) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_u16(self, _: u16) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_u32(self, _: u32) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_u64(self, _: u64) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_f32(self, _: f32) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_f64(self, _: f64) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_char(self, _: char) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_bytes(self, _: &[u8]) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_none(self) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> std::result::Result<MapKeyTag, CheckError> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_unit_struct(self, _: &'static str) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> std::result::Result<MapKeyTag, CheckError> {
        Ok(MapKeyTag::Normal)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, v: &T) -> std::result::Result<MapKeyTag, CheckError> {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> std::result::Result<MapKeyTag, CheckError> {
        Err(ser::Error::custom("map key must be a string"))
    }
    fn serialize_seq(self, _: Option<usize>) -> std::result::Result<Self::SerializeSeq, CheckError> {
        Err(ser::Error::custom("map key must be a string"))
    }
    fn serialize_tuple(self, _: usize) -> std::result::Result<Self::SerializeTuple, CheckError> {
        Err(ser::Error::custom("map key must be a string"))
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> std::result::Result<Self::SerializeTupleStruct, CheckError> {
        Err(ser::Error::custom("map key must be a string"))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> std::result::Result<Self::SerializeTupleVariant, CheckError> {
        Err(ser::Error::custom("map key must be a string"))
    }
    fn serialize_map(self, _: Option<usize>) -> std::result::Result<Self::SerializeMap, CheckError> {
        Err(ser::Error::custom("map key must be a string"))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> std::result::Result<Self::SerializeStruct, CheckError> {
        Err(ser::Error::custom("map key must be a string"))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> std::result::Result<Self::SerializeStructVariant, CheckError> {
        Err(ser::Error::custom("map key must be a string"))
    }
}

struct MapCheck {
    depth: usize,
    mode: CheckMode,
    pending_key: MapKeyTag,
}

impl MapCheck {
    fn child_mode(&self, tag: MapKeyTag) -> CheckMode {
        match (self.mode, tag) {
            (CheckMode::Skip, _) => CheckMode::Skip,
            (CheckMode::Root, MapKeyTag::Metadata) => CheckMode::Skip,
            (CheckMode::Root, MapKeyTag::UserMaterials) => CheckMode::UserMaterials,
            (CheckMode::UserMaterial, MapKeyTag::PhysicalCard) => CheckMode::Skip,
            _ => CheckMode::Normal,
        }
    }
}

impl SerializeMap for MapCheck {
    type Ok = ();
    type Error = CheckError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> std::result::Result<(), CheckError> {
        self.pending_key = match self.mode {
            CheckMode::Root | CheckMode::UserMaterial => key.serialize(MapKeyProbe)?,
            _ => MapKeyTag::Normal,
        };
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> std::result::Result<(), CheckError> {
        let mode = self.child_mode(self.pending_key);
        if mode == CheckMode::Skip {
            return Ok(());
        }
        value.serialize(NumberCheckSerializer { depth: self.depth + 1, mode })
    }

    fn end(self) -> std::result::Result<(), CheckError> {
        Ok(())
    }
}

impl SerializeStruct for MapCheck {
    type Ok = ();
    type Error = CheckError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> std::result::Result<(), CheckError> {
        let tag = match key {
            "metadata" => MapKeyTag::Metadata,
            "user_materials" => MapKeyTag::UserMaterials,
            "physical_card" => MapKeyTag::PhysicalCard,
            _ => MapKeyTag::Normal,
        };
        let mode = self.child_mode(tag);
        if mode == CheckMode::Skip {
            return Ok(());
        }
        value.serialize(NumberCheckSerializer { depth: self.depth + 1, mode })
    }

    fn end(self) -> std::result::Result<(), CheckError> {
        Ok(())
    }
}

impl SerializeStructVariant for MapCheck {
    type Ok = ();
    type Error = CheckError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> std::result::Result<(), CheckError> {
        SerializeStruct::serialize_field(self, key, value)
    }

    fn end(self) -> std::result::Result<(), CheckError> {
        Ok(())
    }
}

impl Serializer for NumberCheckSerializer {
    type Ok = ();
    type Error = CheckError;
    type SerializeSeq = SeqCheck;
    type SerializeTuple = SeqCheck;
    type SerializeTupleStruct = SeqCheck;
    type SerializeTupleVariant = SeqCheck;
    type SerializeMap = MapCheck;
    type SerializeStruct = MapCheck;
    type SerializeStructVariant = MapCheck;

    fn serialize_bool(self, _: bool) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_i8(self, _: i8) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_i16(self, _: i16) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_i32(self, _: i32) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_i64(self, _: i64) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_u8(self, _: u8) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_u16(self, _: u16) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_u32(self, _: u32) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_u64(self, _: u64) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_f32(self, v: f32) -> std::result::Result<(), CheckError> {
        self.check_float(f64::from(v))
    }
    fn serialize_f64(self, v: f64) -> std::result::Result<(), CheckError> {
        self.check_float(v)
    }
    fn serialize_char(self, _: char) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_str(self, _: &str) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_bytes(self, _: &[u8]) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_none(self) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> std::result::Result<(), CheckError> {
        if self.mode == CheckMode::Skip {
            return Ok(());
        }
        value.serialize(self)
    }
    fn serialize_unit(self) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_unit_struct(self, _: &'static str) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> std::result::Result<(), CheckError> {
        self.check_depth()
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, value: &T) -> std::result::Result<(), CheckError> {
        if self.mode == CheckMode::Skip {
            return Ok(());
        }
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> std::result::Result<(), CheckError> {
        if self.mode == CheckMode::Skip {
            return Ok(());
        }
        self.check_depth()?;
        value.serialize(self.child(CheckMode::Normal))
    }
    fn serialize_seq(self, _: Option<usize>) -> std::result::Result<SeqCheck, CheckError> {
        self.check_depth()?;
        Ok(SeqCheck { depth: self.depth, mode: self.mode, count: 0 })
    }
    fn serialize_tuple(self, _: usize) -> std::result::Result<SeqCheck, CheckError> {
        self.check_depth()?;
        Ok(SeqCheck { depth: self.depth, mode: self.mode, count: 0 })
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> std::result::Result<SeqCheck, CheckError> {
        self.check_depth()?;
        Ok(SeqCheck { depth: self.depth, mode: self.mode, count: 0 })
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> std::result::Result<SeqCheck, CheckError> {
        if self.mode == CheckMode::Skip {
            return Ok(SeqCheck { depth: self.depth, mode: CheckMode::Skip, count: 0 });
        }
        self.check_depth()?;
        let next = self.child(CheckMode::Normal);
        next.check_depth()?;
        Ok(SeqCheck { depth: next.depth, mode: CheckMode::Normal, count: 0 })
    }
    fn serialize_map(self, _: Option<usize>) -> std::result::Result<MapCheck, CheckError> {
        self.check_depth()?;
        Ok(MapCheck { depth: self.depth, mode: self.mode, pending_key: MapKeyTag::Normal })
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> std::result::Result<MapCheck, CheckError> {
        self.check_depth()?;
        Ok(MapCheck { depth: self.depth, mode: self.mode, pending_key: MapKeyTag::Normal })
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> std::result::Result<MapCheck, CheckError> {
        if self.mode == CheckMode::Skip {
            return Ok(MapCheck { depth: self.depth, mode: CheckMode::Skip, pending_key: MapKeyTag::Normal });
        }
        self.check_depth()?;
        let next = self.child(CheckMode::Normal);
        next.check_depth()?;
        Ok(MapCheck { depth: next.depth, mode: CheckMode::Normal, pending_key: MapKeyTag::Normal })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn bounded_value_rejects_high_expansion_object_array_early() {
        // Each `{"a":0},` is 8 bytes in JSON but allocates a full BTreeMap LeafNode (~664 B, ~83×).
        let mut crafted = String::from("[");
        for i in 0..20_000 {
            if i > 0 {
                crafted.push(',');
            }
            crafted.push_str(r#"{"a":0}"#);
        }
        crafted.push(']');
        assert!(from_str_bounded(&crafted, 1 << 20).unwrap_err().to_string().contains("memory budget"));
        assert!(from_str_bounded(r#"{"a":[1,2,3],"b":"ok"}"#, 1 << 20).is_ok());
    }

    #[test]
    fn bounded_value_rejects_trailing_garbage() {
        assert!(from_str_bounded("{} trailing", 1 << 20).is_err());
    }
}
