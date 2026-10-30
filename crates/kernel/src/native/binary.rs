//! Bounded tagged binary values: integer and f64 bits stay exact, no base64/text numbers.
use super::*;
fn bytes<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    let head = input.get(..n).ok_or_else(|| invalid("truncated value"))?;
    *input = input.get(n..).ok_or_else(|| invalid("truncated value"))?;
    Ok(head)
}
fn length(input: &mut &[u8]) -> Result<usize> {
    let raw: [u8; 4] = bytes(input, 4)?.try_into().map_err(invalid)?;
    usize::try_from(u32::from_le_bytes(raw)).map_err(invalid)
}
fn string(input: &mut &[u8]) -> Result<String> {
    let n = length(input)?;
    if n > 1 << 20 {
        return Err(invalid("string budget"));
    }
    Ok(std::str::from_utf8(bytes(input, n)?).map_err(invalid)?.to_owned())
}
pub(super) fn read(input: &mut &[u8], depth: usize, budget: &mut usize) -> Result<Value> {
    if depth > 48 || *budget == 0 {
        return Err(invalid("value budget/depth"));
    }
    *budget -= 1;
    let tag = *bytes(input, 1)?.first().ok_or_else(|| invalid("tag"))?;
    Ok(match tag {
        0 => Value::Null,
        1 => Value::Bool(false),
        2 => Value::Bool(true),
        3..=5 => {
            let raw: [u8; 8] = bytes(input, 8)?.try_into().map_err(invalid)?;
            Value::Number(match tag {
                3 => Number::from(i64::from_le_bytes(raw)),
                4 => Number::from(u64::from_le_bytes(raw)),
                _ => Number::from_f64(f64::from_le_bytes(raw)).ok_or_else(|| invalid("non-finite number"))?,
            })
        }
        6 => Value::String(string(input)?),
        7 | 8 => {
            let n = length(input)?;
            if n > MAX_ITEMS || n > input.len() || n > *budget {
                return Err(invalid("collection budget"));
            }
            if tag == 7 {
                let mut a = Vec::new();
                for _ in 0..n {
                    a.push(read(input, depth + 1, budget)?);
                }
                Value::Array(a)
            } else {
                let mut map = Map::new();
                for _ in 0..n {
                    let key = string(input)?;
                    if map.contains_key(&key) {
                        return Err(invalid("duplicate key"));
                    }
                    map.insert(key, read(input, depth + 1, budget)?);
                }
                Value::Object(map)
            }
        }
        _ => return Err(invalid("unknown binary tag")),
    })
}
#[cfg(test)]
fn count(n: usize, out: &mut Vec<u8>) -> Result<()> {
    out.extend_from_slice(&u32::try_from(n).map_err(invalid)?.to_le_bytes());
    Ok(())
}
#[cfg(test)]
fn text(s: &str, out: &mut Vec<u8>) -> Result<()> {
    count(s.len(), out)?;
    out.extend_from_slice(s.as_bytes());
    Ok(())
}
/// `value` appended to `out`. The encoder writes a body as it serializes
/// ([`write_serialized`]); this writes a value already built, as tests build hostile ones.
#[cfg(test)]
pub(super) fn write(value: &Value, out: &mut Vec<u8>, depth: usize) -> Result<()> {
    if depth > 48 || out.len() > MAX_BYTES {
        return Err(invalid("encoding budget"));
    }
    match value {
        Value::Null => out.push(0),
        Value::Bool(b) => out.push(if *b { 2 } else { 1 }),
        Value::Number(n) => {
            if n.is_f64() {
                out.push(5);
                out.extend_from_slice(&n.as_f64().ok_or_else(|| invalid("number"))?.to_le_bytes());
            } else if let Some(n) = n.as_u64() {
                out.push(4);
                out.extend_from_slice(&n.to_le_bytes());
            } else {
                out.push(3);
                out.extend_from_slice(&n.as_i64().ok_or_else(|| invalid("integer"))?.to_le_bytes());
            }
        }
        Value::String(s) => {
            out.push(6);
            text(s, out)?;
        }
        Value::Array(a) => {
            out.push(7);
            count(a.len(), out)?;
            for v in a {
                write(v, out, depth + 1)?;
            }
        }
        Value::Object(map) => {
            out.push(8);
            count(map.len(), out)?;
            for (k, v) in map {
                text(k, out)?;
                write(v, out, depth + 1)?;
            }
        }
    }
    if out.len() > MAX_BYTES {
        return Err(invalid("encoding size"));
    }
    Ok(())
}

