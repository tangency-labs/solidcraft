//! The ordering-code grammar (docs/formats/generator-package.md, section 2): compiled from the
//! package's token objects, matched by hand-written code in bounded steps. No regex engine.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use super::expr::Value;
use super::{GRAMMAR_BUDGET, GeneratorError, MAX_CODE_CHARS};

/// What a parse produced: a value per grammar field.
pub type Fields = BTreeMap<String, Value>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CharClass {
    Digits,
    Alpha,
    Alnum,
}

#[derive(Clone, Debug)]
pub enum Tok {
    Lit { text: Vec<char>, nocase: bool },
    Enum { field: String, values: Vec<(Vec<char>, String)>, nocase: bool },
    Int { field: String, min: i64, max: i64, step: i64 },
    Decimal { field: String, min: f64, max: f64, step: f64, digits: usize },
    Chars { field: String, class: CharClass, min_len: usize, max_len: usize },
    Optional { toks: Vec<Tok>, default: Fields },
    Flags { field: String, options: Vec<(Vec<char>, String)>, max: usize },
}

fn bad<T>(msg: impl Into<String>) -> Result<T, GeneratorError> {
    Err(GeneratorError::Package(format!("grammar: {}", msg.into())))
}

fn field_name(obj: &serde_json::Map<String, Json>) -> Result<String, GeneratorError> {
    match obj.get("field").and_then(Json::as_str) {
        Some(f) if super::is_name(f) => Ok(f.to_string()),
        _ => bad("a token needs a `field` that is a plain name"),
    }
}

fn known(obj: &serde_json::Map<String, Json>, allowed: &[&str]) -> Result<(), GeneratorError> {
    match obj.keys().find(|k| !allowed.contains(&k.as_str()) && !k.starts_with("x-")) {
        Some(k) => bad(format!("unknown key `{k}` in a token")),
        None => Ok(()),
    }
}

fn int_of(v: Option<&Json>, what: &str) -> Result<i64, GeneratorError> {
    match v.and_then(Json::as_i64) {
        Some(n) if n.abs() <= 1_000_000_000_000 => Ok(n),
        _ => bad(format!("`{what}` must be a whole number")),
    }
}

fn float_of(v: Option<&Json>, what: &str) -> Result<f64, GeneratorError> {
    match v.and_then(Json::as_f64) {
        Some(n) if n.is_finite() => Ok(n),
        _ => bad(format!("`{what}` must be a number")),
    }
}

