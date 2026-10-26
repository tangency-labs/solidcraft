//! Parameter expressions: `2 * width + 5 mm`, `angle / 2`, `sqrt(a^2 + b^2)`, `atan2(h; w)`.
//!
//! Values carry a simple dimension (powers of length, angle and mass; a volume is a length³ and an
//! area a length²). Lengths are millimetres, angles radians and masses grams internally. A
//! unit-less number added to a dimensioned value takes that value's unit: the parameter's own
//! unit when it has one (`width + 5` adds 5 in for an inch parameter, `load + 1` 1 kg for a
//! kilogram one), else in its kind's default unit: millimetres, degrees, kilograms, mm³ and mm².
//! A unit-less final result takes the expected unit (`2` for an inch parameter is 2 in).
//! Trigonometric functions take an angle; a unit-less argument is read as degrees.

use std::collections::BTreeMap;

use crate::DocError;

const MAX_LEN: usize = 4096;
const MAX_DEPTH: usize = 64;
const MAX_ARGS: usize = 64;

/// An expression a feature holds, as the user wrote it (`2 * width + 5 mm`): kept as text and
/// evaluated with the design's parameters. It is saved as a plain string, and reads as one
/// (`&expr` is a `&str`); it changes only as a whole (`*expr = new.into()`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Expr(String);

impl Expr {
    pub fn new(s: impl Into<String>) -> Self {
        Expr(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::ops::Deref for Expr {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Expr {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for Expr {
    fn from(s: String) -> Self {
        Expr(s)
    }
}

impl From<&str> for Expr {
    fn from(s: &str) -> Self {
        Expr(s.to_string())
    }
}

impl From<&String> for Expr {
    fn from(s: &String) -> Self {
        Expr(s.clone())
    }
}

impl From<Expr> for String {
    fn from(e: Expr) -> Self {
        e.0
    }
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for Expr {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Expr {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Expr> for String {
    fn eq(&self, other: &Expr) -> bool {
        *self == other.0
    }
}

impl PartialEq<String> for Expr {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

/// What an expression must evaluate to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Length,
    Angle,
    Unitless,
    Mass,
    Volume,
    Area,
}

impl Kind {
    /// Every kind there is.
    pub const ALL: [Kind; 6] = [Kind::Length, Kind::Angle, Kind::Unitless, Kind::Mass, Kind::Volume, Kind::Area];

    pub fn from_unit(u: &str) -> Kind {
        unit_info(u).map(|(k, _)| k).unwrap_or(Kind::Length)
    }
    /// The unit a new parameter of this kind gets (`mm`, `deg`, `kg`, `mm³`, `mm²` or none). A
    /// mass shows in kilograms, though it is worked out in grams.
    pub fn default_unit(self) -> &'static str {
        match self {
            Kind::Length => "mm",
            Kind::Angle => "deg",
            Kind::Unitless => "",
            Kind::Mass => "kg",
            Kind::Volume => "mm³",
            Kind::Area => "mm²",
        }
    }
    /// The powers of length, angle and mass of a value of this kind.
    fn dims(self) -> Dims {
        match self {
            Kind::Length => [1, 0, 0],
            Kind::Angle => [0, 1, 0],
            Kind::Unitless => [0, 0, 0],
            Kind::Mass => [0, 0, 1],
            Kind::Volume => [3, 0, 0],
            Kind::Area => [2, 0, 0],
        }
    }
    /// The kind of a value with these powers, if it is of one (a length times an angle is not).
    fn of_dims(dims: Dims) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.dims() == dims)
    }
}

/// Units a parameter or an expression may use: name, kind and size (mm, radians, grams, mm³ or
/// mm²). A unit's name can't be a parameter's, but an older design can have a parameter called a
/// new unit word: a change that adds words renames those as the design opens, with a format step
/// of its own that keeps its words (as `mass-volume-and-area-units.rs` does). Left out on purpose:
/// `l`, `L` and `t` (common names of a length or a thickness), powers in ASCII such as `m3` (an M3
/// screw; `cm^3` works) and `oz` (a mass or a volume?).
pub const UNITS: &[(&str, Kind, f64)] = &[
    ("mm", Kind::Length, 1.0),
    ("cm", Kind::Length, 10.0),
    ("m", Kind::Length, 1000.0),
    ("um", Kind::Length, 0.001),
    ("µm", Kind::Length, 0.001),
    ("in", Kind::Length, 25.4),
    ("inch", Kind::Length, 25.4),
    ("ft", Kind::Length, 304.8),
    ("yd", Kind::Length, 914.4),
    ("km", Kind::Length, 1_000_000.0),
    ("mil", Kind::Length, 0.0254),
    ("nm", Kind::Length, 1e-6),
    ("deg", Kind::Angle, std::f64::consts::PI / 180.0),
    ("rad", Kind::Angle, 1.0),
    ("g", Kind::Mass, 1.0),
    ("kg", Kind::Mass, 1000.0),
    ("lb", Kind::Mass, 453.592_37),
    ("ml", Kind::Volume, 1000.0),
    ("mm³", Kind::Volume, 1.0),
    ("cm³", Kind::Volume, 1000.0),
    ("m³", Kind::Volume, 1e9),
    ("mm²", Kind::Area, 1.0),
    ("cm²", Kind::Area, 100.0),
    ("m²", Kind::Area, 1e6),
];

/// Functions an expression may call (name, signature) — for autocomplete and help.
pub const FUNCTIONS: &[(&str, &str)] = &[
    ("sin", "sin(angle)"),
    ("cos", "cos(angle)"),
    ("tan", "tan(angle)"),
    ("asin", "asin(x) → angle"),
    ("acos", "acos(x) → angle"),
    ("atan", "atan(x) → angle"),
    ("atan2", "atan2(y, x) → angle"),
    ("sqrt", "sqrt(x)"),
    ("abs", "abs(x)"),
    ("min", "min(a, b, …)"),
    ("max", "max(a, b, …)"),
    ("floor", "floor(x)"),
    ("ceil", "ceil(x)"),
    ("round", "round(x)"),
    ("exp", "exp(x)"),
    ("sinh", "sinh(x)"),
    ("cosh", "cosh(x)"),
    ("tanh", "tanh(x)"),
    ("asinh", "asinh(x)"),
    ("acosh", "acosh(x), x ≥ 1"),
    ("atanh", "atanh(x), −1 < x < 1"),
    ("ln", "ln(x)"),
    ("log", "log(x) (base 10)"),
    ("log10", "log10(x)"),
    ("sign", "sign(x)"),
    ("hypot", "hypot(a, b)"),
    ("pow", "pow(x, n)"),
    ("trunc", "trunc(x)"),
    ("mod", "mod(a, b)"),
];

/// Constants an expression may use.
pub const CONSTANTS: &[&str] = &["pi", "PI"];

/// Kind and size (mm, radians, grams, mm³ or mm² per unit) of a parameter's unit; empty means
/// unit-less.
pub fn unit_info(u: &str) -> Option<(Kind, f64)> {
    match u.trim() {
        "" | "none" | "unitless" => Some((Kind::Unitless, 1.0)),
        "°" | "degree" | "degrees" => Some((Kind::Angle, std::f64::consts::PI / 180.0)),
        t => UNITS.iter().find(|(n, _, _)| *n == t).map(|(_, k, s)| (*k, *s)),
    }
}

/// Why `u` can't be a parameter's unit (None = it is one), naming every unit [`UNITS`] has.
pub fn unit_problem(u: &str) -> Option<String> {
    if unit_info(u).is_some() {
        return None;
    }
    let names: Vec<&str> = UNITS.iter().map(|(name, _, _)| *name).collect();
    Some(format!("unknown unit `{u}` ({} or none)", names.join(", ")))
}

/// How unit-less numbers are read: by default in each kind's default unit (`Kind::default_unit`:
/// millimetres, degrees, kilograms, mm³ and mm²), so a bare number means what it shows as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Defaults {
    /// Millimetres per unit-less length number.
    pub len: f64,
    /// Radians per unit-less angle number.
    pub ang: f64,
    /// Grams per unit-less mass number (1000 by default: kilograms).
    pub mass: f64,
    /// Cubic millimetres per unit-less volume number.
    pub vol: f64,
    /// Square millimetres per unit-less area number.
    pub area: f64,
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults { len: 1.0, ang: std::f64::consts::PI / 180.0, mass: 1000.0, vol: 1.0, area: 1.0 }
    }
}

impl Defaults {
    /// Defaults for a parameter with this unit (`in` reads bare numbers as inches, `kg` as
    /// kilograms); the document's length unit covers lengths for the other kinds.
    pub fn for_unit(unit: &str, doc_len: f64) -> Defaults {
        let mut d = Defaults { len: doc_len, ..Defaults::default() };
        match unit_info(unit) {
            Some((Kind::Length, s)) => d.len = s,
            Some((Kind::Angle, s)) => d.ang = s,
            Some((Kind::Mass, s)) => d.mass = s,
            Some((Kind::Volume, s)) => d.vol = s,
            Some((Kind::Area, s)) => d.area = s,
            _ => {}
        }
        d
    }
    /// The size of a unit-less number read as a value of kind `k`.
    fn scale(self, k: Kind) -> f64 {
        match k {
            Kind::Length => self.len,
            Kind::Angle => self.ang,
            Kind::Unitless => 1.0,
            Kind::Mass => self.mass,
            Kind::Volume => self.vol,
            Kind::Area => self.area,
        }
    }
}

/// A value's powers of length, angle and mass, in that order (`[-3, 0, 1]` is a density).
type Dims = [i8; 3];

/// The names of the powers, in [`Dims`] order.
const POWERS: [&str; 3] = ["length", "angle", "mass"];

/// A dimensioned value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Value {
    pub v: f64,
    pub len: i8,
    pub ang: i8,
    pub mass: i8,
}

impl Value {
    /// `x` (in mm, radians, grams, mm³ or mm²) as a value of kind `k`.
    pub fn of_kind(k: Kind, x: f64) -> Value {
        Value::with_dims(x, k.dims())
    }
    pub fn num(v: f64) -> Value {
        Value::of_kind(Kind::Unitless, v)
    }
    pub fn length(mm: f64) -> Value {
        Value::of_kind(Kind::Length, mm)
    }
    pub fn angle(rad: f64) -> Value {
        Value::of_kind(Kind::Angle, rad)
    }
    pub fn mass(g: f64) -> Value {
        Value::of_kind(Kind::Mass, g)
    }
    /// The value's powers. Units are compared through these alone, so a later power is one more
    /// field here.
    fn dims(&self) -> Dims {
        [self.len, self.ang, self.mass]
    }
    fn same_dims(&self, other: &Value) -> bool {
        self.dims() == other.dims()
    }
    /// A number with these powers.
    fn with_dims(v: f64, [len, ang, mass]: Dims) -> Value {
        Value { v, len, ang, mass }
    }
    pub fn unitless(&self) -> bool {
        self.dims() == [0; 3]
    }
    /// The value's kind, if it is a plain length, angle, mass, volume, area or number (a density
    /// is none of them).
    pub fn kind(&self) -> Option<Kind> {
        Kind::of_dims(self.dims())
    }
    /// Convert to the expected kind (mm, radians, grams, mm³ or mm²); unit-less numbers are read
    /// in the default units.
    pub fn to_kind(self, k: Kind) -> Result<f64, DocError> {
        self.to_kind_in(k, Defaults::default())
    }
    /// Convert to the expected kind, reading a unit-less result in the given defaults.
    pub fn to_kind_in(self, k: Kind, d: Defaults) -> Result<f64, DocError> {
        let out = if self.kind() == Some(k) {
            self.v
        } else if self.unitless() {
            self.v * d.scale(k)
        } else {
            let hint = if k == Kind::Unitless && self.dims() == Kind::Length.dims() {
                " (a count needs a unit-less value: divide by a length, e.g. `floor(L / (210 mm))`)"
            } else {
                ""
            };
            return Err(DocError::Expr(format!("expected {}, got {}{hint}", kind_word(k), self.describe())));
        };
        if out.is_finite() { Ok(out) } else { Err(DocError::Expr("result is not a finite number".into())) }
    }
    /// "a length", "an angle", "a value in length^-3·mass"…
    pub fn describe(&self) -> String {
        match self.kind() {
            Some(k) => kind_word(k).to_string(),
            None => {
                let pow = |name: &str, n: i8| match n {
                    0 => String::new(),
                    1 => name.to_string(),
                    _ => format!("{name}^{n}"),
                };
                let parts: Vec<String> = POWERS.into_iter().zip(self.dims()).map(|(name, n)| pow(name, n)).filter(|s| !s.is_empty()).collect();
                format!("a value in {}", parts.join("·"))
            }
        }
    }
}

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Length => "a length",
        Kind::Angle => "an angle",
        Kind::Unitless => "a unit-less number",
        Kind::Mass => "a mass",
        Kind::Volume => "a volume",
        Kind::Area => "an area",
    }
}

