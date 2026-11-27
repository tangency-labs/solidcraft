//! Ordering-code field templates (docs/formats/generator.md, section 3): a template such as
//! `MGN{size}{block: C=standard, H=heavy}[E]{blocks}R{length: int 40..2000}` reads a code into
//! typed parameters and prints the parameters back as a code. Matching is hand-written, tries the
//! longest fit first, backs up when a later part fails and stops after a fixed number of steps.
//! There is no regular-expression engine, so no input can make it run away.

use std::collections::BTreeMap;

use serde_json::Value as Json;

/// An ordering code, in characters.
pub const MAX_CODE_CHARS: usize = 128;
/// A template, in characters.
pub const MAX_TEMPLATE_CHARS: usize = 256;
/// Fields (and optional parts) in one template.
pub const MAX_FIELDS: usize = 32;
/// Matching steps for one code, all templates together.
pub const MATCH_BUDGET: u32 = 20_000;
/// Values one choice or letter map may list.
const MAX_CHOICES: usize = 256;
/// Optional parts nest at most this deep.
const MAX_DEPTH: usize = 4;

/// The parameters a code means: a value per field, by field name.
pub type Params = BTreeMap<String, Json>;

/// Why a template or a code was not accepted.
#[derive(Clone, Debug, PartialEq)]
pub enum TemplateError {
    /// The template text is malformed.
    Syntax(String),
    /// A bound was exceeded.
    Limit { what: &'static str, limit: usize },
    /// The code does not fit the template.
    NoMatch(String),
    /// A parameter cannot be written as this template's field.
    Field { field: String, message: String },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::Syntax(m) => write!(f, "ordering-code template: {m}"),
            TemplateError::Limit { what, limit } => write!(f, "ordering code limit: more than {limit} {what}"),
            TemplateError::NoMatch(m) => f.write_str(m),
            TemplateError::Field { field, message } => write!(f, "`{field}`: {message}"),
        }
    }
}

impl std::error::Error for TemplateError {}

fn syntax<T>(msg: impl Into<String>) -> Result<T, TemplateError> {
    Err(TemplateError::Syntax(msg.into()))
}

/// Characters a code may separate its parts with; they are ignored when reading.
pub fn is_separator(c: char) -> bool {
    matches!(c, '-' | '_' | ' ' | '/' | '.')
}

/// A plain name: letters, digits and underscores, not starting with a digit.
pub fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && s.len() <= 64 && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A set of extra characters a free-text field accepts, written as `[A-Za-z0-9-]` (ranges and
/// single characters; an optional trailing `+` or `*` is accepted and ignored).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharSet(Vec<(char, char)>);

impl CharSet {
    pub fn parse(text: &str) -> Result<CharSet, TemplateError> {
        let t = text.trim();
        let t = t.strip_suffix(['+', '*']).unwrap_or(t);
        let Some(inner) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) else {
            return syntax(format!("an escape pattern looks like `[A-Za-z0-9-]`, not `{text}`"));
        };
        let chars: Vec<char> = inner.chars().collect();
        if chars.is_empty() || chars.len() > 64 {
            return syntax("an escape pattern lists 1 to 64 characters");
        }
        let mut ranges = Vec::new();
        let mut i = 0;
        while let Some(&a) = chars.get(i) {
            match (chars.get(i + 1), chars.get(i + 2)) {
                (Some('-'), Some(&b)) => {
                    if a > b {
                        return syntax(format!("the range `{a}-{b}` runs backwards"));
                    }
                    ranges.push((a, b));
                    i += 3;
                }
                _ => {
                    ranges.push((a, a));
                    i += 1;
                }
            }
        }
        Ok(CharSet(ranges))
    }

    pub fn contains(&self, c: char) -> bool {
        self.0.iter().any(|(a, b)| (*a..=*b).contains(&c))
    }
}