/// Compile one token. `depth` counts nested optionals.
pub fn compile_token(raw: &Json, depth: usize) -> Result<Tok, GeneratorError> {
    let Some(obj) = raw.as_object() else { return bad("a token must be an object") };
    if let Some(text) = obj.get("lit") {
        known(obj, &["lit", "nocase"])?;
        let Some(t) = text.as_str().filter(|t| !t.is_empty() && t.chars().count() <= 64) else { return bad("`lit` must be 1 to 64 characters") };
        return Ok(Tok::Lit { text: t.chars().collect(), nocase: obj.get("nocase").and_then(Json::as_bool).unwrap_or(false) });
    }
    if let Some(list) = obj.get("enum") {
        known(obj, &["field", "enum", "labels", "nocase"])?;
        let field = field_name(obj)?;
        let Some(items) = list.as_array().filter(|a| !a.is_empty() && a.len() <= 256) else { return bad("`enum` must list 1 to 256 values") };
        let mut values = Vec::new();
        for item in items {
            let Some(s) = item.as_str().filter(|s| !s.is_empty() && s.chars().count() <= 64) else {
                return bad("an `enum` value must be 1 to 64 characters");
            };
            values.push((s.chars().collect::<Vec<_>>(), s.to_string()));
        }
        // The longest value that fits is tried first.
        values.sort_by_key(|(chars, _)| std::cmp::Reverse(chars.len()));
        return Ok(Tok::Enum { field, values, nocase: obj.get("nocase").and_then(Json::as_bool).unwrap_or(false) });
    }
    if let Some(r) = obj.get("int") {
        known(obj, &["field", "int"])?;
        let field = field_name(obj)?;
        let Some(r) = r.as_object() else { return bad("`int` must be an object with min and max") };
        let (min, max) = (int_of(r.get("min"), "min")?, int_of(r.get("max"), "max")?);
        let step = match r.get("step") {
            None => 1,
            s => int_of(s, "step")?,
        };
        if min > max || step < 1 {
            return bad("`int` needs min <= max and a step of 1 or more");
        }
        return Ok(Tok::Int { field, min, max, step });
    }
    if let Some(r) = obj.get("decimal") {
        known(obj, &["field", "decimal"])?;
        let field = field_name(obj)?;
        let Some(r) = r.as_object() else { return bad("`decimal` must be an object with min and max") };
        let (min, max) = (float_of(r.get("min"), "min")?, float_of(r.get("max"), "max")?);
        let step = match r.get("step") {
            None => 0.0,
            s => float_of(s, "step")?,
        };
        let digits = match r.get("digits") {
            None => 3,
            d => usize::try_from(int_of(d, "digits")?).unwrap_or(99),
        };
        if min > max || step < 0.0 || digits > 9 {
            return bad("`decimal` needs min <= max, a step of 0 or more and at most 9 digits");
        }
        return Ok(Tok::Decimal { field, min, max, step, digits });
    }
    if let Some(c) = obj.get("chars") {
        known(obj, &["field", "chars", "min_len", "max_len"])?;
        let field = field_name(obj)?;
        let class = match c.as_str() {
            Some("digits") => CharClass::Digits,
            Some("alpha") => CharClass::Alpha,
            Some("alnum") => CharClass::Alnum,
            _ => return bad("`chars` must be digits, alpha or alnum"),
        };
        let min_len = usize::try_from(int_of(obj.get("min_len").or(Some(&Json::from(1))), "min_len")?).unwrap_or(0);
        let max_len = usize::try_from(int_of(obj.get("max_len").or(Some(&Json::from(MAX_CODE_CHARS))), "max_len")?).unwrap_or(0);
        if min_len == 0 || min_len > max_len || max_len > MAX_CODE_CHARS {
            return bad("`chars` needs 1 <= min_len <= max_len <= 128");
        }
        return Ok(Tok::Chars { field, class, min_len, max_len });
    }
    if let Some(inner) = obj.get("optional") {
        known(obj, &["optional", "default"])?;
        if depth >= 4 {
            return bad("optional groups nest at most 4 deep");
        }
        let Some(items) = inner.as_array().filter(|a| !a.is_empty() && a.len() <= 32) else { return bad("`optional` must list 1 to 32 tokens") };
        let toks = items.iter().map(|t| compile_token(t, depth + 1)).collect::<Result<Vec<_>, _>>()?;
        let mut default = Fields::new();
        if let Some(d) = obj.get("default") {
            let Some(d) = d.as_object() else { return bad("`default` must be an object of field values") };
            for (k, v) in d {
                let value = match v {
                    Json::String(s) if s.chars().count() <= 64 => Value::Text(s.clone()),
                    Json::Number(n) => match n.as_f64().filter(|x| x.is_finite()) {
                        Some(x) => Value::Num(x),
                        None => return bad("a default must be finite"),
                    },
                    _ => return bad("a default must be text or a number"),
                };
                default.insert(k.clone(), value);
            }
        }
        return Ok(Tok::Optional { toks, default });
    }
    if let Some(f) = obj.get("flags") {
        known(obj, &["flags"])?;
        let Some(f) = f.as_object() else { return bad("`flags` must be an object") };
        let field = field_name(f)?;
        let Some(options) = f.get("options").and_then(Json::as_array).filter(|a| !a.is_empty() && a.len() <= 32) else {
            return bad("`flags.options` must list 1 to 32 options");
        };
        let mut out = Vec::new();
        for o in options {
            let code = o.get("code").and_then(Json::as_str).filter(|c| !c.is_empty() && c.chars().count() <= 16);
            let value = o.get("value").and_then(Json::as_str).filter(|v| v.chars().count() <= 64);
            let (Some(code), Some(value)) = (code, value) else { return bad("a flag option needs a `code` and a `value`") };
            out.push((code.chars().collect::<Vec<_>>(), value.to_string()));
        }
        out.sort_by_key(|(chars, _)| std::cmp::Reverse(chars.len()));
        let max = match f.get("max") {
            None => out.len(),
            m => usize::try_from(int_of(m, "max")?).unwrap_or(0),
        };
        return Ok(Tok::Flags { field, options: out, max });
    }
    bad("a token needs one of lit, enum, int, decimal, chars, optional or flags")
}

fn starts(input: &[char], pos: usize, text: &[char], nocase: bool) -> bool {
    let Some(slice) = input.get(pos..pos + text.len()) else { return false };
    slice.iter().zip(text).all(|(a, b)| if nocase { a.eq_ignore_ascii_case(b) } else { a == b })
}

struct Matcher<'a> {
    input: &'a [char],
    budget: u32,
}