/// A number in the given unit (e.g. 50.8 mm in `in` is 2, 500 g in `kg` 0.5); a unit of another
/// kind than the value's, or an unknown one, leaves it as is.
pub fn value_in_unit(v: Value, unit: &str) -> f64 {
    match unit_info(unit) {
        Some((k, s)) if v.kind() == Some(k) && s != 0.0 => v.v / s,
        _ => v.v,
    }
}

fn unit(name: &str) -> Option<Value> {
    UNITS.iter().find(|(n, _, _)| *n == name).map(|(_, k, s)| Value::of_kind(*k, *s))
}

fn is_constant(id: &str) -> bool {
    CONSTANTS.contains(&id)
}

/// What kind of word the expression language reserves `name` as: `unit`, `constant` or
/// `function` (None: free for a parameter).
pub fn reserved_kind(name: &str) -> Option<&'static str> {
    if unit(name).is_some() {
        Some("unit")
    } else if is_constant(name) {
        Some("constant")
    } else if FUNCTIONS.iter().any(|(f, _)| *f == name) {
        Some("function")
    } else {
        None
    }
}

/// Is this a name the expression language reserves (unit, constant or function)?
pub fn is_reserved(name: &str) -> bool {
    reserved_kind(name).is_some()
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Id(String),
    Op(char),
}

/// Tokens with their byte ranges in the source.
fn lex_spans(s: &str) -> Result<Vec<(Tok, usize, usize)>, DocError> {
    if s.len() > MAX_LEN {
        return Err(DocError::Expr("expression too long".into()));
    }
    let cs: Vec<(usize, char)> = s.char_indices().collect();
    let at = |i: usize| cs.get(i).map(|x| x.0).unwrap_or(s.len());
    let mut i = 0;
    let mut out = Vec::new();
    // Parentheses open at this point, innermost last, true for a function call's: a comma directly
    // in a call separates its arguments, and anywhere else can only be a decimal comma (#9408).
    let mut parens: Vec<bool> = Vec::new();
    // A single leading `=` marks the text as a formula (`=d3`, #4681) and is not part of it.
    if let Some(k) = cs.iter().position(|(_, c)| !c.is_whitespace())
        && cs.get(k).is_some_and(|(_, c)| *c == '=')
    {
        i = k + 1;
    }
    // A value typed as a definition, `name = expression` (#4071), is worth its expression: the
    // name is what it defines, not part of it (`Document::define_inline` stores it apart).
    if i == 0
        && definition(s).is_some()
        && let Some(eq) = s.find('=')
    {
        i = cs.iter().position(|(b, _)| *b > eq).unwrap_or(cs.len());
    }
    while let Some(&(_, c)) = cs.get(i) {
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' || leading_decimal_comma(&cs, i, in_call(&parens), out.last()) {
            let st = i;
            // A leading decimal comma (`,5` for 0,5, #2966) starts the number as `.5` would.
            let leading_comma = c == ',';
            if leading_comma {
                i += 1;
            }
            while cs.get(i).is_some_and(|(_, c)| c.is_ascii_digit() || *c == '.') {
                i += 1;
            }
            // A decimal comma (`2,5 mm`, as typed with a numeric keypad in many locales, #1347):
            // a comma between digits, in a number with no point, not directly in a function call
            // (there a comma separates arguments: `max(1,2)`).
            let digit = |k: usize| cs.get(k).is_some_and(|(_, c)| c.is_ascii_digit());
            let has_point = cs.get(st..i).is_some_and(|ds| ds.iter().any(|(_, c)| *c == '.'));
            if !in_call(&parens)
                && !leading_comma
                && !has_point
                && digit(i.wrapping_sub(1))
                && cs.get(i).is_some_and(|(_, c)| *c == ',')
                && digit(i + 1)
            {
                i += 1;
                while digit(i) {
                    i += 1;
                }
            }
            // Exponent: 1e-3 (but not `2em`).
            if cs.get(i).is_some_and(|(_, c)| *c == 'e' || *c == 'E') {
                let sign = cs.get(i + 1).is_some_and(|(_, c)| *c == '-' || *c == '+');
                let digit_at = if sign { i + 2 } else { i + 1 };
                if cs.get(digit_at).is_some_and(|(_, c)| c.is_ascii_digit()) {
                    i = digit_at;
                    while cs.get(i).is_some_and(|(_, c)| c.is_ascii_digit()) {
                        i += 1;
                    }
                }
            }
            let t = s.get(at(st)..at(i)).unwrap_or_default();
            let v: f64 = t.replace(',', ".").parse().map_err(|_| DocError::Expr(format!("bad number `{t}`")))?;
            // A mixed number, as imperial sizes are written (`2 1/8"` is 2.125 in, #1802): a whole
            // number, then a proper fraction of whole numbers, is one value, not `2 × 1/8`.
            if t.chars().all(|c| c.is_ascii_digit())
                && let Some((fraction, end)) = mixed_fraction(&cs, i, in_call(&parens))
            {
                out.push((Tok::Num(v + fraction), at(st), at(end)));
                i = end;
                continue;
            }
            out.push((Tok::Num(v), at(st), at(i)));
        } else if c.is_alphabetic() || c == '_' || c == 'µ' {
            let st = i;
            while cs.get(i).is_some_and(|(_, c)| c.is_alphanumeric() || *c == '_') {
                i += 1;
            }
            out.push((Tok::Id(s.get(at(st)..at(i)).unwrap_or_default().to_string()), at(st), at(i)));
        } else if c == '°' {
            out.push((Tok::Id("deg".into()), at(i), at(i + 1)));
            i += 1;
        } else if c == '"' || c == '″' {
            // Inch and foot marks (`1"`, `2'`, #1502), straight or as primes.
            out.push((Tok::Id("in".into()), at(i), at(i + 1)));
            i += 1;
        } else if c == '\'' || c == '′' {
            out.push((Tok::Id("ft".into()), at(i), at(i + 1)));
            i += 1;
        } else if c == '×' || c == '·' {
            out.push((Tok::Op('*'), at(i), at(i + 1)));
            i += 1;
        } else if c == '÷' {
            out.push((Tok::Op('/'), at(i), at(i + 1)));
            i += 1;
        } else if c == '−' {
            out.push((Tok::Op('-'), at(i), at(i + 1)));
            i += 1;
        } else if c == ';' {
            // Some CAD tools separate function arguments with `;` in some locales.
            out.push((Tok::Op(','), at(i), at(i + 1)));
            i += 1;
        } else if "+-*/^(),%".contains(c) {
            match c {
                '(' => parens.push(matches!(out.last(), Some((Tok::Id(_), _, _)))),
                ')' => {
                    parens.pop();
                }
                _ => {}
            }
            out.push((Tok::Op(c), at(i), at(i + 1)));
            i += 1;
        } else {
            return Err(DocError::Expr(format!("unexpected `{c}`")));
        }
    }
    Ok(out)
}

/// Whether the innermost open parenthesis is a function call's.
fn in_call(parens: &[bool]) -> bool {
    parens.last() == Some(&true)
}

/// Whether the comma at `i` starts a number, as a leading decimal comma (`,5` for 0,5, #2966).
/// It must not be directly in a function call (`in_call`: there a comma separates arguments, as
/// for any decimal comma), be followed by a digit, and stand where an operand is expected: at the
/// start or right after `( + - * / ^`. A comma after a number, name, `)` or `%` keeps its meaning
/// (`2 ,5` and `1,5,3` stay errors).
fn leading_decimal_comma(cs: &[(usize, char)], i: usize, in_call: bool, prev: Option<&(Tok, usize, usize)>) -> bool {
    !in_call
        && cs.get(i).is_some_and(|(_, c)| *c == ',')
        && cs.get(i + 1).is_some_and(|(_, c)| c.is_ascii_digit())
        && match prev {
            None => true,
            Some((Tok::Op(op), _, _)) => "(+-*/^".contains(*op),
            Some(_) => false,
        }
}

/// The proper fraction of a mixed number whose whole part ends at `i`: spaces, digits, `/`,
/// digits, with nothing after them that would continue the number (`2 1/8.5`, `2 1/8e3`, a
/// decimal comma: `2 1/8,5`). Its value and where it ends. `in_call`: directly in a function call
/// there (a comma separates arguments: `max(2 1/8,1)`).
fn mixed_fraction(cs: &[(usize, char)], mut i: usize, in_call: bool) -> Option<(f64, usize)> {
    let digits = |from: usize| cs.get(from..).map_or(0, |rest| rest.iter().take_while(|(_, c)| c.is_ascii_digit()).count());
    let space = i;
    while cs.get(i).is_some_and(|(_, c)| *c == ' ' || *c == '\t') {
        i += 1;
    }
    let numerator = digits(i);
    if i == space || numerator == 0 || !cs.get(i + numerator).is_some_and(|(_, c)| *c == '/') {
        return None;
    }
    let denominator = digits(i + numerator + 1);
    let end = i + numerator + 1 + denominator;
    let next = |k: usize| cs.get(end + k).map(|(_, c)| *c);
    let continues = matches!(next(0), Some('.'))
        || (!in_call && matches!(next(0), Some(',')) && next(1).is_some_and(|c| c.is_ascii_digit()))
        || (matches!(next(0), Some('e' | 'E')) && next(1).is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '+'));
    if denominator == 0 || continues {
        return None;
    }
    // Compared as digits, not as floats: long ones round to the same float (#1802 review).
    let digits_of = |from: usize, len: usize| -> Option<String> {
        let s: String = cs.get(from..from + len)?.iter().map(|(_, c)| *c).collect();
        Some(s.trim_start_matches('0').to_string())
    };
    let (n, d) = (digits_of(i, numerator)?, digits_of(i + numerator + 1, denominator)?);
    let proper = !d.is_empty() && (n.len(), n.as_str()) < (d.len(), d.as_str());
    let value = |t: &str| if t.is_empty() { Some(0.0) } else { t.parse::<f64>().ok() };
    if !proper {
        return None;
    }
    Some((value(&n)? / value(&d)?, end))
}

fn lex(s: &str) -> Result<Vec<Tok>, DocError> {
    Ok(lex_spans(s)?.into_iter().map(|(t, _, _)| t).collect())
}

/// Is the identifier token at `i` a function's name, called (followed by `(`)?
fn is_call(toks: &[Tok], i: usize) -> bool {
    matches!(toks.get(i + 1), Some(Tok::Op('(')))
}

/// Is the identifier token at `i` a reference to a parameter (not a call, unit or constant)?
fn is_reference(toks: &[Tok], i: usize, id: &str) -> bool {
    !is_call(toks, i) && unit(id).is_none() && !is_constant(id)
}