/// What one field accepts.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    /// One of a list; the longest listed value that fits is taken.
    Choice(Vec<String>),
    /// Code letters standing for values (`C=standard`); printing writes the letters back.
    Map(Vec<(String, String)>),
    /// A whole number in a range, on a step counted from `min`.
    Int { min: i64, max: i64, step: i64 },
    /// A decimal with at most `digits` places; a `step` of 0 allows any value.
    Decimal { min: f64, max: f64, step: f64, digits: usize },
    /// Letters and digits, plus the characters of `extra`.
    Text { min: usize, max: usize, extra: CharSet },
    /// Options in any order, each at most once, some with an argument (`-LTP`, `-SC12`). The value
    /// is an object: the option's key (its code without separators) to `true` or its argument.
    Set(Vec<SetOption>),
    /// Everything left of the code, as typed (a free tail such as `+SPECIAL`). Always last.
    Rest,
}

/// One option of a [`FieldKind::Set`].
#[derive(Clone, Debug, PartialEq)]
pub struct SetOption {
    /// The code as written, separators included (`-SC`).
    pub code: String,
    /// The key in the value: the code's letters and digits (`SC`).
    pub key: String,
    /// What follows the code, when the option takes an argument.
    pub arg: Option<Box<FieldKind>>,
}

/// A declared parameter: what its field accepts and the value an omitted optional part gives.
#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    pub kind: FieldKind,
    pub default: Option<Json>,
}

/// The parameters a package declares, by name.
pub type Declared = BTreeMap<String, Decl>;

#[derive(Clone, Debug)]
enum Node {
    /// Fixed text, case ignored; its separators match any run of separators, except one written
    /// `\-` (the `true` here), which must be present as it is.
    Lit(Vec<(char, bool)>),
    Field {
        name: String,
        kind: FieldKind,
    },
    /// A part that may be left out. `flag` is a boolean parameter that says whether it is there;
    /// `absent` are the values set when it is left out.
    Optional {
        nodes: Vec<Node>,
        flag: Option<String>,
        absent: Vec<(String, Json)>,
    },
}

/// A compiled template.
#[derive(Clone, Debug)]
pub struct Template {
    source: String,
    nodes: Vec<Node>,
    fields: Vec<String>,
}

struct Parser<'a> {
    chars: Vec<char>,
    at: usize,
    declared: &'a Declared,
    count: usize,
}

fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, TemplateError> {
    match text.trim().parse::<T>() {
        Ok(n) => Ok(n),
        Err(_) => syntax(format!("`{text}` is not a valid {what}")),
    }
}

fn range_text(spec: &str) -> Result<(&str, &str, Vec<&str>), TemplateError> {
    let mut words = spec.split_whitespace();
    let Some(range) = words.next() else { return syntax("a number field needs a range such as `40..2000`") };
    let Some((lo, hi)) = range.split_once("..") else { return syntax(format!("`{range}` is not a range such as `40..2000`")) };
    Ok((lo, hi, words.collect()))
}