/// Why a value could not be written (a serde error carries only its message).
#[derive(Debug)]
pub(super) struct WriteError(String);

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WriteError {}

impl serde::ser::Error for WriteError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        WriteError(msg.to_string())
    }
}

fn refuse(what: &str) -> WriteError {
    WriteError(what.into())
}

type Written<T = ()> = std::result::Result<T, WriteError>;

/// `value` appended to `out` as the tagged binary value it is as JSON: the bytes [`write`] gives
/// for `serde_json::to_value(value)`, refused where that, the checks on its result
/// (`validate_value`, the [`MAX_VALUES`] budget) or [`write`] refuse. Written as it is
/// serialized, with no JSON value in between: a body's record is millions of values, and building
/// them all first was most of the cost of encoding it.
///
/// As JSON: a struct or a map is an object of its entries in key order (a JSON object's), its
/// keys strings (a map's keys may be strings, characters, booleans or integers, written as JSON
/// writes such keys); an enum variant without data its name, one with data an object of its name
/// and the data; an option its value or null; bytes an array of numbers. A number is an integer
/// (unsigned from 0 up, signed below) or a finite float; a float that is not finite is null.
///
/// Refused: nesting past 48, a string over 1 MiB, an array of more than [`MAX_ITEMS`], a float
/// over 1e12 in size, more than [`MAX_VALUES`] values, more than [`MAX_BYTES`] in `out`.
pub(super) fn write_serialized<T: serde::Serialize + ?Sized>(value: &T, out: &mut Vec<u8>) -> Result<()> {
    let mut w = Writer { out, values: 0 };
    value.serialize(Ser { w: &mut w, depth: 0 }).map_err(invalid)
}

struct Writer<'a> {
    out: &'a mut Vec<u8>,
    values: usize,
}

impl Writer<'_> {
    /// Count a value at `depth` and write its tag.
    fn start(&mut self, depth: usize, tag: u8) -> Written {
        if depth > 48 {
            return Err(refuse("nesting"));
        }
        self.values = self.values.saturating_add(1);
        if self.values > MAX_VALUES {
            return Err(refuse("value budget"));
        }
        self.out.push(tag);
        self.fits()
    }
    fn fits(&self) -> Written {
        if self.out.len() > MAX_BYTES { Err(refuse("encoding size")) } else { Ok(()) }
    }
    fn number(&mut self, depth: usize, tag: u8, bits: [u8; 8]) -> Written {
        self.start(depth, tag)?;
        self.out.extend_from_slice(&bits);
        self.fits()
    }
    fn unsigned(&mut self, depth: usize, v: u64) -> Written {
        self.number(depth, 4, v.to_le_bytes())
    }
    fn signed(&mut self, depth: usize, v: i64) -> Written {
        match u64::try_from(v) {
            Ok(u) => self.unsigned(depth, u),
            Err(_) => self.number(depth, 3, v.to_le_bytes()),
        }
    }
    fn float(&mut self, depth: usize, v: f64) -> Written {
        if !v.is_finite() {
            return self.start(depth, 0);
        }
        if v.abs() > 1e12 {
            return Err(refuse("non-finite metadata"));
        }
        self.number(depth, 5, v.to_le_bytes())
    }
    fn length(&mut self, n: usize) -> Written {
        self.out.extend_from_slice(&u32::try_from(n).map_err(|_| refuse("length"))?.to_le_bytes());
        self.fits()
    }
    fn text(&mut self, s: &str) -> Written {
        self.length(s.len())?;
        self.out.extend_from_slice(s.as_bytes());
        self.fits()
    }
    fn string(&mut self, depth: usize, s: &str) -> Written {
        if s.len() > 1 << 20 {
            return Err(refuse("string budget"));
        }
        self.start(depth, 6)?;
        self.text(s)
    }
    /// Open a collection at `depth`: its tag and room for its count, which goes at the place
    /// returned.
    fn open(&mut self, depth: usize, tag: u8) -> Written<usize> {
        self.start(depth, tag)?;
        let at = self.out.len();
        self.out.extend_from_slice(&[0; 4]);
        self.fits()?;
        Ok(at)
    }
    fn count_at(&mut self, at: usize, n: usize) -> Written {
        let n = u32::try_from(n).map_err(|_| refuse("length"))?.to_le_bytes();
        self.out.get_mut(at..at + 4).ok_or_else(|| refuse("count"))?.copy_from_slice(&n);
        Ok(())
    }
}

