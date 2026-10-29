//! ISO 10303-21 ("Part 21", the STEP clear-text exchange structure) reader.
//!
//! Written from the published file syntax: a `HEADER` section of instances without
//! ids, one or more `DATA` sections of `#id = INSTANCE;` lines, where an instance is
//! simple (`NAME(params)`) or complex (`(A(params) B(params) ...)`), parameters are
//! integers, reals, strings, enumerations (`.T.`), binaries (`"..."`), references
//! (`#n`), unset (`$`), derived (`*`), lists and typed values (`NAME(param)`).
//! Comments are `/* ... */`.
//!
//! Untrusted input: the entity count and list nesting are bounded by [`Limits`], a
//! duplicate id is an error, and nothing is allocated from a count read out of the
//! file (lists grow as their items are read). Strings are kept raw (the `\X\`,
//! `\X2\` encodings are not decoded; no geometry depends on them).

use std::collections::BTreeMap;
use std::fmt;

/// Bounds applied to untrusted input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most `#id = ...` instances accepted in one file.
    pub max_entities: usize,
    /// Deepest nesting of parameter lists and typed parameters.
    pub max_list_depth: usize,
    /// Deepest chain of references the mapper follows from a representation item
    /// (e.g. surface of revolution -> curve -> ...). Cycles stop here.
    pub max_ref_depth: usize,
    /// Most parameters in one list (and most items in any mapped list).
    pub max_list_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entities: 4_000_000,
            max_list_depth: 32,
            max_ref_depth: 64,
            max_list_len: 1_000_000,
        }
    }
}

/// A Part 21 syntax or limit error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line of the offending token.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// One parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    /// An integer.
    Int(i64),
    /// A real.
    Real(f64),
    /// A string, raw (quotes removed, `''` unescaped, control directives kept).
    Str(String),
    /// An enumeration value without its dots, e.g. `T`, `MILLI`.
    Enum(String),
    /// A binary value, raw hex digits.
    Binary(String),
    /// A reference `#n`.
    Ref(u64),
    /// `$`: unset optional attribute.
    Null,
    /// `*`: attribute derived in a subtype.
    Derived,
    /// `( ... )`.
    List(Vec<Param>),
    /// `NAME(value)`, e.g. `LENGTH_MEASURE(25.4)`.
    Typed(String, Box<Param>),
}

impl Param {
    /// The number, for an integer or a real (or a typed number such as
    /// `LENGTH_MEASURE(1.)`).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Param::Real(x) => Some(*x),
            Param::Int(i) => Some(*i as f64),
            Param::Typed(_, p) => p.as_f64(),
            _ => None,
        }
    }

    /// The reference id.
    pub fn as_ref_id(&self) -> Option<u64> {
        match self {
            Param::Ref(r) => Some(*r),
            _ => None,
        }
    }

    /// The list items.
    pub fn as_list(&self) -> Option<&[Param]> {
        match self {
            Param::List(l) => Some(l),
            _ => None,
        }
    }

    /// The enumeration value.
    pub fn as_enum(&self) -> Option<&str> {
        match self {
            Param::Enum(e) => Some(e),
            _ => None,
        }
    }
}

/// One entity record: a name and its parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Entity name, upper case as written.
    pub name: String,
    /// Parameters in order.
    pub params: Vec<Param>,
}

/// One instance of the data section.
#[derive(Debug, Clone, PartialEq)]
pub enum Instance {
    /// `NAME(params)`.
    Simple(Record),
    /// `(A(params) B(params) ...)`: one record per supertype/subtype, in file order.
    Complex(Vec<Record>),
}

impl Instance {
    /// The record named `name`: the simple record if it has that name, or that part
    /// of a complex instance.
    pub fn record(&self, name: &str) -> Option<&Record> {
        match self {
            Instance::Simple(r) => (r.name == name).then_some(r),
            Instance::Complex(rs) => rs.iter().find(|r| r.name == name),
        }
    }

    /// The simple record's name, or for a complex instance the names joined by `+`.
    pub fn type_name(&self) -> String {
        match self {
            Instance::Simple(r) => r.name.clone(),
            Instance::Complex(rs) => rs
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join("+"),
        }
    }
}