/// Identifiers an expression refers to (excluding units, constants and functions).
pub fn references(s: &str) -> Vec<String> {
    // Every evaluation fingerprints every feature by the parameters its expressions name, so the
    // same short expressions are asked about on every command: the answers are kept (per thread,
    // bounded) instead of being lexed again.
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::HashMap<String, Vec<String>>> = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    if let Some(hit) = SEEN.with(|seen| seen.borrow().get(s).cloned()) {
        return hit;
    }
    let out = scan_references(s);
    SEEN.with(|seen| {
        let mut seen = seen.borrow_mut();
        if seen.len() >= 1 << 16 {
            seen.clear();
        }
        seen.insert(s.to_string(), out.clone());
    });
    out
}

/// [`references`], worked out.
fn scan_references(s: &str) -> Vec<String> {
    let Ok(toks) = lex(s) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if let Tok::Id(id) = t
            && is_reference(&toks, i, id)
            && !out.contains(id)
        {
            out.push(id.clone());
        }
    }
    out
}

/// The expression with every reference to `old` renamed to `new` (text otherwise kept).
pub fn rename_reference(s: &str, old: &str, new: &str) -> String {
    rename_where(s, old, new, is_reference)
}

/// [`rename_reference`] as a build in which `old` was not a unit read the expression: every
/// `old` that isn't called is renamed (`2 g` was 2 × the parameter `g`). It renames an older
/// design's parameter named after what is now a unit.
pub fn rename_word(s: &str, old: &str, new: &str) -> String {
    rename_where(s, old, new, |toks, i, _| !is_call(toks, i))
}

/// `s` with each identifier `old` that `rename` (tokens, its index, `old`) picks replaced by `new`.
fn rename_where(s: &str, old: &str, new: &str, rename: impl Fn(&[Tok], usize, &str) -> bool) -> String {
    let Ok(spans) = lex_spans(s) else { return s.to_string() };
    let toks: Vec<Tok> = spans.iter().map(|(t, _, _)| t.clone()).collect();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (i, (t, a, b)) in spans.iter().enumerate() {
        if let Tok::Id(id) = t
            && id == old
            && rename(toks.as_slice(), i, id.as_str())
        {
            out.push_str(s.get(last..*a).unwrap_or_default());
            out.push_str(new);
            last = *b;
        }
    }
    out.push_str(s.get(last..).unwrap_or_default());
    out
}

/// Is the expression just a number (with an optional unit), like `20`, `20 mm` or `-1.5 in`?
pub fn is_literal(s: &str) -> bool {
    // A definition names its value: it is not just a number, whatever it defines (#4071).
    if definition(s).is_some() {
        return false;
    }
    let Ok(toks) = lex(s) else { return false };
    let toks: &[Tok] = match toks.first() {
        Some(Tok::Op('-' | '+')) => toks.get(1..).unwrap_or_default(),
        _ => &toks,
    };
    match toks {
        [Tok::Num(_)] => true,
        [Tok::Num(_), Tok::Id(u)] => unit(u).is_some(),
        _ => false,
    }
}

/// Is the expression a bare number without a unit, like `20` or `-1.5`? A leading `=` only marks
/// a formula (#4681), so `=20` is the same bare number.
pub fn is_bare_number(s: &str) -> bool {
    let number = s.trim().strip_prefix('=').unwrap_or(s);
    is_literal(s) && number.trim().trim_start_matches(['-', '+']).trim().parse::<f64>().is_ok()
}

/// Is `s` shaped like a parameter name: a letter or `_`, then letters, digits and `_`? The shape
/// only; whether the name is free or reserved is `Document::param_name_problem`'s business.
pub(crate) fn is_name_shaped(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c.is_alphabetic() || c == '_') && cs.all(|c| c.is_alphanumeric() || c == '_')
}

/// A value typed as a definition, `name = expression` (or `#name = expression`, #2371): the name
/// without the `#` and the text after the first `=`, both trimmed (the text may be empty). `None` for anything else, including text whose
/// left side is not shaped like a name (`2 = 3`), which stays an ordinary expression and fails
/// to parse as before. Every value input defines a user parameter this way (#2916, #4071): until
/// `Document::define_inline` stores the parameter, the text evaluates as its expression.
pub fn definition(s: &str) -> Option<(&str, &str)> {
    let (name, rhs) = s.split_once('=')?;
    let name = name.trim();
    let rhs = rhs.trim();
    let name = name.strip_prefix('#').unwrap_or(name);
    // `w == 3` is not a definition: a second `=` would read as a formula marker (#4681).
    (is_name_shaped(name) && !rhs.starts_with('=')).then_some((name, rhs))
}

/// The length unit of a design with these units: `mm` unless they name another length unit.
pub fn length_unit_or_mm(units: &str) -> &str {
    let u = units.trim();
    if unit_info(u).is_some_and(|(k, _)| k == Kind::Length) { u } else { "mm" }
}

struct Parser<'a> {
    toks: Vec<Tok>,
    /// Each token's byte range in the source: whether a fraction is written closed up.
    spans: Vec<(usize, usize)>,
    pos: usize,
    lookup: &'a dyn Fn(&str) -> Result<Value, DocError>,
    depth: usize,
    defaults: Defaults,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn enter(&mut self) -> Result<(), DocError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(DocError::Expr("expression nested too deeply".into())) } else { Ok(()) }
    }
    fn expr(&mut self) -> Result<Value, DocError> {
        self.enter()?;
        let mut a = self.term()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let b = self.term()?;
            a = add(a, b, c == '-', self.defaults)?;
        }
        self.depth -= 1;
        Ok(a)
    }
    fn term(&mut self) -> Result<Value, DocError> {
        let mut a = self.unary()?;
        loop {
            match self.peek().cloned() {
                Some(Tok::Op('*')) => {
                    self.pos += 1;
                    let b = self.unary()?;
                    a = mul(a, b, false)?;
                }
                Some(Tok::Op('/')) => {
                    self.pos += 1;
                    let b = self.unary()?;
                    a = mul(a, b, true)?;
                }
                Some(Tok::Op('%')) => {
                    self.pos += 1;
                    let b = self.unary()?;
                    a = modulo(a, b, self.defaults)?;
                }
                // Juxtaposition: `10 mm`, `2 width`.
                Some(Tok::Id(_) | Tok::Num(_)) | Some(Tok::Op('(')) => {
                    let b = self.unary()?;
                    a = mul(a, b, false)?;
                }
                _ => return Ok(a),
            }
        }
    }
    fn unary(&mut self) -> Result<Value, DocError> {
        self.signed(true)
    }
    /// A signed operand. `glue`: a number takes the unit written after it (not in an exponent).
    fn signed(&mut self, glue: bool) -> Result<Value, DocError> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                self.enter()?;
                let v = self.signed(glue)?;
                self.depth -= 1;
                Ok(Value { v: -v.v, ..v })
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.enter()?;
                let v = self.signed(glue)?;
                self.depth -= 1;
                Ok(v)
            }
            _ => self.power(glue),
        }
    }
    fn power(&mut self, glue: bool) -> Result<Value, DocError> {
        let number = matches!(self.peek(), Some(Tok::Num(_)));
        let at = self.pos;
        let mut base = self.atom()?;
        // A number and the unit after it are one quantity, bound tighter than `*` and `/`:
        // `10 mm / 2 mm` is 5, not (10 mm / 2) mm = 5 mm². The unit keeps its own power
        // (`3 mm^2`). A name that is not a unit (`2 width`) stays an ordinary product.
        if glue && number {
            // A fraction of two numbers before a unit is one value, as sizes are written:
            // `1/2"` and `1/2 in` are half an inch (as the mixed number `2 1/8"` is one value).
            // After a division only written closed up: `1 in / 1/2 in` is 2, but
            // `width / 10 / 2 mm` is (width / 10) / 2 mm.
            if let (Some(Tok::Op('/')), Some(Tok::Num(d))) = (self.peek(), self.toks.get(self.pos + 1))
                && self.unit_at(self.pos + 2)
                && (!self.after_division(at) || self.closed_up(at, self.pos + 1))
            {
                let d = Value::num(*d);
                self.pos += 2;
                base = mul(base, d, true)?;
            }
            if self.unit_at(self.pos) {
                let u = self.power(false)?;
                return mul(base, u, false);
            }
        }
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            self.enter()?;
            // The exponent is a plain number: `2^3 mm` stays 8 mm.
            let e = self.signed(false)?;
            self.depth -= 1;
            return pow(base, e);
        }
        Ok(base)
    }
    /// Whether the operand starting at token `i` is divided by: a `/` before it, past any signs.
    fn after_division(&self, i: usize) -> bool {
        let mut k = i;
        while k > 0 && matches!(self.toks.get(k - 1), Some(Tok::Op('+' | '-'))) {
            k -= 1;
        }
        k > 0 && matches!(self.toks.get(k - 1), Some(Tok::Op('/')))
    }
    /// Whether tokens `a` to `b` are written without spaces between them.
    fn closed_up(&self, a: usize, b: usize) -> bool {
        (a..b).all(|i| matches!((self.spans.get(i), self.spans.get(i + 1)), (Some(x), Some(y)) if x.1 == y.0))
    }
    /// Whether the token at `i` is a unit name, not called as a function.
    fn unit_at(&self, i: usize) -> bool {
        matches!(self.toks.get(i), Some(Tok::Id(id)) if unit(id).is_some()) && !matches!(self.toks.get(i + 1), Some(Tok::Op('(')))
    }
    fn atom(&mut self) -> Result<Value, DocError> {
        match self.next() {
            Some(Tok::Num(v)) => Ok(Value::num(v)),
            Some(Tok::Op('(')) => {
                let v = self.expr()?;
                match self.next() {
                    Some(Tok::Op(')')) => Ok(v),
                    _ => Err(DocError::Expr("missing `)`".into())),
                }
            }
            Some(Tok::Id(id)) => {
                if let Some(Tok::Op('(')) = self.peek() {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::Op(')'))) {
                        loop {
                            if args.len() >= MAX_ARGS {
                                return Err(DocError::Expr(format!("too many arguments to `{id}`")));
                            }
                            args.push(self.expr()?);
                            match self.next() {
                                Some(Tok::Op(',')) => continue,
                                Some(Tok::Op(')')) => break,
                                _ => return Err(DocError::Expr(format!("missing `)` after the arguments of `{id}`"))),
                            }
                        }
                    } else {
                        self.pos += 1;
                    }
                    return call(&id, &args, self.defaults);
                }
                match id.as_str() {
                    "pi" | "PI" => return Ok(Value::num(std::f64::consts::PI)),
                    _ => {}
                }
                if let Some(u) = unit(&id) {
                    return Ok(u);
                }
                (self.lookup)(&id)
            }
            Some(Tok::Op(c)) => Err(DocError::Expr(format!("unexpected `{c}`"))),
            None => Err(DocError::Expr("unexpected end of expression".into())),
        }
    }
}

/// A unit-less number next to a value with these powers, in that value's default unit.
fn scale_bare(x: Value, dims: Dims, d: Defaults) -> f64 {
    match Kind::of_dims(dims) {
        Some(k) if x.unitless() => x.v * d.scale(k),
        _ => x.v,
    }
}

fn add(a: Value, b: Value, sub: bool, d: Defaults) -> Result<Value, DocError> {
    let dims = if a.unitless() {
        b.dims()
    } else if b.unitless() || a.same_dims(&b) {
        a.dims()
    } else {
        return Err(DocError::Expr(format!("cannot {} {} and {}", if sub { "subtract" } else { "add" }, a.describe(), b.describe())));
    };
    let (x, y) = (scale_bare(a, dims, d), scale_bare(b, dims, d));
    Ok(Value::with_dims(if sub { x - y } else { x + y }, dims))
}