fn spec_kind(spec: &str) -> Result<FieldKind, TemplateError> {
    let spec = spec.trim();
    let (head, rest) = spec.split_once(char::is_whitespace).unwrap_or((spec, ""));
    match head {
        "int" => {
            let (lo, hi, extra) = range_text(rest)?;
            let (min, max): (i64, i64) = (number(lo, "whole number")?, number(hi, "whole number")?);
            let mut step = 1;
            let mut it = extra.into_iter();
            while let Some(word) = it.next() {
                match (word, it.next()) {
                    ("step", Some(n)) => step = number(n, "step")?,
                    _ => return syntax(format!("`{word}` is not understood after an int range (only `step N`)")),
                }
            }
            if min > max || step < 1 || min.abs() > 1_000_000_000_000 || max.abs() > 1_000_000_000_000 {
                return syntax("an int field needs min <= max, |values| <= 10^12 and a step of 1 or more");
            }
            Ok(FieldKind::Int { min, max, step })
        }
        "decimal" => {
            let (lo, hi, extra) = range_text(rest)?;
            let (min, max): (f64, f64) = (number(lo, "number")?, number(hi, "number")?);
            let (mut step, mut digits): (f64, usize) = (0.0, 3);
            let mut it = extra.into_iter();
            while let Some(word) = it.next() {
                match (word, it.next()) {
                    ("step", Some(n)) => step = number(n, "step")?,
                    ("digits", Some(n)) => digits = number(n, "digit count")?,
                    _ => return syntax(format!("`{word}` is not understood after a decimal range (`step N`, `digits N`)")),
                }
            }
            if !(min.is_finite() && max.is_finite() && step.is_finite()) || min > max || step < 0.0 || digits > 9 {
                return syntax("a decimal field needs finite min <= max, a step of 0 or more and at most 9 digits");
            }
            Ok(FieldKind::Decimal { min, max, step, digits })
        }
        "rest" if rest.trim().is_empty() => Ok(FieldKind::Rest),
        _ if spec.starts_with("set(") => set_kind(spec),
        "text" => {
            let (lo, hi, extra) = range_text(rest)?;
            let (min, max): (usize, usize) = (number(lo, "length")?, number(hi, "length")?);
            if min == 0 || min > max || max > MAX_CODE_CHARS || !extra.is_empty() {
                return syntax("a text field needs 1 <= min <= max <= 128 characters, as `text 1..8`");
            }
            Ok(FieldKind::Text { min, max, extra: CharSet::default() })
        }
        _ if spec.contains('=') => {
            let mut pairs = Vec::new();
            for part in spec.split(',') {
                let Some((code, value)) = part.split_once('=') else { return syntax(format!("`{}` is not `LETTER=value`", part.trim())) };
                let (code, value) = (code.trim(), value.trim());
                if code.is_empty() || code.chars().count() > 16 || code.chars().any(|c| is_separator(c) || c.is_whitespace()) {
                    return syntax(format!("`{code}` is not a code of 1 to 16 characters without separators"));
                }
                if value.is_empty() || value.chars().count() > 64 {
                    return syntax(format!("the value for `{code}` must be 1 to 64 characters"));
                }
                pairs.push((code.to_string(), value.to_string()));
            }
            if pairs.is_empty() || pairs.len() > MAX_CHOICES {
                return syntax("a letter map lists 1 to 256 pairs");
            }
            Ok(FieldKind::Map(pairs))
        }
        _ => {
            let values: Vec<String> = spec.split('|').map(|v| v.trim().to_string()).collect();
            if values.is_empty()
                || values.len() > MAX_CHOICES
                || values.iter().any(|v| v.is_empty() || v.chars().count() > 64 || v.chars().any(char::is_whitespace))
            {
                return syntax("a choice lists 1 to 256 values of 1 to 64 characters, separated by `|`");
            }
            Ok(FieldKind::Choice(values))
        }
    }
}

fn flush(out: &mut Vec<Node>, lit: &mut Vec<(char, bool)>) {
    if !lit.is_empty() {
        out.push(Node::Lit(std::mem::take(lit)));
    }
}

fn set_kind(spec: &str) -> Result<FieldKind, TemplateError> {
    let Some(inner) = spec.strip_prefix("set(").and_then(|r| r.trim_end().strip_suffix(')')) else {
        return syntax("an option set is written `set(-A, -B:int 0..99)`");
    };
    let mut options: Vec<SetOption> = Vec::new();
    for part in inner.split(',') {
        let (code, arg) = match part.split_once(':') {
            Some((c, a)) => (c.trim(), Some(a)),
            None => (part.trim(), None),
        };
        let key: String = code.chars().filter(char::is_ascii_alphanumeric).collect();
        if key.is_empty() || code.chars().count() > 16 || code.chars().any(char::is_whitespace) {
            return syntax(format!("`{code}` is not an option code of 1 to 16 characters with a letter or digit"));
        }
        if options.iter().any(|o| o.key.eq_ignore_ascii_case(&key)) {
            return syntax(format!("the option `{key}` is listed twice"));
        }
        let arg = match arg {
            Some(a) => match spec_kind(a)? {
                FieldKind::Set(_) | FieldKind::Rest => return syntax("an option's argument is a number, a choice or text"),
                k => Some(Box::new(k)),
            },
            None => None,
        };
        options.push(SetOption { code: code.to_string(), key, arg });
        if options.len() > 16 {
            return syntax("an option set lists at most 16 options");
        }
    }
    Ok(FieldKind::Set(options))
}