/// A value to write at a depth.
pub(super) struct Ser<'w, 'a> {
    w: &'w mut Writer<'a>,
    depth: usize,
}

impl<'w, 'a> Ser<'w, 'a> {
    /// An object of one entry, named `variant`, whose value comes next (at the depth returned).
    fn variant(self, variant: &str) -> Written<Ser<'w, 'a>> {
        let at = self.w.open(self.depth, 8)?;
        self.w.count_at(at, 1)?;
        self.w.text(variant)?;
        Ok(Ser { w: self.w, depth: self.depth + 1 })
    }
    fn array(self) -> Written<Array<'w, 'a>> {
        let at = self.w.open(self.depth, 7)?;
        Ok(Array { w: self.w, depth: self.depth, at, items: 0 })
    }
    fn object(self) -> Written<Object<'w, 'a>> {
        let at = self.w.open(self.depth, 8)?;
        Ok(Object { w: self.w, depth: self.depth, at, entries: Vec::new(), key: None })
    }
}

/// An array being written: where its count goes, and how many items it has.
pub(super) struct Array<'w, 'a> {
    w: &'w mut Writer<'a>,
    depth: usize,
    at: usize,
    items: usize,
}

impl Array<'_, '_> {
    fn item<T: serde::Serialize + ?Sized>(&mut self, value: &T) -> Written {
        self.items += 1;
        if self.items > MAX_ITEMS {
            return Err(refuse("array budget"));
        }
        value.serialize(Ser { w: self.w, depth: self.depth + 1 })
    }
    fn close(self) -> Written {
        self.w.count_at(self.at, self.items)
    }
}

/// An object being written: where its count goes, and each entry's key and where it ends. The
/// entries are written as they come and put in key order when it closes.
pub(super) struct Object<'w, 'a> {
    w: &'w mut Writer<'a>,
    depth: usize,
    at: usize,
    entries: Vec<(String, usize)>,
    /// The key of the entry whose value comes next.
    key: Option<String>,
}

impl Object<'_, '_> {
    fn key(&mut self, key: String) -> Written {
        self.w.text(&key)?;
        self.key = Some(key);
        Ok(())
    }
    fn value<T: serde::Serialize + ?Sized>(&mut self, value: &T) -> Written {
        let key = self.key.take().ok_or_else(|| refuse("an object's value without its key"))?;
        value.serialize(Ser { w: self.w, depth: self.depth + 1 })?;
        self.entries.push((key, self.w.out.len()));
        Ok(())
    }
    fn entry<T: serde::Serialize + ?Sized>(&mut self, key: &str, value: &T) -> Written {
        self.key(key.to_owned())?;
        self.value(value)
    }
    fn close(self) -> Written {
        let Object { w, at, entries, .. } = self;
        let first = at + 4;
        if !entries.windows(2).all(|p| p[0].0 < p[1].0) {
            // In key order, as a JSON object (a map by key) holds them. A key twice would keep
            // only its last value there; no record repeats one.
            let mut spans: Vec<(&str, usize, usize)> = Vec::with_capacity(entries.len());
            let mut start = first;
            for (key, end) in &entries {
                spans.push((key.as_str(), start, *end));
                start = *end;
            }
            spans.sort_by(|a, b| a.0.cmp(b.0));
            if spans.windows(2).any(|p| p[0].0 == p[1].0) {
                return Err(refuse("duplicate key"));
            }
            let written = w.out.split_off(first);
            for (_, start, end) in spans {
                w.out.extend_from_slice(written.get(start - first..end - first).ok_or_else(|| refuse("object"))?);
            }
        }
        w.count_at(at, entries.len())
    }
}

/// A map key as JSON writes it: a string.
struct Key;