/// Unit powers combined power by power with `f`, each within ±16 (checked as a range: `abs` of
/// -128, which 16 × -8 makes, overflows).
fn combine(a: Dims, b: Dims, f: impl Fn(i8, i8) -> Option<i8>) -> Result<Dims, DocError> {
    let mut out = [0; 3];
    for (o, (x, y)) in out.iter_mut().zip(a.into_iter().zip(b)) {
        *o = f(x, y).filter(|p| (-16..=16).contains(p)).ok_or_else(|| DocError::Expr("unit powers too large".into()))?;
    }
    Ok(out)
}

fn mul(a: Value, b: Value, div: bool) -> Result<Value, DocError> {
    if div && b.v == 0.0 {
        return Err(DocError::Expr("division by zero".into()));
    }
    let dims = combine(a.dims(), b.dims(), |x, y| if div { x.checked_sub(y) } else { x.checked_add(y) })?;
    Ok(Value::with_dims(if div { a.v / b.v } else { a.v * b.v }, dims))
}

fn modulo(a: Value, b: Value, d: Defaults) -> Result<Value, DocError> {
    let dims = if a.unitless() { b.dims() } else { a.dims() };
    if !(a.unitless() || b.unitless() || a.same_dims(&b)) {
        return Err(DocError::Expr(format!("cannot take {} modulo {}", a.describe(), b.describe())));
    }
    let (x, y) = (scale_bare(a, dims, d), scale_bare(b, dims, d));
    if y == 0.0 {
        return Err(DocError::Expr("modulo by zero".into()));
    }
    Ok(Value::with_dims(x.rem_euclid(y), dims))
}

fn pow(base: Value, e: Value) -> Result<Value, DocError> {
    if !e.unitless() {
        return Err(DocError::Expr(format!("an exponent must be a unit-less number, not {}", e.describe())));
    }
    let n = e.v;
    let k = if (n - n.round()).abs() < 1e-12 && n.abs() < 16.0 { n.round() as i8 } else { 0 };
    if !base.unitless() && k == 0 {
        return Err(DocError::Expr("a value with units needs a small whole-number exponent".into()));
    }
    Ok(Value::with_dims(base.v.powf(n), combine(base.dims(), [k; 3], i8::checked_mul)?))
}

fn call(f: &str, args: &[Value], d: Defaults) -> Result<Value, DocError> {
    let one = || match args {
        [a] => Ok(*a),
        _ => Err(DocError::Expr(format!("`{f}` takes one argument"))),
    };
    let two = || match args {
        [a, b] => Ok((*a, *b)),
        _ => Err(DocError::Expr(format!("`{f}` takes two arguments"))),
    };
    let rad = |a: Value| match a.kind() {
        Some(Kind::Angle) => Ok(a.v),
        Some(Kind::Unitless) => Ok(a.v * d.ang),
        _ => Err(DocError::Expr(format!("`{f}` needs an angle, not {}", a.describe()))),
    };
    let num = |a: Value| if a.unitless() { Ok(a.v) } else { Err(DocError::Expr(format!("`{f}` needs a unit-less number, not {}", a.describe()))) };
    let same = |a: Value, b: Value| -> Result<(Value, Value), DocError> {
        // Bring a bare number to the other operand's unit (as in `a + b`).
        let z = add(a, Value { v: 0.0, ..b }, false, d)?;
        let w = add(b, Value { v: 0.0, ..a }, false, d)?;
        Ok((z, w))
    };
    let unit_fn = |x: f64, a: Value| Value { v: x, ..a };
    Ok(match f {
        "sin" => Value::num(rad(one()?)?.sin()),
        "cos" => Value::num(rad(one()?)?.cos()),
        "tan" => Value::num(rad(one()?)?.tan()),
        "asin" | "acos" => {
            let x = num(one()?)?;
            if !(-1.0..=1.0).contains(&x) {
                return Err(DocError::Expr(format!("`{f}` needs a number between -1 and 1")));
            }
            Value::angle(if f == "asin" { x.asin() } else { x.acos() })
        }
        "atan" => Value::angle(num(one()?)?.atan()),
        "atan2" => {
            let (y, x) = two()?;
            let (y, x) = same(y, x)?;
            if !y.same_dims(&x) {
                return Err(DocError::Expr("`atan2` needs two values with the same units".into()));
            }
            Value::angle(y.v.atan2(x.v))
        }
        "sqrt" => {
            let a = one()?;
            if a.dims().iter().any(|p| p % 2 != 0) {
                return Err(DocError::Expr("sqrt of an odd power of a unit".into()));
            }
            if a.v < 0.0 {
                return Err(DocError::Expr("sqrt of a negative number".into()));
            }
            Value::with_dims(a.v.sqrt(), a.dims().map(|p| p / 2))
        }
        "abs" => {
            let a = one()?;
            unit_fn(a.v.abs(), a)
        }
        "sign" => Value::num(one()?.v.signum()),
        "floor" | "ceil" | "round" | "trunc" => {
            let a = one()?;
            // Round in the value's own default unit (round(2.4 in) = 2 in for an inch parameter).
            let scale = scale_bare(Value::num(1.0), a.dims(), d);
            let x = if scale != 0.0 { a.v / scale } else { a.v };
            let r = match f {
                "floor" => x.floor(),
                "ceil" => x.ceil(),
                "trunc" => x.trunc(),
                _ => x.round(),
            };
            unit_fn(r * if scale != 0.0 { scale } else { 1.0 }, a)
        }
        "exp" => Value::num(num(one()?)?.exp()),
        "sinh" => Value::num(num(one()?)?.sinh()),
        "cosh" => Value::num(num(one()?)?.cosh()),
        "tanh" => Value::num(num(one()?)?.tanh()),
        "asinh" => Value::num(num(one()?)?.asinh()),
        "acosh" => {
            let x = num(one()?)?;
            if !(x >= 1.0) {
                return Err(DocError::Expr("`acosh` needs a number of at least 1".into()));
            }
            Value::num(x.acosh())
        }
        "atanh" => {
            let x = num(one()?)?;
            if !(x > -1.0 && x < 1.0) {
                return Err(DocError::Expr("`atanh` needs a number between -1 and 1, both excluded".into()));
            }
            Value::num(x.atanh())
        }
        "ln" | "log" | "log10" => {
            let x = num(one()?)?;
            if x <= 0.0 {
                return Err(DocError::Expr(format!("`{f}` needs a positive number")));
            }
            Value::num(if f == "ln" { x.ln() } else { x.log10() })
        }
        "hypot" => {
            let (a, b) = two()?;
            let (a, b) = same(a, b)?;
            if !a.same_dims(&b) {
                return Err(DocError::Expr("`hypot` needs two values with the same units".into()));
            }
            unit_fn(a.v.hypot(b.v), a)
        }
        "pow" => {
            let (a, b) = two()?;
            pow(a, b)?
        }
        "mod" => {
            let (a, b) = two()?;
            modulo(a, b, d)?
        }
        "min" | "max" => {
            let Some(first) = args.first().copied() else { return Err(DocError::Expr(format!("`{f}` needs arguments"))) };
            let mut acc = first;
            for a in args.iter().skip(1) {
                let (x, y) = same(*a, acc)?;
                if !x.same_dims(&y) {
                    return Err(DocError::Expr(format!("`{f}` needs values with the same units")));
                }
                acc = if (f == "min" && x.v < y.v) || (f == "max" && x.v > y.v) { x } else { y };
            }
            acc
        }
        _ => return Err(DocError::Expr(format!("unknown function `{f}`"))),
    })
}

/// Evaluate a count: a unit-less whole number. A value with a unit says how to fix it, and a
/// fraction is refused rather than rounded to a different number of copies.
pub fn eval_count(s: &str, vals: &BTreeMap<String, Value>) -> Result<f64, DocError> {
    let e = s.trim();
    let value = eval_with(e, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))))?;
    if let Some(kind) = value.kind().filter(|kind| *kind != Kind::Unitless) {
        let subject = if vals.contains_key(e) { format!("`{e}`") } else { "this expression".into() };
        let remedy = if vals.contains_key(e) { "set its Unit to none to use it as a count" } else { "use a unit-less expression as a count" };
        return Err(DocError::Expr(format!("{subject} is {} ({}); {remedy}", value.describe(), kind.default_unit())));
    }
    let n = value.to_kind(Kind::Unitless)?;
    // Allow float noise such as `10 / 3 * 3`, never a real fraction.
    if !n.is_finite() || (n - n.round()).abs() > 1e-9 {
        return Err(DocError::Expr(format!("a count must be a whole number; `{e}` is {n}")));
    }
    Ok(n.round())
}

/// Evaluate an expression; `lookup` resolves parameter names. Bare numbers are in the default
/// units (mm, degrees, grams…).
pub fn eval_with(s: &str, lookup: &dyn Fn(&str) -> Result<Value, DocError>) -> Result<Value, DocError> {
    eval_with_defaults(s, lookup, Defaults::default())
}

/// Evaluate an expression reading bare numbers next to lengths / angles in `defaults`.
pub fn eval_with_defaults(s: &str, lookup: &dyn Fn(&str) -> Result<Value, DocError>, defaults: Defaults) -> Result<Value, DocError> {
    let (toks, spans): (Vec<Tok>, Vec<(usize, usize)>) = lex_spans(s)?.into_iter().map(|(t, a, b)| (t, (a, b))).unzip();
    if toks.is_empty() {
        if let Some((name, _)) = definition(s) {
            return Err(DocError::Expr(format!("`{name} =` needs a value after the `=`")));
        }
        return Err(DocError::Expr("empty expression".into()));
    }
    let mut p = Parser { toks, spans, pos: 0, lookup, depth: 0, defaults };
    let v = p.expr()?;
    if p.pos < p.toks.len() {
        return Err(DocError::Expr(format!("unexpected input in `{s}`")));
    }
    if !v.v.is_finite() {
        return Err(DocError::Expr(format!("`{s}` is not a finite number")));
    }
    Ok(v)
}

/// One parameter to evaluate: name, expression and unit (`mm`, `in`, `deg`, empty…).
#[derive(Clone, Debug, PartialEq)]
pub struct ParamDef {
    pub name: String,
    pub expr: String,
    pub unit: String,
}

/// Evaluate all parameters in dependency order: values (mm, rad, g, unit-less…) and errors by
/// name. Circular references are reported with their path (`a → b → a`). `doc_len` is the
/// document's length unit in mm (bare numbers of unit-less-unit parameters next to lengths).
pub fn eval_params(params: &[ParamDef], doc_len: f64) -> (BTreeMap<String, Value>, BTreeMap<String, String>) {
    // Each command evaluates the design's parameters (every named feature input is one) several
    // times over, mostly unchanged: the last few results are kept (per thread) for the same
    // definitions, compared in full.
    type Evaluated = (BTreeMap<String, Value>, BTreeMap<String, String>);
    thread_local! {
        static LAST: std::cell::RefCell<Vec<(Vec<ParamDef>, u64, Evaluated)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let key = doc_len.to_bits();
    let hit = LAST.with(|last| {
        let mut last = last.borrow_mut();
        let at = last.iter().position(|(defs, len, _)| *len == key && defs.as_slice() == params)?;
        let entry = last.remove(at);
        let out = entry.2.clone();
        last.push(entry);
        Some(out)
    });
    if let Some(out) = hit {
        return out;
    }
    let out = eval_params_uncached(params, doc_len);
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if last.len() >= 4 {
            last.remove(0);
        }
        last.push((params.to_vec(), key, out.clone()));
    });
    out
}