impl Parser<'_> {
    fn field(&mut self, body: &str) -> Result<Node, TemplateError> {
        self.count += 1;
        if self.count > MAX_FIELDS {
            return Err(TemplateError::Limit { what: "template fields", limit: MAX_FIELDS });
        }
        let (name, spec) = match body.split_once(':') {
            Some((n, s)) => (n.trim(), Some(s)),
            None => (body.trim(), None),
        };
        if !is_name(name) {
            return syntax(format!("`{name}` is not a plain field name"));
        }
        let kind = match spec {
            Some(s) => spec_kind(s)?,
            None => match self.declared.get(name) {
                Some(d) => d.kind.clone(),
                None => return syntax(format!("the field `{name}` is not declared in [parameters] and has no spec")),
            },
        };
        Ok(Node::Field { name: name.to_string(), kind })
    }

    fn nodes(&mut self, depth: usize, inside: bool) -> Result<(Vec<Node>, Option<String>), TemplateError> {
        let mut out = Vec::new();
        let mut lit: Vec<(char, bool)> = Vec::new();
        let mut flag = None;
        loop {
            let Some(&c) = self.chars.get(self.at) else {
                if inside {
                    return syntax("a `[` is never closed");
                }
                break;
            };
            self.at += 1;
            match c {
                '{' => {
                    flush(&mut out, &mut lit);
                    let start = self.at;
                    while self.chars.get(self.at).is_some_and(|c| *c != '}') {
                        if matches!(self.chars.get(self.at), Some('{' | '[' | ']')) {
                            return syntax("a field's braces hold only its name and spec");
                        }
                        self.at += 1;
                    }
                    if self.chars.get(self.at).is_none() {
                        return syntax("a `{` is never closed");
                    }
                    let body: String = self.chars.get(start..self.at).unwrap_or_default().iter().collect();
                    self.at += 1;
                    let node = self.field(&body)?;
                    out.push(node);
                }
                '}' => return syntax("a `}` without its `{`"),
                '[' => {
                    flush(&mut out, &mut lit);
                    if depth >= MAX_DEPTH {
                        return syntax("optional parts nest at most 4 deep");
                    }
                    self.count += 1;
                    if self.count > MAX_FIELDS {
                        return Err(TemplateError::Limit { what: "template fields", limit: MAX_FIELDS });
                    }
                    let (nodes, inner_flag) = self.nodes(depth + 1, true)?;
                    if nodes.is_empty() {
                        return syntax("an optional part `[]` is empty");
                    }
                    let absent = absent_values(&nodes, inner_flag.as_deref(), self.declared);
                    out.push(Node::Optional { nodes, flag: inner_flag, absent });
                }
                ']' => {
                    if !inside {
                        return syntax("a `]` without its `[`");
                    }
                    flush(&mut out, &mut lit);
                    return Ok((out, flag));
                }
                ':' if inside => {
                    flush(&mut out, &mut lit);
                    let start = self.at;
                    while self.chars.get(self.at).is_some_and(|c| *c != ']') {
                        self.at += 1;
                    }
                    let name: String = self.chars.get(start..self.at).unwrap_or_default().iter().collect();
                    let name = name.trim().to_string();
                    if !is_name(&name) {
                        return syntax(format!("`{name}` is not a plain flag name after `:`"));
                    }
                    flag = Some(name);
                }
                '\\' => match self.chars.get(self.at) {
                    Some(&e) if !e.is_control() => {
                        self.at += 1;
                        lit.push((e, true));
                    }
                    _ => return syntax("a `\\` needs the character it marks"),
                },
                c if c.is_control() => return syntax("a template has no control characters"),
                c => lit.push((c, false)),
            }
        }
        flush(&mut out, &mut lit);
        Ok((out, flag))
    }
}

/// What an optional part sets when it is left out: its fields' declared defaults and its own flags, false.
fn absent_values(nodes: &[Node], flag: Option<&str>, declared: &Declared) -> Vec<(String, Json)> {
    let mut out = Vec::new();
    if let Some(f) = flag {
        out.push((f.to_string(), Json::Bool(false)));
    }
    collect_absent(nodes, declared, &mut out);
    out
}

fn collect_absent(nodes: &[Node], declared: &Declared, out: &mut Vec<(String, Json)>) {
    for n in nodes {
        match n {
            Node::Lit(_) => {}
            Node::Field { name, .. } => {
                if let Some(d) = declared.get(name).and_then(|d| d.default.clone()) {
                    out.push((name.clone(), d));
                }
            }
            Node::Optional { absent, .. } => out.extend(absent.iter().cloned()),
        }
    }
}