impl serde::Serializer for Key {
    type Ok = String;
    type Error = WriteError;
    type SerializeSeq = serde::ser::Impossible<String, WriteError>;
    type SerializeTuple = serde::ser::Impossible<String, WriteError>;
    type SerializeTupleStruct = serde::ser::Impossible<String, WriteError>;
    type SerializeTupleVariant = serde::ser::Impossible<String, WriteError>;
    type SerializeMap = serde::ser::Impossible<String, WriteError>;
    type SerializeStruct = serde::ser::Impossible<String, WriteError>;
    type SerializeStructVariant = serde::ser::Impossible<String, WriteError>;

    fn serialize_bool(self, v: bool) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_i8(self, v: i8) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_i16(self, v: i16) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_i32(self, v: i32) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_i64(self, v: i64) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_i128(self, v: i128) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_u8(self, v: u8) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_u16(self, v: u16) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_u32(self, v: u32) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_u64(self, v: u64) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_u128(self, v: u128) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_f32(self, _: f32) -> Written<String> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_f64(self, _: f64) -> Written<String> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_char(self, v: char) -> Written<String> {
        Ok(v.to_string())
    }
    fn serialize_str(self, v: &str) -> Written<String> {
        Ok(v.to_owned())
    }
    fn serialize_bytes(self, _: &[u8]) -> Written<String> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_none(self) -> Written<String> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_some<T: serde::Serialize + ?Sized>(self, _: &T) -> Written<String> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_unit(self) -> Written<String> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_unit_struct(self, _: &'static str) -> Written<String> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Written<String> {
        Ok(variant.to_owned())
    }
    fn serialize_newtype_struct<T: serde::Serialize + ?Sized>(self, _: &'static str, value: &T) -> Written<String> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: serde::Serialize + ?Sized>(self, _: &'static str, _: u32, _: &'static str, _: &T) -> Written<String> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_seq(self, _: Option<usize>) -> Written<Self::SerializeSeq> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_tuple(self, _: usize) -> Written<Self::SerializeTuple> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Written<Self::SerializeTupleStruct> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Written<Self::SerializeTupleVariant> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_map(self, _: Option<usize>) -> Written<Self::SerializeMap> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Written<Self::SerializeStruct> {
        Err(refuse("a map key must be a string"))
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Written<Self::SerializeStructVariant> {
        Err(refuse("a map key must be a string"))
    }
}

impl<'w, 'a> serde::Serializer for Ser<'w, 'a> {
    type Ok = ();
    type Error = WriteError;
    type SerializeSeq = Array<'w, 'a>;
    type SerializeTuple = Array<'w, 'a>;
    type SerializeTupleStruct = Array<'w, 'a>;
    type SerializeTupleVariant = Array<'w, 'a>;
    type SerializeMap = Object<'w, 'a>;
    type SerializeStruct = Object<'w, 'a>;
    type SerializeStructVariant = Object<'w, 'a>;