/// The metadata from the Part 21 `HEADER` section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// `FILE_DESCRIPTION` descriptions.
    pub descriptions: Vec<String>,
    /// `FILE_DESCRIPTION` implementation level.
    pub implementation_level: String,
    /// `FILE_NAME` name.
    pub file_name: String,
    /// `FILE_NAME` time stamp.
    pub time_stamp: String,
    /// `FILE_NAME` authors.
    pub authors: Vec<String>,
    /// `FILE_NAME` organizations.
    pub organizations: Vec<String>,
    /// `FILE_NAME` preprocessor version.
    pub preprocessor_version: String,
    /// `FILE_NAME` originating system.
    pub originating_system: String,
    /// `FILE_NAME` authorization.
    pub authorization: String,
}

/// A parsed exchange file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepFile {
    /// Header metadata.
    pub header: Header,
    /// `FILE_SCHEMA` names, e.g. `CONFIG_CONTROL_DESIGN` (AP203) or
    /// `AUTOMOTIVE_DESIGN` (AP214).
    pub schemas: Vec<String>,
    /// Every data-section instance by id.
    pub entities: BTreeMap<u64, Instance>,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Keyword(String),
    Id(u64),
    Int(i64),
    Real(f64),
    Str(String),
    Enum(String),
    Binary(String),
    LParen,
    RParen,
    Comma,
    Semi,
    Eq,
    Dollar,
    Star,
    Eof,
}