fn collect_names(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        match n {
            Node::Lit(_) => {}
            Node::Field { name, .. } => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Node::Optional { nodes, flag, .. } => {
                collect_names(nodes, out);
                if let Some(f) = flag
                    && !out.contains(f)
                {
                    out.push(f.clone());
                }
            }
        }
    }
}

fn sorted_longest(mut v: Vec<(Vec<char>, String)>) -> Vec<(Vec<char>, String)> {
    v.sort_by_key(|(chars, _)| std::cmp::Reverse(chars.len()));
    v
}

impl Template {
    /// Compile `source`. A bare `{name}` takes its kind from `declared`.
    pub fn parse(source: &str, declared: &Declared) -> Result<Template, TemplateError> {
        if source.chars().count() > MAX_TEMPLATE_CHARS {
            return Err(TemplateError::Limit { what: "template characters", limit: MAX_TEMPLATE_CHARS });
        }
        if source.trim().is_empty() {
            return syntax("a template is empty");
        }
        let mut p = Parser { chars: source.chars().collect(), at: 0, declared, count: 0 };
        let (nodes, _) = p.nodes(0, false)?;
        let mut fields = Vec::new();
        collect_names(&nodes, &mut fields);
        Ok(Template { source: source.to_string(), nodes, fields })
    }

    /// The template as written.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// What every code of this template starts with, for an index: the leading fixed text in
    /// capitals, with each value of a leading choice or letter map appended (at most 64).
    pub fn prefixes(&self) -> Vec<String> {
        let mut lit = String::new();
        let mut next = None;
        for n in &self.nodes {
            match n {
                Node::Lit(t) => lit.extend(t.iter().filter(|(c, _)| !is_separator(*c)).map(|(c, _)| c.to_ascii_uppercase())),
                other => {
                    next = Some(other);
                    break;
                }
            }
        }
        match next {
            Some(Node::Field { kind: FieldKind::Choice(v), .. }) => v.iter().take(64).map(|x| format!("{lit}{}", x.to_ascii_uppercase())).collect(),
            Some(Node::Field { kind: FieldKind::Map(p), .. }) => p.iter().take(64).map(|(c, _)| format!("{lit}{}", c.to_ascii_uppercase())).collect(),
            _ => vec![lit],
        }
    }

    /// The parameter names this template sets.
    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    /// Read a code. `Ok(None)`: it does not fit this template.
    pub fn match_code(&self, code: &str) -> Result<Option<Params>, TemplateError> {
        let mut budget = MATCH_BUDGET;
        self.match_with(code, &mut budget)
    }

    fn match_with(&self, code: &str, budget: &mut u32) -> Result<Option<Params>, TemplateError> {
        let input: Vec<char> = code.trim().chars().collect();
        if input.len() > MAX_CODE_CHARS {
            return Err(TemplateError::Limit { what: "ordering code characters", limit: MAX_CODE_CHARS });
        }
        let mut m = Matcher { input: &input, budget };
        m.run(&self.nodes, &[], 0, Params::new())
    }

    /// Write parameters as a code, in this template's spelling. A parameter this template cannot
    /// express (outside its list, map or range) is an error naming the field.
    pub fn print(&self, params: &Params) -> Result<String, TemplateError> {
        let mut out = String::new();
        print_nodes(&self.nodes, params, &mut out)?;
        if out.chars().count() > MAX_CODE_CHARS {
            return Err(TemplateError::Limit { what: "ordering code characters", limit: MAX_CODE_CHARS });
        }
        Ok(out)
    }
}

/// Match `code` against each template in turn, sharing one step budget. The first complete
/// match wins; the index says which template it was.
pub fn match_any(templates: &[Template], code: &str) -> Result<Option<(usize, Params)>, TemplateError> {
    let mut budget = MATCH_BUDGET;
    for (i, t) in templates.iter().enumerate() {
        if let Some(p) = t.match_with(code, &mut budget)? {
            return Ok(Some((i, p)));
        }
    }
    Ok(None)
}

struct Matcher<'a> {
    input: &'a [char],
    budget: &'a mut u32,
}