    fn serialize_bool(self, v: bool) -> Written {
        self.w.start(self.depth, if v { 2 } else { 1 })
    }
    fn serialize_i8(self, v: i8) -> Written {
        self.w.signed(self.depth, v.into())
    }
    fn serialize_i16(self, v: i16) -> Written {
        self.w.signed(self.depth, v.into())
    }
    fn serialize_i32(self, v: i32) -> Written {
        self.w.signed(self.depth, v.into())
    }
    fn serialize_i64(self, v: i64) -> Written {
        self.w.signed(self.depth, v)
    }
    fn serialize_i128(self, v: i128) -> Written {
        match (u64::try_from(v), i64::try_from(v)) {
            (Ok(u), _) => self.w.unsigned(self.depth, u),
            (_, Ok(i)) => self.w.signed(self.depth, i),
            _ => Err(refuse("number out of range")),
        }
    }
    fn serialize_u8(self, v: u8) -> Written {
        self.w.unsigned(self.depth, v.into())
    }
    fn serialize_u16(self, v: u16) -> Written {
        self.w.unsigned(self.depth, v.into())
    }
    fn serialize_u32(self, v: u32) -> Written {
        self.w.unsigned(self.depth, v.into())
    }
    fn serialize_u64(self, v: u64) -> Written {
        self.w.unsigned(self.depth, v)
    }
    fn serialize_u128(self, v: u128) -> Written {
        self.w.unsigned(self.depth, u64::try_from(v).map_err(|_| refuse("number out of range"))?)
    }
    fn serialize_f32(self, v: f32) -> Written {
        self.w.float(self.depth, v.into())
    }
    fn serialize_f64(self, v: f64) -> Written {
        self.w.float(self.depth, v)
    }
    fn serialize_char(self, v: char) -> Written {
        self.w.string(self.depth, v.encode_utf8(&mut [0; 4]))
    }
    fn serialize_str(self, v: &str) -> Written {
        self.w.string(self.depth, v)
    }
    fn serialize_bytes(self, v: &[u8]) -> Written {
        let mut a = self.array()?;
        for b in v {
            a.item(b)?;
        }
        a.close()
    }
    fn serialize_none(self) -> Written {
        self.w.start(self.depth, 0)
    }
    fn serialize_some<T: serde::Serialize + ?Sized>(self, value: &T) -> Written {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Written {
        self.w.start(self.depth, 0)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Written {
        self.w.start(self.depth, 0)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Written {
        self.w.string(self.depth, variant)
    }
    fn serialize_newtype_struct<T: serde::Serialize + ?Sized>(self, _: &'static str, value: &T) -> Written {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: serde::Serialize + ?Sized>(self, _: &'static str, _: u32, variant: &'static str, value: &T) -> Written {
        value.serialize(self.variant(variant)?)
    }
    fn serialize_seq(self, _: Option<usize>) -> Written<Array<'w, 'a>> {
        self.array()
    }
    fn serialize_tuple(self, _: usize) -> Written<Array<'w, 'a>> {
        self.array()
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Written<Array<'w, 'a>> {
        self.array()
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Written<Array<'w, 'a>> {
        self.variant(variant)?.array()
    }
    fn serialize_map(self, _: Option<usize>) -> Written<Object<'w, 'a>> {
        self.object()
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Written<Object<'w, 'a>> {
        self.object()
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Written<Object<'w, 'a>> {
        self.variant(variant)?.object()
    }
}

impl serde::ser::SerializeSeq for Array<'_, '_> {
    type Ok = ();
    type Error = WriteError;
    fn serialize_element<T: serde::Serialize + ?Sized>(&mut self, value: &T) -> Written {
        self.item(value)
    }
    fn end(self) -> Written {
        self.close()
    }
}

impl serde::ser::SerializeTuple for Array<'_, '_> {
    type Ok = ();
    type Error = WriteError;
    fn serialize_element<T: serde::Serialize + ?Sized>(&mut self, value: &T) -> Written {
        self.item(value)
    }
    fn end(self) -> Written {
        self.close()
    }
}

impl serde::ser::SerializeTupleStruct for Array<'_, '_> {
    type Ok = ();
    type Error = WriteError;
    fn serialize_field<T: serde::Serialize + ?Sized>(&mut self, value: &T) -> Written {
        self.item(value)
    }
    fn end(self) -> Written {
        self.close()
    }
}

impl serde::ser::SerializeTupleVariant for Array<'_, '_> {
    type Ok = ();
    type Error = WriteError;
    fn serialize_field<T: serde::Serialize + ?Sized>(&mut self, value: &T) -> Written {
        self.item(value)
    }
    fn end(self) -> Written {
        self.close()
    }
}

impl serde::ser::SerializeMap for Object<'_, '_> {
    type Ok = ();
    type Error = WriteError;
    fn serialize_key<T: serde::Serialize + ?Sized>(&mut self, key: &T) -> Written {
        let key = key.serialize(Key)?;
        self.key(key)
    }
    fn serialize_value<T: serde::Serialize + ?Sized>(&mut self, value: &T) -> Written {
        self.value(value)
    }
    fn end(self) -> Written {
        self.close()
    }
}

impl serde::ser::SerializeStruct for Object<'_, '_> {
    type Ok = ();
    type Error = WriteError;
    fn serialize_field<T: serde::Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Written {
        self.entry(key, value)
    }
    fn end(self) -> Written {
        self.close()
    }
}

impl serde::ser::SerializeStructVariant for Object<'_, '_> {
    type Ok = ();
    type Error = WriteError;
    fn serialize_field<T: serde::Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Written {
        self.entry(key, value)
    }
    fn end(self) -> Written {
        self.close()
    }
}
