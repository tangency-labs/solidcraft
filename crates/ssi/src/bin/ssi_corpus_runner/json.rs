//! Just enough JSON for the ssi-corpus cases: parse a document, read numbers,
//! strings, arrays and objects out of it, and write numbers back.

use std::fmt::Write;

#[derive(Debug, Clone)]
pub enum Json {
    Null,
    Bool(#[allow(dead_code)] bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            Json::Num(x) => Some(*x),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }
    /// A numeric array, or None when any item is not a number.
    pub fn nums(&self) -> Option<Vec<f64>> {
        self.arr()?.iter().map(Json::num).collect()
    }
}

pub fn parse(text: &str) -> Result<Json, String> {
    let mut parser = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    let value = parser.value()?;
    parser.ws();
    if parser.i != parser.s.len() {
        return Err(format!("trailing text at byte {}", parser.i));
    }
    Ok(value)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    /// The input from the cursor on.
    fn rest(&self) -> &[u8] {
        self.s.get(self.i..).unwrap_or_default()
    }

    fn ws(&mut self) {
        while matches!(self.s.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn expect(&mut self, word: &str) -> Result<(), String> {
        if self.rest().starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(())
        } else {
            Err(format!("expected {word} at byte {}", self.i))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.s.get(self.i) {
            None => Err("unexpected end".into()),
            Some(b'n') => self.expect("null").map(|()| Json::Null),
            Some(b't') => self.expect("true").map(|()| Json::Bool(true)),
            Some(b'f') => self.expect("false").map(|()| Json::Bool(false)),
            // Python's json writes these for non-finite floats.
            Some(b'N') => self.expect("NaN").map(|()| Json::Num(f64::NAN)),
            Some(b'I') => self.expect("Infinity").map(|()| Json::Num(f64::INFINITY)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return Err(format!("expected , or ] at byte {}", self.i)),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut fields = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(fields));
                }
                loop {
                    self.ws();
                    let key = self.string()?;
                    self.ws();
                    self.expect(":")?;
                    let value = self.value()?;
                    fields.push((key, value));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(fields));
                        }
                        _ => return Err(format!("expected , or }} at byte {}", self.i)),
                    }
                }
            }
            Some(_) => self.number(),
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        if self.rest().starts_with(b"-Infinity") {
            self.i += 9;
            return Ok(Json::Num(f64::NEG_INFINITY));
        }
        while matches!(
            self.s.get(self.i),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.i += 1;
        }
        let digits = self.s.get(start..self.i).unwrap_or_default();
        let text = std::str::from_utf8(digits).map_err(|e| e.to_string())?;
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("bad number {text:?} at byte {start}"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let text = self.s.get(self.i..self.i + 4).ok_or("short \\u escape")?;
        self.i += 4;
        u32::from_str_radix(std::str::from_utf8(text).map_err(|e| e.to_string())?, 16)
            .map_err(|e| e.to_string())
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect("\"")?;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return Err("unterminated string".into());
            };
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|e| e.to_string()),
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return Err("unterminated escape".into());
                    };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let high = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&high)
                                && self.rest().starts_with(b"\\u")
                            {
                                self.i += 2;
                                let low = self.hex4()?;
                                0x10000
                                    + ((high - 0xd800) << 10)
                                    + (low.wrapping_sub(0xdc00) & 0x3ff)
                            } else {
                                high
                            };
                            char::from_u32(code).unwrap_or('\u{fffd}')
                        }
                        _ => return Err(format!("bad escape at byte {}", self.i)),
                    };
                    let mut buf = [0; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
    }
}

/// A number as JSON: shortest round-trip form, null when not finite.
pub fn num(out: &mut String, x: f64) {
    if x.is_finite() {
        let _ = write!(out, "{x:e}");
    } else {
        out.push_str("null");
    }
}

pub fn point(out: &mut String, p: &[f64]) {
    out.push('[');
    for (k, &x) in p.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        num(out, x);
    }
    out.push(']');
}

pub fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