/// [`eval_params`], worked out.
fn eval_params_uncached(params: &[ParamDef], doc_len: f64) -> (BTreeMap<String, Value>, BTreeMap<String, String>) {
    let mut done: BTreeMap<String, Value> = BTreeMap::new();
    let mut errors: BTreeMap<String, String> = BTreeMap::new();
    let index: BTreeMap<&str, &ParamDef> = params.iter().map(|p| (p.name.as_str(), p)).collect();
    struct Ctx<'a> {
        index: BTreeMap<&'a str, &'a ParamDef>,
        done: BTreeMap<String, Value>,
        errors: BTreeMap<String, String>,
        stack: Vec<String>,
        doc_len: f64,
    }
    fn visit(name: &str, cx: &mut Ctx) -> Result<Value, DocError> {
        if let Some(v) = cx.done.get(name) {
            return Ok(*v);
        }
        if let Some(e) = cx.errors.get(name) {
            return Err(DocError::Expr(format!("`{name}` has an error: {e}")));
        }
        let Some(p) = cx.index.get(name).copied() else {
            return Err(DocError::Expr(format!("unknown parameter `{name}`")));
        };
        if let Some(at) = cx.stack.iter().position(|s| s == name) {
            let mut path: Vec<&str> = cx.stack.iter().skip(at).map(String::as_str).collect();
            path.push(name);
            return Err(DocError::Expr(format!("circular reference: {}", path.join(" → "))));
        }
        if cx.stack.len() > 512 {
            return Err(DocError::Expr("parameters nested too deeply".into()));
        }
        cx.stack.push(name.to_string());
        let mut vals: BTreeMap<String, Value> = BTreeMap::new();
        let mut err = None;
        for d in references(&p.expr) {
            match visit(&d, cx) {
                Ok(v) => {
                    vals.insert(d, v);
                }
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        cx.stack.pop();
        let r = match err {
            Some(e) => Err(e),
            // Most parameters are named feature inputs with a literal value ("10 mm"): those are
            // worked out once (per thread) rather than parsed again on every evaluation.
            None if vals.is_empty() => eval_constant(p, cx.doc_len),
            None => eval_def(p, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))), cx.doc_len),
        };
        match &r {
            Ok(v) => {
                cx.done.insert(name.to_string(), *v);
            }
            Err(e) => {
                let msg = e.to_string();
                cx.errors.insert(name.to_string(), msg.strip_prefix("expression: ").unwrap_or(&msg).to_string());
            }
        }
        r
    }
    let mut cx = Ctx { index, done: std::mem::take(&mut done), errors: std::mem::take(&mut errors), stack: Vec::new(), doc_len };
    for p in params {
        cx.stack.clear();
        let _ = visit(&p.name, &mut cx);
    }
    (cx.done, cx.errors)
}

/// One parameter's value: its expression in its unit, with the values of the parameters it
/// refers to from `lookup`.
pub fn eval_def(p: &ParamDef, lookup: &dyn Fn(&str) -> Result<Value, DocError>, doc_len: f64) -> Result<Value, DocError> {
    let (kind, _) = unit_info(&p.unit).unwrap_or((Kind::Length, 1.0));
    let defaults = Defaults::for_unit(&p.unit, doc_len);
    let x = eval_with_defaults(&p.expr, lookup, defaults)?.to_kind_in(kind, defaults)?;
    Ok(Value::of_kind(kind, x))
}

/// [`eval_def`] of a parameter whose expression names no other parameter, remembered by its
/// expression, unit and the design's length unit.
fn eval_constant(p: &ParamDef, doc_len: f64) -> Result<Value, DocError> {
    type Key = (String, String, u64);
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::HashMap<Key, Result<Value, DocError>>> = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let key = (p.expr.clone(), p.unit.clone(), doc_len.to_bits());
    if let Some(hit) = SEEN.with(|seen| seen.borrow().get(&key).cloned()) {
        return hit;
    }
    let out = eval_def(p, &|n| Err(DocError::Expr(format!("unknown parameter `{n}`"))), doc_len);
    SEEN.with(|seen| {
        let mut seen = seen.borrow_mut();
        if seen.len() >= 1 << 16 {
            seen.clear();
        }
        seen.insert(key, out.clone());
    });
    out
}