impl Matcher<'_> {
    fn skip(&self, mut pos: usize) -> usize {
        while self.input.get(pos).is_some_and(|c| is_separator(*c)) {
            pos += 1;
        }
        pos
    }

    fn starts(&self, pos: usize, text: &[char]) -> bool {
        self.input.get(pos..pos + text.len()).is_some_and(|s| s.iter().zip(text).all(|(a, b)| a.eq_ignore_ascii_case(b)))
    }

    /// Where fixed text `text` ends when it is read at `pos`, if it is there.
    fn literal(&self, pos: usize, text: &[(char, bool)]) -> Option<usize> {
        let mut p = if text.first().is_some_and(|(_, required)| *required) { pos } else { self.skip(pos) };
        for (c, required) in text {
            if is_separator(*c) && !required {
                p = self.skip(p);
            } else if self.input.get(p).is_some_and(|i| i.eq_ignore_ascii_case(c)) {
                p += 1;
            } else {
                return None;
            }
        }
        Some(p)
    }

    fn run(&mut self, nodes: &[Node], rest: &[&[Node]], pos: usize, fields: Params) -> Result<Option<Params>, TemplateError> {
        if *self.budget == 0 {
            return Err(TemplateError::Limit { what: "matching steps", limit: MATCH_BUDGET as usize });
        }
        *self.budget -= 1;
        let Some((first, tail)) = nodes.split_first() else {
            return match rest.split_first() {
                Some((next, more)) => self.run(next, more, pos, fields),
                None => Ok((self.skip(pos) == self.input.len()).then_some(fields)),
            };
        };
        match first {
            Node::Lit(text) => match self.literal(pos, text) {
                Some(p) => self.run(tail, rest, p, fields),
                None => Ok(None),
            },
            Node::Field { name, kind } => {
                // A free tail keeps the separators it starts with.
                let at = if matches!(kind, FieldKind::Rest) { pos } else { self.skip(pos) };
                for (len, value) in self.candidates(kind, at) {
                    let mut f = fields.clone();
                    // A letter map also keeps the letters as typed (`block_code`), next to the value.
                    if let FieldKind::Map(pairs) = kind
                        && let Some((code, _)) = pairs.iter().find(|(_, v)| value.as_str() == Some(v.as_str()))
                    {
                        f.insert(format!("{name}_code"), Json::String(code.clone()));
                    }
                    f.insert(name.clone(), value);
                    if let Some(done) = self.run(tail, rest, at + len, f)? {
                        return Ok(Some(done));
                    }
                }
                Ok(None)
            }
            Node::Optional { nodes: inner, flag, absent } => {
                let mut chain: Vec<&[Node]> = Vec::with_capacity(rest.len() + 1);
                chain.push(tail);
                chain.extend_from_slice(rest);
                let mut with = fields.clone();
                if let Some(f) = flag {
                    with.insert(f.clone(), Json::Bool(true));
                }
                if let Some(done) = self.run(inner, &chain, pos, with)? {
                    return Ok(Some(done));
                }
                let mut without = fields;
                for (k, v) in absent {
                    without.insert(k.clone(), v.clone());
                }
                self.run(tail, rest, pos, without)
            }
        }
    }

    /// The readings of a field at `at`, best first: how many characters each takes and its value.
    fn candidates(&self, kind: &FieldKind, at: usize) -> Vec<(usize, Json)> {
        let mut out = Vec::new();
        match kind {
            FieldKind::Choice(values) => {
                let list = sorted_longest(values.iter().map(|v| (v.chars().collect(), v.clone())).collect());
                for (chars, canonical) in &list {
                    if self.starts(at, chars) {
                        out.push((chars.len(), Json::String(canonical.clone())));
                    }
                }
            }
            FieldKind::Map(pairs) => {
                let list = sorted_longest(pairs.iter().map(|(c, v)| (c.chars().collect(), v.clone())).collect());
                for (chars, value) in &list {
                    if self.starts(at, chars) {
                        out.push((chars.len(), Json::String(value.clone())));
                    }
                }
            }
            FieldKind::Int { min, max, step } => {
                let digits = self.input.get(at..).unwrap_or_default().iter().take(15).take_while(|c| c.is_ascii_digit()).count();
                for len in (1..=digits).rev() {
                    let text: String = self.input.get(at..at + len).unwrap_or_default().iter().collect();
                    if len > 1 && text.starts_with('0') {
                        continue;
                    }
                    let Ok(n) = text.parse::<i64>() else { continue };
                    if n < *min || n > *max || (n - min).checked_rem(*step) != Some(0) {
                        continue;
                    }
                    out.push((len, Json::from(n)));
                }
            }
            FieldKind::Decimal { min, max, step, digits } => {
                let run = self.input.get(at..).unwrap_or_default().iter().take(24).take_while(|c| c.is_ascii_digit() || **c == '.').count();
                for len in (1..=run).rev() {
                    let text: String = self.input.get(at..at + len).unwrap_or_default().iter().collect();
                    if text.matches('.').count() > 1 || text.starts_with('.') || text.ends_with('.') {
                        continue;
                    }
                    let places = text.split_once('.').map_or(0, |(_, frac)| frac.len());
                    if places > *digits {
                        continue;
                    }
                    let Ok(n) = text.parse::<f64>() else { continue };
                    if !n.is_finite() || n < *min || n > *max {
                        continue;
                    }
                    if *step > 0.0 {
                        let k = (n - min) / step;
                        if (k - k.round()).abs() > 1e-9 {
                            continue;
                        }
                    }
                    out.push((len, Json::from(n)));
                }
            }
            FieldKind::Text { min, max, extra } => {
                let ok = |c: &char| c.is_ascii_alphanumeric() || extra.contains(*c);
                let run = self.input.get(at..).unwrap_or_default().iter().take(*max).take_while(|c| ok(c)).count();
                for len in (*min..=run).rev() {
                    let text: String = self.input.get(at..at + len).unwrap_or_default().iter().collect();
                    out.push((len, Json::String(text)));
                }
            }
            FieldKind::Rest => {
                let left = self.input.get(at..).unwrap_or_default();
                if !left.is_empty() && left.len() <= 64 {
                    out.push((left.len(), Json::String(left.iter().collect())));
                }
            }
            FieldKind::Set(options) => out.push(self.options(options, at)),
        }
        out
    }

    /// Read options in any order, each once, the longest code first; stops at the first thing that is none.
    fn options(&self, options: &[SetOption], at: usize) -> (usize, Json) {
        let mut order: Vec<&SetOption> = options.iter().collect();
        order.sort_by_key(|o| std::cmp::Reverse(o.code.chars().count()));
        let mut found = serde_json::Map::new();
        let mut p = at;
        'more: loop {
            for o in &order {
                if found.contains_key(&o.key) {
                    continue;
                }
                let code: Vec<(char, bool)> = o.code.chars().map(|c| (c, false)).collect();
                let Some(after) = self.literal(p, &code) else { continue };
                match &o.arg {
                    None => {
                        found.insert(o.key.clone(), Json::Bool(true));
                        p = after;
                        continue 'more;
                    }
                    Some(kind) => {
                        if let Some((len, value)) = self.candidates(kind, after).into_iter().next() {
                            found.insert(o.key.clone(), value);
                            p = after + len;
                            continue 'more;
                        }
                    }
                }
            }
            break;
        }
        (p - at, Json::Object(found))
    }
}