struct Lexer<'a> {
    s: &'a [u8],
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            line: self.line,
            message: message.into(),
        })
    }

    fn peek_byte(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn skip_ws(&mut self) -> Result<(), ParseError> {
        loop {
            match self.peek_byte() {
                Some(b'\n') => {
                    self.line += 1;
                    self.pos += 1;
                }
                Some(c) if c.is_ascii_whitespace() => self.pos += 1,
                Some(b'/') if self.s.get(self.pos + 1) == Some(&b'*') => {
                    self.pos += 2;
                    loop {
                        match self.peek_byte() {
                            None => return self.err("unterminated comment"),
                            Some(b'*') if self.s.get(self.pos + 1) == Some(&b'/') => {
                                self.pos += 2;
                                break;
                            }
                            Some(b'\n') => {
                                self.line += 1;
                                self.pos += 1;
                            }
                            Some(_) => self.pos += 1,
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn take_while(&mut self, f: impl Fn(u8) -> bool) -> &'a [u8] {
        let start = self.pos;
        while self.peek_byte().is_some_and(&f) {
            self.pos += 1;
        }
        self.s.get(start..self.pos).unwrap_or_default()
    }

    fn next(&mut self) -> Result<Tok, ParseError> {
        self.skip_ws()?;
        let Some(c) = self.peek_byte() else {
            return Ok(Tok::Eof);
        };
        let single = match c {
            b'(' => Some(Tok::LParen),
            b')' => Some(Tok::RParen),
            b',' => Some(Tok::Comma),
            b';' => Some(Tok::Semi),
            b'=' => Some(Tok::Eq),
            b'$' => Some(Tok::Dollar),
            b'*' => Some(Tok::Star),
            _ => None,
        };
        if let Some(t) = single {
            self.pos += 1;
            return Ok(t);
        }
        match c {
            b'#' => {
                self.pos += 1;
                let d = self.take_while(|b| b.is_ascii_digit());
                let n = std::str::from_utf8(d)
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok());
                match n {
                    Some(n) => Ok(Tok::Id(n)),
                    None => self.err("bad entity id"),
                }
            }
            b'\'' => {
                self.pos += 1;
                let mut out = Vec::new();
                loop {
                    match self.peek_byte() {
                        None => return self.err("unterminated string"),
                        Some(b'\'') => {
                            if self.s.get(self.pos + 1) == Some(&b'\'') {
                                out.push(b'\'');
                                self.pos += 2;
                            } else {
                                self.pos += 1;
                                break;
                            }
                        }
                        Some(b) => {
                            if b == b'\n' {
                                self.line += 1;
                            }
                            out.push(b);
                            self.pos += 1;
                        }
                    }
                }
                Ok(Tok::Str(String::from_utf8_lossy(&out).into_owned()))
            }
            b'"' => {
                self.pos += 1;
                let d = self.take_while(|b| b != b'"');
                let v = String::from_utf8_lossy(d).into_owned();
                if self.peek_byte() != Some(b'"') {
                    return self.err("unterminated binary");
                }
                self.pos += 1;
                Ok(Tok::Binary(v))
            }
            b'.' if self
                .s
                .get(self.pos + 1)
                .is_some_and(u8::is_ascii_alphabetic) =>
            {
                self.pos += 1;
                let d = self.take_while(|b| b.is_ascii_alphanumeric() || b == b'_');
                let v = String::from_utf8_lossy(d).into_owned();
                if self.peek_byte() != Some(b'.') {
                    return self.err("unterminated enumeration");
                }
                self.pos += 1;
                Ok(Tok::Enum(v))
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number(),
            b'!' | b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                let d = self.take_while(|b| {
                    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'!'
                });
                Ok(Tok::Keyword(
                    String::from_utf8_lossy(d).to_ascii_uppercase(),
                ))
            }
            other => self.err(format!("unexpected character 0x{other:02x}")),
        }
    }

    fn number(&mut self) -> Result<Tok, ParseError> {
        let start = self.pos;
        if matches!(self.peek_byte(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let mut digits = !self.take_while(|b| b.is_ascii_digit()).is_empty();
        let mut real = false;
        if self.peek_byte() == Some(b'.') {
            real = true;
            self.pos += 1;
            digits |= !self.take_while(|b| b.is_ascii_digit()).is_empty();
        }
        if matches!(self.peek_byte(), Some(b'E' | b'e')) {
            real = true;
            self.pos += 1;
            if matches!(self.peek_byte(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.take_while(|b| b.is_ascii_digit());
        }
        let text = self
            .s
            .get(start..self.pos)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("");
        if real && !digits {
            // `.`, `-.`, `-.E3`: no mantissa digit, not a number (would parse as 0).
            return self.err(format!("bad real '{text}'"));
        }
        if real {
            // Rust's parser rejects "1." followed by an exponent only in some forms;
            // normalise "1.E5" / "1." to "1.0E5" / "1.0".
            let fixed = text.replacen(".E", ".0E", 1).replacen(".e", ".0e", 1);
            let fixed = if fixed.ends_with('.') {
                format!("{fixed}0")
            } else {
                fixed
            };
            match fixed.parse::<f64>() {
                Ok(x) if x.is_finite() => Ok(Tok::Real(x)),
                _ => self.err(format!("bad real '{text}'")),
            }
        } else {
            match text.parse::<i64>() {
                Ok(i) => Ok(Tok::Int(i)),
                Err(_) => self.err(format!("bad integer '{text}'")),
            }
        }
    }
}

struct Parser<'a> {
    lx: Lexer<'a>,
    tok: Tok,
    limits: Limits,
}

impl<'a> Parser<'a> {
    fn new(s: &'a [u8], limits: Limits) -> Result<Self, ParseError> {
        let mut lx = Lexer { s, pos: 0, line: 1 };
        let tok = lx.next()?;
        Ok(Self { lx, tok, limits })
    }

    fn bump(&mut self) -> Result<Tok, ParseError> {
        let next = self.lx.next()?;
        Ok(std::mem::replace(&mut self.tok, next))
    }

    fn expect(&mut self, t: &Tok, what: &str) -> Result<(), ParseError> {
        if &self.tok == t {
            self.bump()?;
            Ok(())
        } else {
            self.lx
                .err(format!("expected {what}, found {:?}", self.tok))
        }
    }

    fn keyword(&mut self) -> Result<String, ParseError> {
        match self.bump()? {
            Tok::Keyword(k) => Ok(k),
            other => self.lx.err(format!("expected a keyword, found {other:?}")),
        }
    }

    /// `( p, p, ... )` with the opening paren current.
    fn list(&mut self, depth: usize) -> Result<Vec<Param>, ParseError> {
        if depth > self.limits.max_list_depth {
            return self.lx.err("parameter lists nested too deeply");
        }
        self.expect(&Tok::LParen, "'('")?;
        let mut out = Vec::new();
        if self.tok == Tok::RParen {
            self.bump()?;
            return Ok(out);
        }
        loop {
            if out.len() >= self.limits.max_list_len {
                return self.lx.err("parameter list too long");
            }
            out.push(self.param(depth)?);
            match self.bump()? {
                Tok::Comma => {}
                Tok::RParen => return Ok(out),
                other => return self.lx.err(format!("expected ',' or ')', found {other:?}")),
            }
        }
    }

    fn param(&mut self, depth: usize) -> Result<Param, ParseError> {
        if depth > self.limits.max_list_depth {
            return self.lx.err("parameter lists nested too deeply");
        }
        Ok(match self.tok.clone() {
            Tok::LParen => Param::List(self.list(depth + 1)?),
            Tok::Keyword(k) => {
                self.bump()?;
                self.expect(&Tok::LParen, "'(' after a typed parameter")?;
                let inner = self.param(depth + 1)?;
                self.expect(&Tok::RParen, "')' closing a typed parameter")?;
                Param::Typed(k, Box::new(inner))
            }
            Tok::Id(i) => {
                self.bump()?;
                Param::Ref(i)
            }
            Tok::Int(i) => {
                self.bump()?;
                Param::Int(i)
            }
            Tok::Real(x) => {
                self.bump()?;
                Param::Real(x)
            }
            Tok::Str(s) => {
                self.bump()?;
                Param::Str(s)
            }
            Tok::Enum(e) => {
                self.bump()?;
                Param::Enum(e)
            }
            Tok::Binary(b) => {
                self.bump()?;
                Param::Binary(b)
            }
            Tok::Dollar => {
                self.bump()?;
                Param::Null
            }
            Tok::Star => {
                self.bump()?;
                Param::Derived
            }
            other => return self.lx.err(format!("unexpected {other:?} in parameters")),
        })
    }

    fn record(&mut self) -> Result<Record, ParseError> {
        let name = self.keyword()?;
        let params = self.list(1)?;
        Ok(Record { name, params })
    }

    fn instance(&mut self) -> Result<Instance, ParseError> {
        if self.tok == Tok::LParen {
            self.bump()?;
            let mut parts = Vec::new();
            while self.tok != Tok::RParen {
                if parts.len() >= self.limits.max_list_len {
                    return self.lx.err("complex instance too long");
                }
                parts.push(self.record()?);
            }
            self.bump()?;
            if parts.is_empty() {
                return self.lx.err("empty complex instance");
            }
            Ok(Instance::Complex(parts))
        } else {
            Ok(Instance::Simple(self.record()?))
        }
    }
}

/// Parses a whole exchange file.
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<StepFile, ParseError> {
    let mut p = Parser::new(bytes, *limits)?;
    let mut out = StepFile::default();
    match p.bump()? {
        Tok::Keyword(k) if k == "ISO-10303-21" => {}
        other => {
            return p
                .lx
                .err(format!("not a Part 21 file (starts with {other:?})"))
        }
    }
    p.expect(&Tok::Semi, "';'")?;
    loop {
        let kw = match p.bump()? {
            Tok::Keyword(k) => k,
            Tok::Eof => return p.lx.err("missing END-ISO-10303-21"),
            other => return p.lx.err(format!("expected a section, found {other:?}")),
        };
        match kw.as_str() {
            "END-ISO-10303-21" => {
                p.expect(&Tok::Semi, "';'")?;
                return Ok(out);
            }
            "HEADER" => {
                p.expect(&Tok::Semi, "';'")?;
                while p.tok != Tok::Keyword("ENDSEC".into()) {
                    let r = p.record()?;
                    p.expect(&Tok::Semi, "';' after a header entity")?;
                    match r.name.as_str() {
                        "FILE_SCHEMA" => {
                            if let Some(Param::List(l)) = r.params.first() {
                                for s in l {
                                    if let Param::Str(s) = s {
                                        out.schemas.push(s.clone());
                                    }
                                }
                            }
                        }
                        "FILE_DESCRIPTION" => {
                            if let Some(Param::List(l)) = r.params.first() {
                                out.header.descriptions = l
                                    .iter()
                                    .filter_map(|p| match p {
                                        Param::Str(s) => Some(s.clone()),
                                        _ => None,
                                    })
                                    .collect();
                            }
                            if let Some(Param::Str(s)) = r.params.get(1) {
                                out.header.implementation_level.clone_from(s);
                            }
                        }
                        "FILE_NAME" => {
                            let string = |i: usize| match r.params.get(i) {
                                Some(Param::Str(s)) => s.clone(),
                                _ => String::new(),
                            };
                            let strings = |i: usize| match r.params.get(i) {
                                Some(Param::List(l)) => l
                                    .iter()
                                    .filter_map(|p| match p {
                                        Param::Str(s) => Some(s.clone()),
                                        _ => None,
                                    })
                                    .collect(),
                                _ => Vec::new(),
                            };
                            out.header.file_name = string(0);
                            out.header.time_stamp = string(1);
                            out.header.authors = strings(2);
                            out.header.organizations = strings(3);
                            out.header.preprocessor_version = string(4);
                            out.header.originating_system = string(5);
                            out.header.authorization = string(6);
                        }
                        _ => {}
                    }
                }
                p.bump()?;
                p.expect(&Tok::Semi, "';'")?;
            }
            "DATA" => {
                // Edition 3 allows DATA('name', (schemas)); skip the parameters.
                if p.tok == Tok::LParen {
                    p.list(1)?;
                }
                p.expect(&Tok::Semi, "';'")?;
                loop {
                    match p.bump()? {
                        Tok::Keyword(k) if k == "ENDSEC" => break,
                        Tok::Id(id) => {
                            p.expect(&Tok::Eq, "'='")?;
                            let inst = p.instance()?;
                            p.expect(&Tok::Semi, "';' after an instance")?;
                            if out.entities.len() >= limits.max_entities {
                                return p
                                    .lx
                                    .err(format!("more than {} entities", limits.max_entities));
                            }
                            if out.entities.insert(id, inst).is_some() {
                                return p.lx.err(format!("duplicate entity #{id}"));
                            }
                        }
                        other => return p.lx.err(format!("expected '#id =', found {other:?}")),
                    }
                }
                p.expect(&Tok::Semi, "';'")?;
            }
            other => return p.lx.err(format!("unknown section {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('x'),'2;1');\n\
        FILE_NAME('t','2026-01-01',(''),(''),'','','');\nFILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\n\
        ENDSEC;\nDATA;\n/* a comment */\n#1=CARTESIAN_POINT('it''s',(1.,-2.5E-1,3));\n\
        #2=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n\
        #3=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#2);\n#4=X($,\"0F\",((1,2),()));\n\
        ENDSEC;\nEND-ISO-10303-21;\n";

    #[test]
    fn parses_tiny_file() {
        let f = parse(TINY.as_bytes(), &Limits::default()).unwrap();
        assert_eq!(f.schemas, vec!["CONFIG_CONTROL_DESIGN".to_string()]);
        assert_eq!(f.header.file_name, "t");
        assert_eq!(f.header.time_stamp, "2026-01-01");
        assert_eq!(f.header.descriptions, vec!["x".to_string()]);
        assert_eq!(f.header.implementation_level, "2;1");
        assert_eq!(f.entities.len(), 4);
        let Instance::Simple(r) = &f.entities[&1] else {
            panic!()
        };
        assert_eq!(r.name, "CARTESIAN_POINT");
        assert_eq!(r.params[0], Param::Str("it's".into()));
        assert_eq!(
            r.params[1],
            Param::List(vec![Param::Real(1.0), Param::Real(-0.25), Param::Int(3)])
        );
        let c = &f.entities[&2];
        assert!(c.record("SI_UNIT").is_some());
        assert_eq!(c.type_name(), "LENGTH_UNIT+NAMED_UNIT+SI_UNIT");
        let Instance::Simple(r) = &f.entities[&3] else {
            panic!()
        };
        assert_eq!(r.params[0].as_f64(), Some(25.4));
        let Instance::Simple(r) = &f.entities[&4] else {
            panic!()
        };
        assert_eq!(r.params[0], Param::Null);
        assert_eq!(r.params[1], Param::Binary("0F".into()));
    }

    #[test]
    fn rejects_bad_input() {
        let lim = Limits::default();
        assert!(parse(b"", &lim).is_err());
        assert!(parse(
            b"ISO-10303-21;\nDATA;\n#1=A(;\nENDSEC;END-ISO-10303-21;",
            &lim
        )
        .is_err());
        assert!(parse(
            b"ISO-10303-21;DATA;#1=A();#1=B();ENDSEC;END-ISO-10303-21;",
            &lim
        )
        .is_err());
        assert!(parse(b"ISO-10303-21;DATA;#1=A('x);ENDSEC;END-ISO-10303-21;", &lim).is_err());
        let deep = format!(
            "ISO-10303-21;DATA;#1=A({}{});ENDSEC;END-ISO-10303-21;",
            "(".repeat(100),
            ")".repeat(100)
        );
        assert!(parse(deep.as_bytes(), &lim).is_err());
        let few = Limits {
            max_entities: 1,
            ..lim
        };
        assert!(parse(
            b"ISO-10303-21;DATA;#1=A();#2=B();ENDSEC;END-ISO-10303-21;",
            &few
        )
        .is_err());
    }

    #[test]
    fn number_forms() {
        let f = parse(
            b"ISO-10303-21;DATA;#1=A(1.,1.E2,-.5,+3,1.5e-3);ENDSEC;END-ISO-10303-21;",
            &Limits::default(),
        )
        .unwrap();
        let Instance::Simple(r) = &f.entities[&1] else {
            panic!()
        };
        let v: Vec<f64> = r.params.iter().map(|p| p.as_f64().unwrap()).collect();
        assert_eq!(v, vec![1.0, 100.0, -0.5, 3.0, 1.5e-3]);
    }

    #[test]
    fn rejects_numbers_without_digits() {
        // A real needs at least one mantissa digit; these read as 0 before.
        let accepted: Vec<String> = [".", "-.", "+.", ".E5", "-.e-3", "(1.,.,3.)"]
            .into_iter()
            .filter_map(|bad| {
                let src = format!("ISO-10303-21;DATA;#1=A({bad});ENDSEC;END-ISO-10303-21;");
                let f = parse(src.as_bytes(), &Limits::default()).ok()?;
                Some(format!("`{bad}` -> {:?}", f.entities[&1]))
            })
            .collect();
        assert!(accepted.is_empty(), "accepted: {accepted:#?}");
        // The error names the token and its line.
        let e = parse(
            b"ISO-10303-21;\nDATA;\n#1=A(.);\nENDSEC;END-ISO-10303-21;",
            &Limits::default(),
        )
        .unwrap_err();
        assert_eq!(e.line, 3);
        assert!(e.message.contains("bad real '.'"), "{e}");
    }

    #[test]
    fn typed_nesting_refusal_at_small_configured_limit() {
        // Every entry route reaches the same parameter parser. Keep these inputs small:
        // this tests the documented refusal, not an actual stack overflow.
        let cases = [
            ("simple", 2, "DATA;#1=X(T(T(1)));ENDSEC;"),
            ("complex", 2, "DATA;#1=(X(T(T(1)))Y());ENDSEC;"),
            ("header", 2, "HEADER;X(T(T(1)));ENDSEC;DATA;ENDSEC;"),
            ("data parameters", 3, "DATA('name',(T(T(1))));ENDSEC;"),
        ];
        let mut accepted = Vec::new();
        for (name, max_list_depth, section) in cases {
            let lim = Limits {
                max_list_depth,
                ..Limits::default()
            };
            let input = format!("ISO-10303-21;{section}END-ISO-10303-21;");
            match parse(input.as_bytes(), &lim) {
                Ok(_) => accepted.push(name),
                Err(error) => {
                    assert_eq!(error.message, "parameter lists nested too deeply", "{name}");
                }
            }
        }
        assert!(
            accepted.is_empty(),
            "over-limit typed parameters accepted: {accepted:?}"
        );
    }

    #[test]
    fn typed_nesting_accepted_boundary() {
        let lim = Limits {
            max_list_depth: 2,
            ..Limits::default()
        };
        let expected = Param::Typed("T".into(), Box::new(Param::Int(1)));
        for body in ["#1=X(T(1));", "#1=(X(T(1))Y());"] {
            let input = format!("ISO-10303-21;DATA;{body}ENDSEC;END-ISO-10303-21;");
            let f = parse(input.as_bytes(), &lim).unwrap();
            let r = f.entities[&1].record("X").unwrap();
            assert_eq!(r.params, vec![expected.clone()]);
            assert_eq!(r.params[0].as_f64(), Some(1.0));
        }
        let f = parse(
            b"ISO-10303-21;DATA;#1=X((1));ENDSEC;END-ISO-10303-21;",
            &lim,
        )
        .unwrap();
        assert_eq!(
            f.entities[&1].record("X").unwrap().params,
            vec![Param::List(vec![Param::Int(1)])]
        );
        for (max_list_depth, section) in [
            (2, "HEADER;X(T(1));ENDSEC;DATA;ENDSEC;"),
            (3, "DATA('name',(T(1)));ENDSEC;"),
        ] {
            let lim = Limits {
                max_list_depth,
                ..lim
            };
            let input = format!("ISO-10303-21;{section}END-ISO-10303-21;");
            assert!(parse(input.as_bytes(), &lim).is_ok());
        }
    }

    #[test]
    fn typed_nesting_mixed_bypass() {
        let lim = Limits {
            max_list_depth: 3,
            ..Limits::default()
        };
        // Outer list 1, nested list 2, typed leaf 3: exactly at the bound.
        let ok = parse(
            b"ISO-10303-21;DATA;#1=X((T(1)));ENDSEC;END-ISO-10303-21;",
            &lim,
        )
        .unwrap();
        assert_eq!(
            ok.entities[&1].record("X").unwrap().params,
            vec![Param::List(vec![Param::Typed(
                "T".into(),
                Box::new(Param::Int(1))
            )])]
        );
        // One more typed wrapper must not bypass the bound after entering a list.
        let deep = parse(
            b"ISO-10303-21;DATA;#1=X((T(T(1))));ENDSEC;END-ISO-10303-21;",
            &lim,
        );
        assert_eq!(
            deep.unwrap_err().message,
            "parameter lists nested too deeply"
        );
    }

    #[test]
    fn typed_nesting_preserves_existing_list_boundary() {
        let lim = Limits {
            max_list_depth: 2,
            ..Limits::default()
        };
        assert!(parse(
            b"ISO-10303-21;DATA;#1=X((1));ENDSEC;END-ISO-10303-21;",
            &lim
        )
        .is_ok());
        let error = parse(
            b"ISO-10303-21;DATA;#1=X(((1)));ENDSEC;END-ISO-10303-21;",
            &lim,
        )
        .unwrap_err();
        assert_eq!(error.message, "parameter lists nested too deeply");
    }

    #[test]
    fn typed_nesting_default_32_wrappers() {
        let lim = Limits::default();
        // 31 typed wrappers accepted, 32 rejected
        let mut s = String::from("ISO-10303-21;DATA;#1=X(");
        for _ in 0..31 {
            s.push_str("T(");
        }
        s.push('1');
        for _ in 0..31 {
            s.push(')');
        }
        s.push_str(");ENDSEC;END-ISO-10303-21;");
        assert!(parse(s.as_bytes(), &lim).is_ok());
        let mut s2 = String::from("ISO-10303-21;DATA;#1=X(");
        for _ in 0..32 {
            s2.push_str("T(");
        }
        s2.push('1');
        for _ in 0..32 {
            s2.push(')');
        }
        s2.push_str(");ENDSEC;END-ISO-10303-21;");
        assert!(parse(s2.as_bytes(), &lim).is_err());
    }
}