/// Dependency edges between parameters: (parameter, parameter it references).
pub fn param_edges(params: &[ParamDef]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in params {
        for r in references(&p.expr) {
            out.push((p.name.clone(), r));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str, k: Kind) -> Result<f64, DocError> {
        let look = |n: &str| match n {
            "width" => Ok(Value::length(40.0)),
            "ang" => Ok(Value::angle(std::f64::consts::FRAC_PI_2)),
            "count" => Ok(Value::num(3.0)),
            _ => Err(DocError::Expr(format!("unknown {n}"))),
        };
        eval_with(s, &look)?.to_kind(k)
    }

    fn def(n: &str, e: &str, u: &str) -> ParamDef {
        ParamDef { name: n.into(), expr: e.into(), unit: u.into() }
    }

    /// `Kind::ALL` lists every kind once: a new kind fails to build in this match, which counts
    /// them, so it can't be left out of `ALL` (where `of_dims` would never find it).
    #[test]
    fn every_kind_is_listed_once() {
        let kinds = |k: Kind| match k {
            Kind::Length | Kind::Angle | Kind::Unitless | Kind::Mass | Kind::Volume | Kind::Area => 6,
        };
        assert_eq!(Kind::ALL.len(), kinds(Kind::Length));
        for k in Kind::ALL {
            assert_eq!(Kind::ALL.iter().filter(|x| **x == k).count(), 1, "{k:?}");
            assert_eq!(Kind::of_dims(k.dims()), Some(k), "{k:?}");
        }
    }

    #[test]
    fn a_decimal_comma_outside_parentheses() {
        // #1347: a comma between digits is a decimal comma where it cannot separate arguments.
        assert_eq!(ev("2,5", Kind::Unitless).unwrap(), 2.5);
        assert_eq!(ev("2,5 mm", Kind::Length).unwrap(), 2.5);
        assert_eq!(ev("12,75mm + 0,25", Kind::Length).unwrap(), 13.0);
        assert_eq!(ev("width / 2,5", Kind::Length).unwrap(), 16.0);
        assert!((ev("1,5e3 mm", Kind::Length).unwrap() - 1500.0).abs() < 1e-9);
        assert!((ev("22,5 deg", Kind::Angle).unwrap() - 22.5f64.to_radians()).abs() < 1e-12);
        // Inside parentheses a comma still separates a function's arguments.
        assert_eq!(ev("max(1,2)", Kind::Unitless).unwrap(), 2.0);
        assert_eq!(ev("max(1 mm, 3, 2)", Kind::Length).unwrap(), 3.0);
        // Plain parentheses have no arguments to separate (#9408), also inside a call.
        assert_eq!(ev("(2,5)", Kind::Unitless).unwrap(), 2.5);
        assert_eq!(ev("-(width-0,5)/2", Kind::Length).unwrap(), -19.75);
        assert_eq!(ev("(,5)", Kind::Unitless).unwrap(), 0.5);
        assert_eq!(ev("(2 1/8,5)", Kind::Unitless).unwrap(), 2.0 / 8.5);
        assert_eq!(ev("max((1,5), 1)", Kind::Unitless).unwrap(), 1.5);
        assert_eq!(ev("(max(1,2))", Kind::Unitless).unwrap(), 2.0);
        // Not between two digits, or a second comma: still an error.
        for bad in ["2,", "2 ,5", "2, 5", "1,5,3", "1.5,3", ",5,3", ",5.3", "width ,5", "(1) ,5"] {
            assert!(ev(bad, Kind::Unitless).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_leading_decimal_comma_is_a_number() {
        // #2966: `,5` is 0,5 where `.5` would be a number: at the start or after an operator.
        assert_eq!(ev(",5", Kind::Unitless).unwrap(), 0.5);
        assert_eq!(ev(",5 mm", Kind::Length).unwrap(), 0.5);
        assert_eq!(ev("-,5", Kind::Unitless).unwrap(), -0.5);
        assert_eq!(ev("2*,5", Kind::Unitless).unwrap(), 1.0);
        assert_eq!(ev("width - ,25", Kind::Length).unwrap(), 39.75);
        // Function arguments are still separated by commas.
        assert_eq!(ev("max(1,5)", Kind::Unitless).unwrap(), 5.0);
        assert_eq!(ev("max(,5; 2)", Kind::Unitless).ok(), None, "inside parentheses a comma is a separator");
    }

    #[test]
    fn inch_and_foot_marks() {
        // #1502: `1"` in a millimetre design is an inch, `2'` two feet, straight or as primes.
        assert_eq!(ev("1\"", Kind::Length).unwrap(), 25.4);
        assert_eq!(ev("1″", Kind::Length).unwrap(), 25.4);
        assert_eq!(ev("2'", Kind::Length).unwrap(), 609.6);
        assert_eq!(ev("2′", Kind::Length).unwrap(), 609.6);
        assert_eq!(ev("1/2\"", Kind::Length).unwrap(), 12.7);
        assert_eq!(ev("width + 1\"", Kind::Length).unwrap(), 65.4);
        assert_eq!(ev("2' + 3\"", Kind::Length).unwrap(), 685.8);
        // A mark alone is its unit alone, as `in` is.
        assert_eq!(ev("\"", Kind::Length).ok(), ev("in", Kind::Length).ok());
    }

    #[test]
    fn mixed_numbers() {
        // #1802: `2 1/8"` is 2.125 in, as imperial sizes are written, not 2 × 1/8 in.
        assert_eq!(ev("2 1/8\"", Kind::Length).unwrap(), 2.125 * 25.4);
        assert_eq!(ev("2 1/8 in", Kind::Length).unwrap(), 2.125 * 25.4);
        assert_eq!(ev("3 7/16\"", Kind::Length).unwrap(), 3.4375 * 25.4);
        assert_eq!(ev("10 1/2mm", Kind::Length).unwrap(), 10.5);
        assert_eq!(ev("width + 1 1/2", Kind::Length).unwrap(), 41.5);
        assert_eq!(ev("max(2 1/8, 3)", Kind::Unitless).unwrap(), 3.0);
        // Inside parentheses a comma after the fraction separates arguments; outside, a comma
        // between digits is a decimal comma, which continues the number (as before).
        assert_eq!(ev("max(2 1/8,1)", Kind::Unitless).unwrap(), 2.125);
        assert_eq!(ev("2 1/8,5", Kind::Unitless).unwrap(), 2.0 / 8.5);
        // Long numerators and denominators are compared as digits, not as rounded floats.
        assert!((ev("2 9007199254740992/9007199254740993", Kind::Unitless).unwrap() - 3.0).abs() < 1e-12);
        assert_eq!(ev("2 007/8", Kind::Unitless).unwrap(), 2.875);
        assert_eq!(ev("-2 1/4", Kind::Unitless).unwrap(), -2.25);
        // Not a proper fraction of whole numbers after a whole number: as before.
        for (s, v) in [("2 3/2", 3.0), ("2.5 1/2", 1.25), ("2 1/8.5", 2.0 / 8.5), ("2 1 / 8", 0.25), ("2 1/0", f64::INFINITY), ("1/8", 0.125)] {
            assert_eq!(ev(s, Kind::Unitless).ok(), Some(v).filter(|v| v.is_finite()), "{s}");
        }
    }

    #[test]
    fn a_number_and_its_unit_are_one_quantity() {
        // `10 mm / 2 mm` divides by 2 mm; it is not `(10 mm / 2) mm`, a value in mm².
        assert_eq!(ev("10 mm / 2 mm", Kind::Unitless).unwrap(), 5.0);
        assert_eq!(ev("10mm/2mm", Kind::Unitless).unwrap(), 5.0);
        assert_eq!(ev("width / 4 mm", Kind::Unitless).unwrap(), 10.0);
        assert_eq!(ev("6 mm / 2 mm * 3", Kind::Unitless).unwrap(), 9.0);
        assert!((ev("90 deg / 1 rad", Kind::Unitless).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((ev("(ang - 30 deg) / 1 rad * width", Kind::Length).unwrap() - 40.0 * 60f64.to_radians()).abs() < 1e-12);
        assert_eq!(ev("1 in / 1 mm", Kind::Unitless).unwrap(), 25.4);
        assert_eq!(ev("2 1/2 in / 1 in", Kind::Unitless).unwrap(), 2.5);
        assert_eq!(ev("1\" / 1 mm", Kind::Unitless).unwrap(), 25.4);
        assert_eq!(ev("-10 mm / 2 mm", Kind::Unitless).unwrap(), -5.0);
        // A fraction of numbers right before a unit stays one value: half an inch.
        assert_eq!(ev("1/2 in", Kind::Length).unwrap(), 12.7);
        assert_eq!(ev("width + 3/4\"", Kind::Length).unwrap(), 40.0 + 0.75 * 25.4);
        assert_eq!(ev("1 in / 1/2 in", Kind::Unitless).unwrap(), 2.0);
        assert_eq!(ev("10 / 2 mm", Kind::Length).unwrap(), 5.0);
        // After a division, a spaced-out `/` divides again: (width / 10) / 2 mm.
        assert_eq!(ev("width / 10 / 2 mm", Kind::Unitless).unwrap(), 2.0);
        assert_eq!(ev("width / -10 / 2 mm", Kind::Unitless).unwrap(), -2.0);
        assert_eq!(ev("width / 10/2 mm", Kind::Unitless).unwrap(), 8.0);
        // The unit keeps its own power, and an exponent stays a plain number.
        assert_eq!(ev("sqrt(9 mm^2)", Kind::Length).unwrap(), 3.0);
        assert_eq!(ev("8 mm^3 / 2 mm^2", Kind::Length).unwrap(), 4.0);
        assert_eq!(ev("2^3 mm", Kind::Length).unwrap(), 8.0);
        // A number next to a name that is not a unit is a product, as before.
        assert_eq!(ev("2 width", Kind::Length).unwrap(), 80.0);
        // Unchanged: a unit alone, and a unit after a parenthesis.
        assert_eq!(ev("width / mm", Kind::Unitless).unwrap(), 40.0);
        assert_eq!(ev("(10 / 2) mm", Kind::Length).unwrap(), 5.0);
    }

    #[test]
    fn arithmetic_and_units() {
        assert_eq!(ev("10", Kind::Length).unwrap(), 10.0);
        assert!((ev("90 deg", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(ev("10 mm", Kind::Length).unwrap(), 10.0);
        assert_eq!(ev("1 in", Kind::Length).unwrap(), 25.4);
        assert_eq!(ev("2 cm + 5", Kind::Length).unwrap(), 25.0);
        assert_eq!(ev("width / 2 + 1", Kind::Length).unwrap(), 21.0);
        assert_eq!(ev("width * count", Kind::Length).unwrap(), 120.0);
        assert_eq!(ev("-(2+3)*2^2", Kind::Unitless).unwrap(), -20.0);
        assert!((ev("45", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((ev("ang / 2", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((ev("ang + 90", Kind::Angle).unwrap() - std::f64::consts::PI).abs() < 1e-12);
        assert!((ev("sqrt((3 mm) ^ 2 + (4 mm) ^ 2)", Kind::Length).unwrap() - 5.0).abs() < 1e-12);
        assert!((ev("sin(30)", Kind::Unitless).unwrap() - 0.5).abs() < 1e-12);
        assert!((ev("2*pi", Kind::Unitless).unwrap() - std::f64::consts::TAU).abs() < 1e-12);
        assert_eq!(ev("max(1 mm, 3, 2)", Kind::Length).unwrap(), 3.0);
        assert_eq!(ev("min(1 in, 30 mm)", Kind::Length).unwrap(), 25.4);
        assert_eq!(ev("1e-3 m", Kind::Length).unwrap(), 1.0);
        assert_eq!(ev("1 ft - 1 in", Kind::Length).unwrap(), 304.8 - 25.4);
        assert!((ev("atan2(1 mm, 1 mm)", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((ev("atan2(1; 1)", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((ev("exp(ln(5))", Kind::Unitless).unwrap() - 5.0).abs() < 1e-12);
        assert!((ev("log(1000)", Kind::Unitless).unwrap() - 3.0).abs() < 1e-12);
        // Hyperbolic functions (#9332): their definitions in exp, identities and inverses.
        assert!((ev("cosh(1.5) - (exp(1.5) + exp(-1.5)) / 2", Kind::Unitless).unwrap()).abs() < 1e-12);
        assert!((ev("sinh(1.5) - (exp(1.5) - exp(-1.5)) / 2", Kind::Unitless).unwrap()).abs() < 1e-12);
        assert!((ev("tanh(0.7) - sinh(0.7) / cosh(0.7)", Kind::Unitless).unwrap()).abs() < 1e-12);
        assert!((ev("cosh(2) ^ 2 - sinh(2) ^ 2", Kind::Unitless).unwrap() - 1.0).abs() < 1e-12);
        assert!((ev("asinh(sinh(1.25))", Kind::Unitless).unwrap() - 1.25).abs() < 1e-12);
        assert!((ev("acosh(cosh(1.25))", Kind::Unitless).unwrap() - 1.25).abs() < 1e-12);
        assert!((ev("atanh(tanh(0.5))", Kind::Unitless).unwrap() - 0.5).abs() < 1e-12);
        assert_eq!(ev("cosh(0) + sinh(0) + tanh(0) + asinh(0) + acosh(1) + atanh(0)", Kind::Unitless).unwrap(), 1.0);
        assert_eq!(ev("floor(2.7) + ceil(0.2) + round(1.5) + abs(-1)", Kind::Unitless).unwrap(), 6.0);
        assert!((ev("1 rad", Kind::Angle).unwrap() - 1.0).abs() < 1e-12);
        assert!((ev("acos(0)", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(ev("2 × 3 ÷ 4 − 1", Kind::Unitless).unwrap(), 0.5);
        assert_eq!(ev("hypot(3 mm, 4 mm)", Kind::Length).unwrap(), 5.0);
        assert_eq!(ev("7 % 4", Kind::Unitless).unwrap(), 3.0);
        assert_eq!(ev("width^2 / width", Kind::Length).unwrap(), 40.0);
    }

    /// The hyperbolic functions (#9332) take a unit-less number, and the inverses refuse what lies
    /// outside their domain, as `asin` and `ln` do.
    #[test]
    fn hyperbolic_functions_refuse_units_and_numbers_outside_their_domain() {
        for (e, says) in [
            ("cosh(2 mm)", "`cosh` needs a unit-less number"),
            ("tanh(30 deg)", "`tanh` needs a unit-less number"),
            ("acosh(0.5)", "`acosh` needs a number of at least 1"),
            ("atanh(1)", "`atanh` needs a number between -1 and 1"),
            ("atanh(-1.5)", "`atanh` needs a number between -1 and 1"),
        ] {
            let err = ev(e, Kind::Unitless).map(|v| v.to_string()).unwrap_or_else(|e| e.to_string());
            assert!(err.contains(says), "{e}: {err}");
        }
    }

    /// A count from a length (`(L - 20) / 210`, the 210 read as unit-less) names the fix (#5901).
    #[test]
    fn a_length_where_a_count_is_expected_says_to_divide_by_a_length() {
        let e = ev("width / 2", Kind::Unitless).unwrap_err().to_string();
        assert!(e.contains("expected a unit-less number, got a length") && e.contains("divide by a length"), "{e}");
        assert_eq!(ev("floor(width / (6 mm))", Kind::Unitless).unwrap(), 6.0);
        let e = ev("45 deg", Kind::Unitless).unwrap_err().to_string();
        assert!(!e.contains("divide"), "{e}");
    }

    #[test]
    fn errors() {
        assert!(ev("width + ang", Kind::Length).is_err());
        let e = ev("10 mm + 5 deg", Kind::Length).unwrap_err().to_string();
        assert!(e.contains("cannot add a length and an angle"), "{e}");
        assert!(ev("width * width", Kind::Length).unwrap_err().to_string().contains("expected a length"));
        assert!(ev("nope", Kind::Length).is_err());
        assert!(ev("(1", Kind::Length).is_err());
        assert!(ev("1 / 0", Kind::Length).is_err());
        assert!(ev("", Kind::Length).is_err());
        assert!(ev("1 $ 2", Kind::Length).is_err());
        assert!(ev("sqrt(-1)", Kind::Unitless).is_err());
        assert!(ev("asin(2)", Kind::Angle).is_err());
        assert!(ev("ln(0)", Kind::Unitless).is_err());
        assert!(ev("sin(1 mm)", Kind::Unitless).is_err());
        assert!(ev("atan2(1 mm, 1 deg)", Kind::Angle).is_err());
        assert!(ev("10 ^ 400", Kind::Unitless).is_err());
        assert!(ev("width ^ 1.5", Kind::Unitless).is_err());
        assert!(ev("max()", Kind::Unitless).is_err());
        assert!(ev("frob(1)", Kind::Unitless).is_err());
        let deep = "(".repeat(500) + "1" + &")".repeat(500);
        assert!(ev(&deep, Kind::Length).is_err());
        assert!(ev(&"-".repeat(500), Kind::Length).is_err());
        assert!(ev(&"+".repeat(500), Kind::Length).is_err());
        assert!(ev(&"width*".repeat(100), Kind::Length).is_err());
        assert!(ev(&("2^".repeat(200) + "2"), Kind::Unitless).is_err());
        // A power of 16 times -8 is -128, whose `abs` overflows: refused, not a panic.
        assert!(ev("((1 mm)^15 * 1 mm) ^ -8", Kind::Unitless).is_err());
        assert!(ev("((1 mm)^15 * 1 mm) ^ 8", Kind::Unitless).is_err());
    }

    /// A value's mass power (#4811) is carried through arithmetic and checked like the others: a
    /// mass beside a length is refused wherever two values must have the same units.
    #[test]
    fn a_mass_power_is_carried_and_checked() {
        let look = |n: &str| match n {
            "m" => Ok(Value::mass(50.0)),
            "w" => Ok(Value::length(10.0)),
            _ => Err(DocError::Expr(format!("unknown {n}"))),
        };
        // (Bare numbers beside a mass are read in its kind's unit, which the kinds bring.)
        let ev = |s: &str| eval_with(s, &look);
        assert_eq!(ev("m * 2 + m").unwrap(), Value::mass(150.0));
        assert_eq!(ev("max(m, 2 * m)").unwrap(), Value::mass(100.0));
        assert_eq!(ev("abs(-m)").unwrap(), Value::mass(50.0));
        assert_eq!(ev("(m + m / 5) % m").unwrap(), Value::mass(10.0));
        assert_eq!(ev("sqrt(m * m)").unwrap(), Value::mass(50.0));
        assert_eq!(ev("m / m").unwrap(), Value::num(1.0));
        assert_eq!(ev("m / w^3").unwrap(), Value { v: 0.05, len: -3, ang: 0, mass: 1 });
        assert_eq!(ev("m / w^3").unwrap().describe(), "a value in length^-3·mass");
        assert_eq!(ev("m * w").unwrap().describe(), "a value in length·mass");
        // Every operation that needs matching units refuses a mass beside a length.
        for bad in [
            "m + w",
            "w - m",
            "m % w",
            "mod(w, m)",
            "atan2(m, w)",
            "hypot(w, m)",
            "min(m, w)",
            "max(w, m)",
            "sin(m)",
            "cos(m)",
            "tan(m)",
            "asin(m)",
            "acos(m)",
            "atan(m)",
            "exp(m)",
            "ln(m)",
            "log(m)",
            "log10(m)",
            "sqrt(m)",
            "sqrt(m * w * w)",
            "m ^ 0.5",
            "2 ^ m",
            "pow(w, m)",
        ] {
            assert!(ev(bad).is_err(), "{bad}");
        }
        for k in [Kind::Length, Kind::Angle, Kind::Unitless] {
            let e = ev("m").unwrap().to_kind(k).unwrap_err().to_string();
            assert!(e.contains(&format!("expected {}", kind_word(k))), "{e}");
        }
        // A mass's power stays within ±16, as the others do.
        assert_eq!(ev("m ^ 15 / m ^ 15").unwrap(), Value::num(1.0));
        assert!(ev("m ^ 15 * m * m").is_err());
        assert!(ev("1 / m ^ 15 / m / m").is_err());
    }

    /// Mass, volume and area are kinds (#4811): a length³ is a volume and a length² an area. A
    /// value converts only to its own kind, and a bare number beside one of them, or as one, is
    /// read in the parameter's unit for that kind (grams, mm³ and mm² when it has none).
    #[test]
    fn mass_volume_and_area_are_kinds() {
        let look = |n: &str| match n {
            "m" => Ok(Value::mass(50.0)),
            "w" => Ok(Value::length(10.0)),
            _ => Err(DocError::Expr(format!("unknown {n}"))),
        };
        let ev = |s: &str, k: Kind| eval_with(s, &look).and_then(|v| v.to_kind(k));
        assert_eq!(ev("m", Kind::Mass).unwrap(), 50.0);
        assert_eq!(ev("w^3", Kind::Volume).unwrap(), 1000.0);
        assert_eq!(ev("w * w * 2", Kind::Area).unwrap(), 200.0);
        assert_eq!(ev("w^3 / w", Kind::Area).unwrap(), 100.0);
        assert_eq!(ev("sqrt(w^2 * 4)", Kind::Length).unwrap(), 20.0);
        assert_eq!(ev("(2 mm)^3 + 5", Kind::Volume).unwrap(), 13.0, "a bare number beside a volume is mm³");
        assert_eq!(ev("w^2 + 5", Kind::Area).unwrap(), 105.0);
        // A bare mass is in kilograms, the unit a mass shows in.
        assert_eq!(ev("3", Kind::Mass).unwrap(), 3000.0);
        assert_eq!(ev("m + 1", Kind::Mass).unwrap(), 1050.0);
        assert_eq!(ev("m % 0.03125", Kind::Mass).unwrap(), 18.75);
        assert_eq!(ev("round(m * 30)", Kind::Mass).unwrap(), 2000.0, "rounded in kilograms: 1.5 kg to 2 kg");
        // Each converts only to its own kind, and the error says what it got.
        for (s, k, got) in [
            ("m", Kind::Length, "a mass"),
            ("w^3", Kind::Length, "a volume"),
            ("w^2", Kind::Volume, "an area"),
            ("w", Kind::Area, "a length"),
            ("w^3", Kind::Unitless, "a volume"),
            ("m * w", Kind::Mass, "a value in length·mass"),
        ] {
            let e = ev(s, k).unwrap_err().to_string();
            assert!(e.contains(&format!("expected {}, got {got}", kind_word(k))), "{s}: {e}");
        }
        for k in Kind::ALL {
            assert_eq!(Value::of_kind(k, 1.5).kind(), Some(k), "{k:?}");
            assert_eq!(Value::of_kind(k, 1.5).to_kind(k).unwrap(), 1.5, "{k:?}");
        }
        // In a gram parameter (and cm³, cm²), bare numbers are in its unit.
        let d = Defaults { mass: 1.0, vol: 1000.0, area: 100.0, ..Defaults::default() };
        let in_d = |s: &str, k: Kind| eval_with_defaults(s, &look, d).and_then(|v| v.to_kind_in(k, d));
        assert_eq!(in_d("2", Kind::Mass).unwrap(), 2.0);
        assert_eq!(in_d("m + 1", Kind::Mass).unwrap(), 51.0);
        assert_eq!(in_d("max(m, 60)", Kind::Mass).unwrap(), 60.0);
        assert_eq!(in_d("round(m / 3)", Kind::Mass).unwrap(), 17.0, "rounded in grams");
        assert_eq!(in_d("w^3 + 2", Kind::Volume).unwrap(), 3000.0);
        assert_eq!(in_d("w^2 + 2", Kind::Area).unwrap(), 300.0);
        assert_eq!(in_d("w + 1", Kind::Length).unwrap(), 11.0, "a length keeps its own");
        // A value is shown in a unit of its own kind only.
        assert_eq!(value_in_unit(Value::mass(500.0), "deg"), 500.0);
        assert_eq!(value_in_unit(Value::length(500.0), "cm"), 50.0);
    }

    /// Mass, volume and area units (#4811), with superscripts for the powers (`cm^3` works too).
    #[test]
    fn mass_volume_and_area_units() {
        let near = |s: &str, k: Kind, want: f64| {
            let got = ev(s, k).unwrap_or(f64::NAN);
            assert!((got - want).abs() <= 1e-12 * want.abs().max(1.0), "{s}: {got}, not {want}");
        };
        near("50 g", Kind::Mass, 50.0);
        near("0.5 kg", Kind::Mass, 500.0);
        near("1 lb", Kind::Mass, 453.592_37);
        near("1 kg + 500 g", Kind::Mass, 1500.0);
        near("max(1 kg, 900 g)", Kind::Mass, 1000.0);
        near("min(1 lb, 500 g)", Kind::Mass, 453.592_37);
        near("2 * 1,5 kg", Kind::Mass, 3000.0);
        near("500 ml", Kind::Volume, 500_000.0);
        near("1 ml - 1 cm³", Kind::Volume, 0.0);
        near("2 cm³", Kind::Volume, 2000.0);
        near("2 cm^3", Kind::Volume, 2000.0);
        near("1 m³", Kind::Volume, 1e9);
        near("3 mm³ + 2", Kind::Volume, 5.0);
        near("10 cm * 10 cm * 10 cm", Kind::Volume, 1e6);
        near("1.5 m²", Kind::Area, 1.5e6);
        near("3 cm²", Kind::Area, 300.0);
        near("2mm²", Kind::Area, 2.0);
        near("1 m² / 1 m", Kind::Length, 1000.0);
        near("sqrt(4 cm²)", Kind::Length, 20.0);
        near("1 m³ / 1 m²", Kind::Length, 1000.0);
        // A density times a volume is a mass: steel, 7.85 g/cm³, in a 10 mm cube.
        near("7.85 g/cm^3 * (10 mm)^3", Kind::Mass, 7.85);
        near("7.85 g/cm³ * 1 cm³", Kind::Mass, 7.85);
        assert_eq!(eval_with("7.85 g/cm³", &|_| Err(DocError::Expr("none".into()))).unwrap().describe(), "a value in length^-3·mass");
        assert!(is_literal("50 g") && is_literal("-2 m²") && !is_bare_number("50 g"));
        assert_eq!(references("2 g + kg * lb + g2"), ["g2"]);
        // Shown in a unit of their kind.
        assert!((value_in_unit(Value::mass(500.0), "kg") - 0.5).abs() < 1e-12);
        assert!((value_in_unit(Value::mass(453.592_37), "lb") - 1.0).abs() < 1e-12);
        assert_eq!(value_in_unit(Value::of_kind(Kind::Volume, 2e9), "m³"), 2.0);
        assert_eq!(value_in_unit(Value::of_kind(Kind::Area, 300.0), "cm²"), 3.0);
        assert_eq!(value_in_unit(Value::mass(500.0), "mm³"), 500.0);
        assert_eq!(unit_info("kg"), Some((Kind::Mass, 1000.0)));
        assert_eq!(Kind::from_unit("m²"), Kind::Area);
        assert_eq!((Kind::Mass.default_unit(), Kind::Volume.default_unit(), Kind::Area.default_unit()), ("kg", "mm³", "mm²"));
        // A kind's default unit is one of its own, and a bare number of that kind is read in it:
        // what a value shows as and what a bare number means agree (`3` for a mass is 3 kg).
        for k in Kind::ALL {
            let (unit_kind, size) = unit_info(k.default_unit()).unwrap_or((Kind::Unitless, f64::NAN));
            assert_eq!((unit_kind, size), (k, Defaults::default().scale(k)), "{k:?}");
        }
        // An unknown unit is refused naming every unit there is.
        assert_eq!((unit_problem("kg"), unit_problem(""), unit_problem("degrees")), (None, None, None));
        let why = unit_problem("m3").unwrap_or_default();
        assert!(why.starts_with("unknown unit `m3` (") && why.ends_with(" or none)"), "{why}");
        assert!(UNITS.iter().all(|(name, _, _)| why.split(['(', ',', ' ', ')']).any(|word| word == *name)), "{why}");
    }

    /// A mass, volume or area unit is refused wherever its value would meet a value of another
    /// kind, operation by operation.
    #[test]
    fn mass_volume_and_area_units_refuse_other_kinds() {
        for bad in [
            "1 g + 1 mm",
            "1 g - 1 mm",
            "1 g % 1 mm",
            "mod(1 g, 1 mm)",
            "atan2(1 g, 1 mm)",
            "hypot(1 g, 1 mm)",
            "min(1 g, 1 mm)",
            "max(1 mm, 1 kg)",
            "1 g + 1 deg",
            "1 ml + 1 mm²",
            "1 mm² + 1 mm",
            "1 m³ + 1 kg",
            "sin(1 g)",
            "cos(1 ml)",
            "tan(1 m²)",
            "asin(1 g)",
            "acos(1 g)",
            "atan(1 g)",
            "exp(1 g)",
            "ln(1 kg)",
            "log(1 lb)",
            "log10(1 g)",
            "sqrt(1 g)",
            "sqrt(1 ml)",
            "1 g ^ 0.5",
            "2 ^ (1 g)",
            "pow(2, 1 ml)",
            "(1 kg)^9 * (1 kg)^8",
        ] {
            assert!(eval_with(bad, &|_| Err(DocError::Expr("none".into()))).is_err(), "{bad}");
        }
        for (s, k, got) in [
            ("1 g", Kind::Length, "a mass"),
            ("1 g * 1 mm", Kind::Length, "a value in length·mass"),
            ("1 ml", Kind::Length, "a volume"),
            ("1 mm²", Kind::Volume, "an area"),
            ("1 m²", Kind::Length, "an area"),
            ("1 kg", Kind::Angle, "a mass"),
            ("1 kg", Kind::Unitless, "a mass"),
            ("1 mm", Kind::Mass, "a length"),
        ] {
            let e = ev(s, k).unwrap_err().to_string();
            assert!(e.contains(&format!("expected {}, got {got}", kind_word(k))), "{s}: {e}");
        }
    }

    /// In a mass, volume or area parameter, bare numbers are in its unit (#4811).
    #[test]
    fn parameters_in_mass_volume_and_area_units() {
        let p = vec![
            def("m", "2", "kg"),
            def("half", "m / 2 + 1", "kg"),
            def("r", "round(2.4 kg)", "kg"),
            def("grams", "m", "g"),
            def("v", "5", "cm³"),
            def("box", "(1 cm)^3 + 1", "cm³"),
            def("a", "2", "m²"),
            def("pounds", "1", "lb"),
            def("bad", "m + 1 mm", "kg"),
            def("wrong", "1 kg", "mm"),
        ];
        let (v, e) = eval_params(&p, 1.0);
        assert_eq!(v["m"], Value::mass(2000.0));
        assert_eq!(v["half"], Value::mass(2000.0), "1 kg + 1 kg");
        assert_eq!(v["r"], Value::mass(2000.0), "rounded in kilograms");
        assert_eq!(v["grams"], Value::mass(2000.0));
        assert_eq!(v["v"], Value::of_kind(Kind::Volume, 5000.0));
        assert_eq!(v["box"], Value::of_kind(Kind::Volume, 2000.0));
        assert_eq!(v["a"], Value::of_kind(Kind::Area, 2e6));
        assert_eq!(v["pounds"], Value::mass(453.592_37));
        assert!(e["bad"].contains("cannot add a mass and a length"), "{}", e["bad"]);
        assert!(e["wrong"].contains("expected a length, got a mass"), "{}", e["wrong"]);
        assert_eq!(value_in_unit(v["half"], "kg"), 2.0);
    }

    /// Hostile input never panics: random strings over the expression alphabet.
    #[test]
    fn fuzz_never_panics() {
        let alphabet: Vec<char> = "0123456789.eE+-*/^(),;% abcdmnpitrsgxwkl_°×÷−µ²³\u{0}\u{7f}é\"'".chars().collect();
        let words = [
            "width", "sin(", "atan2(", "mm", "deg", "pi", "1e308", "1e-400", "(", ")", "^", "max(", "in", "count", ",", "g", "kg", "lb", "ml", "cm³",
            "m²", "mm³",
        ];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..20_000 {
            let n = (rnd() % 40) as usize;
            let mut s = String::new();
            for _ in 0..n {
                if rnd() % 4 == 0 {
                    s.push_str(words[(rnd() % words.len() as u64) as usize]);
                } else {
                    s.push(alphabet[(rnd() % alphabet.len() as u64) as usize]);
                }
            }
            for k in Kind::ALL {
                let _ = ev(&s, k);
            }
            let _ = references(&s);
            let _ = rename_reference(&s, "width", "w2");
            let _ = rename_word(&s, "mm", "mm_param");
            let _ = is_literal(&s);
        }
    }

    #[test]
    fn params_in_order_with_cycles() {
        let p = vec![def("b", "a * 2", "mm"), def("a", "10 mm", "mm"), def("x", "y", "mm"), def("y", "z + 1", "mm"), def("z", "x", "mm")];
        let (v, e) = eval_params(&p, 1.0);
        assert_eq!(v.get("b").unwrap().v, 20.0);
        assert!(e.contains_key("x") && e.contains_key("y") && e.contains_key("z"));
        assert!(e["x"].contains("circular reference: x → y → z → x"), "{}", e["x"]);
        assert_eq!(references("2*width + sin(ang) + 3 mm + pi"), vec!["width".to_string(), "ang".to_string()]);
        let (_, e) = eval_params(&[def("s", "s", "mm")], 1.0);
        assert!(e["s"].contains("s → s"));
    }

    #[test]
    fn parameter_units() {
        let p = vec![
            def("a", "2", "in"),
            def("b", "a + 1", "in"),
            def("c", "b", "mm"),
            def("t", "0.5", "rad"),
            def("u", "t + 1", "rad"),
            def("k", "3", ""),
            def("bad", "a + t", "mm"),
            def("mixed", "1 in + 2", "mm"),
        ];
        let (v, e) = eval_params(&p, 1.0);
        assert!((v["a"].v - 50.8).abs() < 1e-9);
        assert!((v["b"].v - 76.2).abs() < 1e-9);
        assert!((v["c"].v - 76.2).abs() < 1e-9);
        assert!((v["u"].v - 1.5).abs() < 1e-12);
        assert_eq!(v["k"], Value::num(3.0));
        assert!(e["bad"].contains("cannot add"));
        assert!((v["mixed"].v - 27.4).abs() < 1e-9);
        assert!((value_in_unit(v["b"], "in") - 3.0).abs() < 1e-12);
        assert!((value_in_unit(Value::angle(std::f64::consts::PI), "deg") - 180.0).abs() < 1e-9);
    }

    #[test]
    fn rename_and_literals() {
        assert_eq!(rename_reference("width*2 + widths + width(1) + sin(width)", "width", "w"), "w*2 + widths + width(1) + sin(w)");
        assert_eq!(rename_reference("10 mm", "mm", "x"), "10 mm");
        // An older build's reading renames a word even where it is now a unit; calls stay.
        assert_eq!(rename_word("10 mm + mm(1) + mms + sin(mm) + 2mm^2", "mm", "mm_param"), "10 mm_param + mm(1) + mms + sin(mm_param) + 2mm_param^2");
        assert_eq!(rename_word("width*2 + width(1)", "width", "w"), rename_reference("width*2 + width(1)", "width", "w"));
        assert_eq!(rename_word("1 $ mm", "mm", "x"), "1 $ mm", "text that doesn't lex is kept");
        assert!(is_literal("20") && is_literal("-1.5 in") && is_literal("3 deg"));
        assert!(!is_literal("d1") && !is_literal("2 * 3") && !is_literal("2 width"));
        assert!(is_bare_number(" -1.5 ") && is_bare_number("+2") && !is_bare_number("20 mm") && !is_bare_number("(2)"));
        // A leading `=` only marks a formula (#4681): `=5` is the bare number `5`, not `==5`.
        assert!(is_bare_number("=5") && is_bare_number(" = -1.5") && !is_bare_number("=5 mm") && !is_bare_number("==5") && !is_bare_number("=d3"));
        assert_eq!((length_unit_or_mm(" in "), length_unit_or_mm("deg"), length_unit_or_mm("")), ("in", "mm", "mm"));
    }

    /// `name = expression` splits at the first `=` when the left side is shaped like a name; the
    /// right side is any text, checked by whoever evaluates it.
    #[test]
    fn definitions_split_on_a_name_and_an_equals_sign() {
        let splits = [
            ("w = 20", "w", "20"),
            ("w=20", "w", "20"),
            ("  w  =  20 mm ", "w", "20 mm"),
            ("w = (a + b) / 2", "w", "(a + b) / 2"),
            ("w = 2 * (3 + sin(45 deg))", "w", "2 * (3 + sin(45 deg))"),
            ("w = 1 in - 5.4 mm", "w", "1 in - 5.4 mm"),
            ("_w1 = 1", "_w1", "1"),
            ("szerokość = 20", "szerokość", "20"),
            // A `#` before the name marks it as a new parameter's (#2371).
            ("#w = 20", "w", "20"),
            ("  #w=20 mm", "w", "20 mm"),
            // Reserved names split too: the parameter check names the reason.
            ("mm = 5", "mm", "5"),
            ("w = a = 3", "w", "a = 3"),
            // An empty expression is the caller's error to report.
            ("w =", "w", ""),
            ("w = ", "w", ""),
        ];
        for (text, name, rhs) in splits {
            assert_eq!(definition(text), Some((name, rhs)), "{text:?}");
        }
        for text in [
            "20",
            "20 mm",
            "width / 2",
            "= 20",
            "=",
            "",
            "2w = 3",
            "a b = 3",
            "a-b = 3",
            "w.x = 3",
            "(w) = 3",
            "-w = 3",
            "# = 3",
            "#= 3",
            "##w = 3",
            "# w = 3",
            "#2w = 3",
            "#w",
            "w == 3",
        ] {
            assert_eq!(definition(text), None, "{text:?}");
        }
    }

    /// Until the design stores it, a definition is worth its expression (#4071): the name is
    /// not a reference, it is never a plain number, and it renames like its expression.
    #[test]
    fn a_definition_evaluates_as_its_expression() {
        let look = |n: &str| if n == "a" { Ok(Value::length(4.0)) } else { Err(DocError::Expr(format!("unknown parameter `{n}`"))) };
        let ev = |s: &str| eval_with(s, &look).and_then(|v| v.to_kind(Kind::Length));
        assert_eq!(ev("w = 20").ok(), Some(20.0));
        assert_eq!(ev("#w = a * 2").ok(), Some(8.0));
        assert_eq!(ev("szerokość=1 in").ok(), Some(25.4));
        assert_eq!(references("w = a + b"), ["a", "b"]);
        assert!(references("w = 3").is_empty());
        assert_eq!(rename_reference("w = a * 2", "a", "len"), "w = len * 2");
        assert_eq!(rename_reference("a = a * 2", "a", "len"), "a = len * 2");
        assert!(!is_literal("w = 20") && !is_bare_number("w = 20"));
        assert!(ev("w = w").unwrap_err().to_string().contains("unknown parameter `w`"));
        assert!(ev("w =").unwrap_err().to_string().contains("`w =` needs a value after the `=`"));
        for bad in ["w = = 3", "2 = 3", "w == 3", "w = 3 = 4", "= 5 ="] {
            assert!(ev(bad).is_err(), "{bad}");
        }
    }

    /// A reserved name is refused by saying what kind of word it is.
    #[test]
    fn a_reserved_name_is_refused_by_its_kind() {
        assert_eq!(
            (reserved_kind("mm"), reserved_kind("deg"), reserved_kind("pi"), reserved_kind("sin"), reserved_kind("width")),
            (Some("unit"), Some("unit"), Some("constant"), Some("function"), None)
        );
        let problem = |n: &str| crate::Document::param_name_problem(n).unwrap_or_default();
        assert_eq!(problem("mm"), "`mm` is a unit name, so it cannot be a parameter name");
        assert_eq!(problem("sin"), "`sin` is a function name, so it cannot be a parameter name");
        assert_eq!(problem("pi"), "`pi` is a constant, so it cannot be a parameter name");
        assert_eq!(crate::Document::param_name_problem("width"), None);
        assert!(problem("2w").contains("not a valid parameter name"));
        // Every unit is reserved (#4811), its superscripts too; the words left out on purpose
        // stay free for parameters.
        for (u, _, _) in UNITS {
            assert_eq!(reserved_kind(u), Some("unit"), "{u}");
        }
        assert_eq!(problem("g"), "`g` is a unit name, so it cannot be a parameter name");
        assert_eq!(problem("cm³"), "`cm³` is a unit name, so it cannot be a parameter name");
        for free in ["l", "L", "t", "m2", "m3", "cm3", "oz", "cl", "mg", "cc", "tonne", "G", "KG", "g_param", "mm³_param"] {
            assert_eq!(crate::Document::param_name_problem(free), None, "{free}");
        }
    }

    /// The kept answers of `references` and `eval_params` never stand for different input: an
    /// edited expression, unit or design length unit gives its own values.
    #[test]
    fn kept_parameter_values_follow_every_change() {
        assert_eq!(references("width * 2 + 3 mm"), vec!["width".to_string()]);
        assert_eq!(references("width * 2 + 3 mm"), vec!["width".to_string()]);
        assert!(references("10 mm").is_empty());
        let mm = |v: &BTreeMap<String, Value>, n: &str| v.get(n).map(|x| x.v);
        let mut defs = vec![def("a", "10 mm", "mm"), def("b", "a * 2", "mm"), def("c", "5", "mm")];
        let (first, _) = eval_params(&defs, 1.0);
        assert_eq!((mm(&first, "a"), mm(&first, "b"), mm(&first, "c")), (Some(10.0), Some(20.0), Some(5.0)));
        defs[0].expr = "12 mm".into();
        let (edited, _) = eval_params(&defs, 1.0);
        assert_eq!((mm(&edited, "a"), mm(&edited, "b")), (Some(12.0), Some(24.0)));
        defs[0].expr = "10 mm".into();
        assert_eq!(eval_params(&defs, 1.0).0, first, "back as it was");
        // Whatever changes, the kept answer is the one worked out afresh.
        for (unit, expr, len) in [("mm", "5", 25.4), ("", "5", 25.4), ("in", "5", 1.0), ("deg", "5", 1.0), ("", "5", 1.0)] {
            defs[2].unit = unit.into();
            defs[2].expr = expr.into();
            assert_eq!(eval_params(&defs, len), eval_params_uncached(&defs, len), "{unit} {expr} {len}");
        }
        defs[2].expr = "bad +".into();
        assert!(eval_params(&defs, 1.0).1.contains_key("c"));
    }
}