fn bad_field<T>(field: &str, message: impl Into<String>) -> Result<T, TemplateError> {
    Err(TemplateError::Field { field: field.to_string(), message: message.into() })
}

/// A parameter's text: a string, or a number written plainly.
fn text_of(v: &Json) -> Option<String> {
    match v {
        Json::String(s) => Some(s.clone()),
        Json::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn present(nodes: &[Node], params: &Params, declared_default: &dyn Fn(&str) -> Option<Json>) -> bool {
    nodes.iter().any(|n| match n {
        Node::Lit(_) => false,
        Node::Field { name, .. } => params.get(name).is_some_and(|v| declared_default(name).as_ref() != Some(v)),
        Node::Optional { nodes, flag, .. } => {
            flag.as_ref().is_some_and(|f| params.get(f) == Some(&Json::Bool(true))) || present(nodes, params, declared_default)
        }
    })
}

fn print_nodes(nodes: &[Node], params: &Params, out: &mut String) -> Result<(), TemplateError> {
    for n in nodes {
        match n {
            Node::Lit(text) => out.extend(text.iter().map(|(c, _)| c)),
            Node::Field { name, kind } => {
                let Some(v) = params.get(name) else {
                    if matches!(kind, FieldKind::Set(_)) {
                        continue;
                    }
                    return bad_field(name, "has no value");
                };
                out.push_str(&print_field(name, kind, v)?);
            }
            Node::Optional { nodes, flag, absent } => {
                let default_of = |n: &str| absent.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
                let there = match flag {
                    Some(f) => params.get(f) == Some(&Json::Bool(true)),
                    None => present(nodes, params, &default_of),
                };
                if there {
                    print_nodes(nodes, params, out)?;
                }
            }
        }
    }
    Ok(())
}

fn print_field(name: &str, kind: &FieldKind, v: &Json) -> Result<String, TemplateError> {
    match kind {
        FieldKind::Choice(values) => {
            let Some(t) = text_of(v) else { return bad_field(name, "must be text") };
            match values.iter().find(|c| c.eq_ignore_ascii_case(&t)) {
                Some(c) => Ok(c.clone()),
                None => bad_field(name, format!("`{t}` is not one of {}", values.join(", "))),
            }
        }
        FieldKind::Map(pairs) => {
            let Some(t) = text_of(v) else { return bad_field(name, "must be text") };
            match pairs.iter().find(|(_, val)| val.eq_ignore_ascii_case(&t)) {
                Some((code, _)) => Ok(code.clone()),
                None => bad_field(name, format!("`{t}` is not one of {}", pairs.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>().join(", "))),
            }
        }
        FieldKind::Int { min, max, step } => {
            let n = match v.as_i64() {
                Some(n) => n,
                None => match v.as_f64().filter(|x| x.is_finite() && x.fract() == 0.0 && x.abs() <= 1e12) {
                    Some(x) => x as i64,
                    None => return bad_field(name, "must be a whole number"),
                },
            };
            if n < *min || n > *max {
                return bad_field(name, format!("{n} is outside {min}..{max}"));
            }
            if (n - min).checked_rem(*step) != Some(0) {
                return bad_field(name, format!("{n} is not on the step of {step} from {min}"));
            }
            Ok(n.to_string())
        }
        FieldKind::Decimal { min, max, step, digits } => {
            let Some(x) = v.as_f64().filter(|x| x.is_finite()) else { return bad_field(name, "must be a number") };
            if x < *min || x > *max {
                return bad_field(name, format!("{x} is outside {min}..{max}"));
            }
            if *step > 0.0 {
                let k = (x - min) / step;
                if (k - k.round()).abs() > 1e-9 {
                    return bad_field(name, format!("{x} is not on the step of {step} from {min}"));
                }
            }
            let mut s = format!("{:.*}", *digits, x);
            if s.contains('.') {
                s = s.trim_end_matches('0').trim_end_matches('.').to_string();
            }
            Ok(s)
        }
        FieldKind::Rest => match v.as_str() {
            Some(t) if !t.is_empty() && t.chars().count() <= 64 && !t.chars().any(char::is_control) => Ok(t.to_string()),
            _ => bad_field(name, "must be 1 to 64 characters of text"),
        },
        FieldKind::Set(options) => {
            let Some(map) = v.as_object() else { return bad_field(name, "must be an object of options") };
            if let Some(k) = map.keys().find(|k| !options.iter().any(|o| &o.key == *k)) {
                return bad_field(name, format!("`{k}` is not one of the options"));
            }
            let mut out = String::new();
            for o in options {
                let Some(given) = map.get(&o.key) else { continue };
                match (&o.arg, given) {
                    (None, Json::Bool(true)) => out.push_str(&o.code),
                    (None, Json::Bool(false)) => {}
                    (None, _) => return bad_field(name, format!("`{}` takes no value", o.key)),
                    (Some(kind), value) => {
                        out.push_str(&o.code);
                        out.push_str(&print_field(&o.key, kind, value)?);
                    }
                }
            }
            Ok(out)
        }
        FieldKind::Text { min, max, extra } => {
            let Some(t) = text_of(v) else { return bad_field(name, "must be text") };
            let len = t.chars().count();
            if len < *min || len > *max {
                return bad_field(name, format!("must be {min} to {max} characters"));
            }
            if !t.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c)) {
                return bad_field(name, "has a character this field does not accept");
            }
            Ok(t)
        }
    }
}