impl Matcher<'_> {
    fn run(&mut self, toks: &[Tok], rest: &[&[Tok]], pos: usize, fields: Fields) -> Result<Option<Fields>, GeneratorError> {
        if self.budget == 0 {
            return Err(GeneratorError::Limit { what: "grammar matching steps", limit: GRAMMAR_BUDGET as usize });
        }
        self.budget -= 1;
        let Some((first, tail)) = toks.split_first() else {
            return match rest.split_first() {
                Some((next, more)) => self.run(next, more, pos, fields),
                None => Ok((pos == self.input.len()).then_some(fields)),
            };
        };
        match first {
            Tok::Lit { text, nocase } => {
                if starts(self.input, pos, text, *nocase) {
                    self.run(tail, rest, pos + text.len(), fields)
                } else {
                    Ok(None)
                }
            }
            Tok::Enum { field, values, nocase } => {
                for (chars, canonical) in values {
                    if starts(self.input, pos, chars, *nocase) {
                        let mut f = fields.clone();
                        f.insert(field.clone(), Value::Text(canonical.clone()));
                        if let Some(done) = self.run(tail, rest, pos + chars.len(), f)? {
                            return Ok(Some(done));
                        }
                    }
                }
                Ok(None)
            }
            Tok::Int { field, min, max, step } => {
                let digits = self.input.get(pos..).unwrap_or_default().iter().take(15).take_while(|c| c.is_ascii_digit()).count();
                for len in (1..=digits).rev() {
                    let text: String = self.input.get(pos..pos + len).unwrap_or_default().iter().collect();
                    if len > 1 && text.starts_with('0') {
                        continue;
                    }
                    let Ok(n) = text.parse::<i64>() else { continue };
                    if n < *min || n > *max || (n - min) % step != 0 {
                        continue;
                    }
                    let mut f = fields.clone();
                    f.insert(field.clone(), Value::Num(n as f64));
                    if let Some(done) = self.run(tail, rest, pos + len, f)? {
                        return Ok(Some(done));
                    }
                }
                Ok(None)
            }
            Tok::Decimal { field, min, max, step, digits } => {
                let run = self.input.get(pos..).unwrap_or_default().iter().take(24).take_while(|c| c.is_ascii_digit() || **c == '.').count();
                for len in (1..=run).rev() {
                    let text: String = self.input.get(pos..pos + len).unwrap_or_default().iter().collect();
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
                    let mut f = fields.clone();
                    f.insert(field.clone(), Value::Num(n));
                    if let Some(done) = self.run(tail, rest, pos + len, f)? {
                        return Ok(Some(done));
                    }
                }
                Ok(None)
            }
            Tok::Chars { field, class, min_len, max_len } => {
                let ok = |c: &char| match class {
                    CharClass::Digits => c.is_ascii_digit(),
                    CharClass::Alpha => c.is_ascii_alphabetic(),
                    CharClass::Alnum => c.is_ascii_alphanumeric(),
                };
                let run = self.input.get(pos..).unwrap_or_default().iter().take(*max_len).take_while(|c| ok(c)).count();
                for len in (*min_len..=run).rev() {
                    let text: String = self.input.get(pos..pos + len).unwrap_or_default().iter().collect();
                    let mut f = fields.clone();
                    f.insert(field.clone(), Value::Text(text));
                    if let Some(done) = self.run(tail, rest, pos + len, f)? {
                        return Ok(Some(done));
                    }
                }
                Ok(None)
            }
            Tok::Optional { toks: inner, default } => {
                // Present first, then left out with its defaults.
                let mut chain: Vec<&[Tok]> = Vec::with_capacity(rest.len() + 1);
                chain.push(tail);
                chain.extend_from_slice(rest);
                if let Some(done) = self.run(inner, &chain, pos, fields.clone())? {
                    return Ok(Some(done));
                }
                let mut f = fields;
                for (k, v) in default {
                    f.insert(k.clone(), v.clone());
                }
                self.run(tail, rest, pos, f)
            }
            Tok::Flags { field, options, max } => {
                let mut used: Vec<usize> = Vec::new();
                let mut at = pos;
                while used.len() < *max {
                    let hit = options.iter().enumerate().find(|(i, (code, _))| !used.contains(i) && starts(self.input, at, code, false));
                    let Some((i, (code, _))) = hit else { break };
                    used.push(i);
                    at += code.len();
                }
                let values = used.iter().filter_map(|i| options.get(*i)).map(|(_, v)| Value::Text(v.clone())).collect();
                let mut f = fields;
                f.insert(field.clone(), Value::List(values));
                self.run(tail, rest, at, f)
            }
        }
    }
}

/// Match `code` against each alternative in turn; the first that fits wins.
pub fn parse_code(alternatives: &[Vec<Tok>], code: &str) -> Result<Option<Fields>, GeneratorError> {
    let input: Vec<char> = code.chars().collect();
    if input.len() > MAX_CODE_CHARS {
        return Err(GeneratorError::Limit { what: "ordering code characters", limit: MAX_CODE_CHARS });
    }
    let mut m = Matcher { input: &input, budget: GRAMMAR_BUDGET };
    for alt in alternatives {
        if let Some(fields) = m.run(alt, &[], 0, Fields::new())? {
            return Ok(Some(fields));
        }
    }
    Ok(None)
}

/// The names of the fields a token list can set.
pub fn field_names(toks: &[Tok], out: &mut Vec<String>) {
    for t in toks {
        match t {
            Tok::Lit { .. } => {}
            Tok::Enum { field, .. } | Tok::Int { field, .. } | Tok::Decimal { field, .. } | Tok::Chars { field, .. } | Tok::Flags { field, .. } => {
                out.push(field.clone())
            }
            Tok::Optional { toks, default } => {
                field_names(toks, out);
                out.extend(default.keys().cloned());
            }
        }
    }
}
