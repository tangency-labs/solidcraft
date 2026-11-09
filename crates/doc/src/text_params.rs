//! Text parameters: named texts (`Label` = `TAG-1`) that the words of a sketch text can be built
//! from, by an expression that joins them and quoted texts with `+` (`Label + '-L'`). They are
//! kept apart from the number parameters: a text has no unit and no number expression reads it.
//! A parameter edit or a configuration row that sets one relabels every text built from it, and
//! the features that use those texts follow in the same rebuild.
//!
//! The words can show a number parameter's value too (#9281): `'Width: ' + Width` reads
//! `Width: 85 mm`, a length in the design's units, an angle in degrees (`30°`), up to three
//! decimals without trailing zeros. `number(Width)` is the number alone (`85`) and
//! `number(Width, 2)` gives it two decimals (`85.00`).

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use crate::expr::Value;
use crate::{DocError, Document, FeatureKind, MAX_PARAMS, Result};

/// The unit that marks a text parameter where parameters are listed, added or imported.
pub const TEXT_UNIT: &str = "text";

/// Longest text a text parameter holds, and longest words expression.
const MAX_LEN: usize = 4096;

/// A named text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextParam {
    pub name: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

/// Is `unit` the text unit, ignoring ASCII case and surrounding whitespace?
pub fn is_text_unit(unit: &str) -> bool {
    unit.trim().eq_ignore_ascii_case(TEXT_UNIT)
}

/// A text as typed for a text parameter: inside matching quotes, what they hold (a quote doubled
/// inside stands for one); else the text itself, without surrounding spaces.
pub fn unquote(s: &str) -> String {
    let t = s.trim();
    for q in ['\'', '"'] {
        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
            let inner = t.get(1..t.len() - 1).unwrap_or_default();
            let doubled = format!("{q}{q}");
            // Only a text whose inner quotes all come in pairs is one quoted text.
            if inner.replace(&doubled, "").contains(q) {
                break;
            }
            return inner.replace(&doubled, &q.to_string());
        }
    }
    t.to_string()
}

enum Term {
    Text(String),
    Name(String),
    /// `number(name)` or `number(name, decimals)`, with the name's byte range.
    Number {
        name: String,
        at: (usize, usize),
        decimals: Option<u8>,
    },
}

/// How the words show a number parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Show {
    /// As named: its value and unit, up to three decimals.
    Named,
    /// In `number(...)`: the number alone, with these decimals or up to three.
    Number(Option<u8>),
}

/// The function that shows a number parameter's value without its unit.
const NUMBER: &str = "number";

/// Most decimals `number(name, decimals)` takes.
const MAX_DECIMALS: u8 = 8;

/// The terms of a words expression with their byte ranges: quoted texts (single or double
/// quotes, a quote doubled inside stands for one) and parameter names, joined by `+`.
fn terms(s: &str) -> Result<Vec<(Term, usize, usize)>> {
    if s.len() > MAX_LEN {
        return Err(DocError::Expr("text expression too long".into()));
    }
    let cs: Vec<(usize, char)> = s.char_indices().collect();
    let at = |i: usize| cs.get(i).map_or(s.len(), |x| x.0);
    let mut out = Vec::new();
    let mut i = 0;
    let mut want_term = true;
    while let Some(&(_, c)) = cs.get(i) {
        if c.is_whitespace() {
            i += 1;
        } else if !want_term {
            if c != '+' {
                return Err(DocError::Expr(format!("unexpected `{c}`: a text expression joins quoted texts and text parameters with +")));
            }
            want_term = true;
            i += 1;
        } else if c == '\'' || c == '"' {
            let start = i;
            let mut text = String::new();
            i += 1;
            loop {
                match cs.get(i).map(|x| x.1) {
                    None => return Err(DocError::Expr(format!("a text in quotes ({c}) is not closed"))),
                    Some(d) if d == c && cs.get(i + 1).is_some_and(|x| x.1 == c) => {
                        text.push(c);
                        i += 2;
                    }
                    Some(d) if d == c => {
                        i += 1;
                        break;
                    }
                    Some(d) => {
                        text.push(d);
                        i += 1;
                    }
                }
            }
            out.push((Term::Text(text), at(start), at(i)));
            want_term = false;
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            i = word_end(&cs, i);
            let word = s.get(at(start)..at(i)).unwrap_or_default().to_string();
            let open = skip_blanks(&cs, i);
            if word == NUMBER && cs.get(open).is_some_and(|x| x.1 == '(') {
                let (term, end) = number_call(s, &cs, open + 1)?;
                i = end;
                out.push((term, at(start), at(i)));
            } else {
                out.push((Term::Name(word), at(start), at(i)));
            }
            want_term = false;
        } else {
            return Err(DocError::Expr(format!("unexpected `{c}`: a text expression joins quoted texts and text parameters with +")));
        }
    }
    if want_term {
        let what = if out.is_empty() { "the text expression is empty" } else { "the text expression ends with +" };
        return Err(DocError::Expr(what.into()));
    }
    Ok(out)
}

fn word_end(cs: &[(usize, char)], mut i: usize) -> usize {
    while cs.get(i).is_some_and(|x| x.1.is_alphanumeric() || x.1 == '_') {
        i += 1;
    }
    i
}

fn skip_blanks(cs: &[(usize, char)], mut i: usize) -> usize {
    while cs.get(i).is_some_and(|x| x.1.is_whitespace()) {
        i += 1;
    }
    i
}

/// `number(` has been read up to `i`: the name, optional decimals and `)`; the term and where
/// it ends.
fn number_call(s: &str, cs: &[(usize, char)], i: usize) -> Result<(Term, usize)> {
    let at = |i: usize| cs.get(i).map_or(s.len(), |x| x.0);
    let usage = || DocError::Expr("number() takes a number parameter and, if wanted, its decimals: number(Width, 2)".into());
    let start = skip_blanks(cs, i);
    if !cs.get(start).is_some_and(|x| x.1.is_alphabetic() || x.1 == '_') {
        return Err(usage());
    }
    let end = word_end(cs, start);
    let name = s.get(at(start)..at(end)).unwrap_or_default().to_string();
    let mut i = skip_blanks(cs, end);
    let mut decimals = None;
    if cs.get(i).is_some_and(|x| x.1 == ',') {
        let from = skip_blanks(cs, i + 1);
        let mut to = from;
        while cs.get(to).is_some_and(|x| x.1.is_ascii_digit()) {
            to += 1;
        }
        let digits = s.get(at(from)..at(to)).unwrap_or_default();
        let n: u8 = digits.parse().map_err(|_| usage())?;
        if n > MAX_DECIMALS {
            return Err(DocError::Expr(format!("number() shows at most {MAX_DECIMALS} decimals")));
        }
        decimals = Some(n);
        i = skip_blanks(cs, to);
    }
    if !cs.get(i).is_some_and(|x| x.1 == ')') {
        return Err(usage());
    }
    Ok((Term::Number { name, at: (at(start), at(end)), decimals }, i + 1))
}

/// A number parameter's value as the words show it: `v` in millimetres, radians or no unit
/// (`len`, `ang`: its dimension), lengths in `units`.
pub fn show_number(v: f64, len: i8, ang: i8, units: &str, show: Show) -> Result<String> {
    let (value, unit) = match (len, ang) {
        (0, 0) => (v, ""),
        (1, 0) => {
            let (scale, name) = crate::display_units::unit_scale(units);
            (v * scale, name)
        }
        (0, 1) => (v.to_degrees(), "°"),
        _ => return Err(DocError::Expr("a text shows lengths, angles and plain numbers only".into())),
    };
    if !value.is_finite() || value.abs() > 1e12 {
        return Err(DocError::Expr("the number is out of range for a text".into()));
    }
    let digits = match show {
        Show::Number(Some(d)) => format!("{value:.*}", usize::from(d)),
        Show::Named | Show::Number(None) => {
            let t = format!("{value:.3}");
            let t = t.trim_end_matches('0').trim_end_matches('.');
            if t == "-0" { "0".to_string() } else { t.to_string() }
        }
    };
    Ok(match (show, unit) {
        (Show::Number(_), _) | (_, "") => digits,
        (Show::Named, "°") => format!("{digits}°"),
        (Show::Named, unit) => format!("{digits} {unit}"),
    })
}

/// The parameters a words expression names (none when it does not parse).
pub fn references(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (t, _, _) in terms(s).unwrap_or_default() {
        if let Term::Name(n) | Term::Number { name: n, .. } = t
            && !out.contains(&n)
        {
            out.push(n);
        }
    }
    out
}

/// The words expression with every reference to `old` renamed to `new` (quoted texts kept).
pub fn rename_reference(s: &str, old: &str, new: &str) -> String {
    let Ok(terms) = terms(s) else { return s.to_string() };
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (t, a, b) in terms {
        let at = match &t {
            Term::Name(n) if n == old => (a, b),
            Term::Number { name, at, .. } if name == old => *at,
            _ => continue,
        };
        out.push_str(s.get(last..at.0).unwrap_or_default());
        out.push_str(new);
        last = at.1;
    }
    out.push_str(s.get(last..).unwrap_or_default());
    out
}

/// The words an expression comes to, each parameter's text from `lookup`.
pub fn eval(s: &str, lookup: &dyn Fn(&str, Show) -> Result<String>) -> Result<String> {
    let mut out = String::new();
    for (t, _, _) in terms(s)? {
        match t {
            Term::Text(text) => out.push_str(&text),
            Term::Name(n) => out.push_str(&lookup(&n, Show::Named)?),
            Term::Number { name, decimals, .. } => out.push_str(&lookup(&name, Show::Number(decimals))?),
        }
        if out.len() > MAX_LEN {
            return Err(DocError::Expr("the text is too long".into()));
        }
    }
    Ok(out)
}

impl Document {
    pub fn text_param(&self, name: &str) -> Option<&TextParam> {
        self.text_params.iter().find(|p| p.name == name)
    }

    /// The words a words expression comes to in this design, number parameters from `vals`.
    pub fn eval_words(&self, e: &str, vals: &BTreeMap<String, Value>) -> Result<String> {
        eval(e, &|n, show| match (self.text_param(n), vals.get(n)) {
            (Some(_), _) if show != Show::Named => Err(DocError::Expr(format!("`{n}` is a text parameter: number() takes a number parameter"))),
            (Some(p), _) => Ok(p.text.clone()),
            (None, Some(v)) => show_number(v.v, v.len, v.ang, &self.units, show).map_err(|error| DocError::Expr(format!("`{n}`: {error}"))),
            (None, None) if self.name_taken(n) => Err(DocError::Expr(format!("`{n}` has no value"))),
            (None, None) => Err(DocError::Expr(format!("unknown parameter `{n}`"))),
        })
    }

    /// Set text parameter `name` to `text` (as typed: [`unquote`]), adding it when new.
    pub fn set_text_param(&mut self, name: &str, text: &str, comment: Option<&str>) -> Result<()> {
        if let Some(p) = Self::param_name_problem(name) {
            return Err(DocError::Invalid(p));
        }
        if text.len() > MAX_LEN {
            return Err(DocError::Expr("text too long".into()));
        }
        if comment.is_some_and(|c| c.len() > 4096) {
            return Err(DocError::Invalid("comment too long".into()));
        }
        let text = unquote(text);
        if let Some(p) = self.text_params.iter_mut().find(|p| p.name == name) {
            p.text = text;
            if let Some(c) = comment {
                p.comment = c.to_string();
            }
            return Ok(());
        }
        if self.name_taken(name) {
            return Err(DocError::Invalid(format!("`{name}` is a number parameter")));
        }
        if self.params.len() + self.text_params.len() >= MAX_PARAMS {
            return Err(DocError::Invalid("too many parameters".into()));
        }
        self.text_params.push(TextParam { name: name.to_string(), text, comment: comment.unwrap_or_default().to_string() });
        Ok(())
    }

    /// Remove text parameter `name`; refused while a text uses it.
    pub(crate) fn remove_text_param(&mut self, name: &str) -> Result<()> {
        let users = self.param_users(name);
        if !users.is_empty() {
            let list: Vec<String> = users.iter().map(crate::ParamUser::describe).collect();
            return Err(DocError::Invalid(format!("`{name}` is used by {}", list.join(", "))));
        }
        self.text_params.retain(|p| p.name != name);
        self.favorites.remove(name);
        Ok(())
    }

    /// The sketches whose texts' words read one of `names`.
    pub(crate) fn features_reading_texts(&self, names: &std::collections::BTreeSet<String>) -> Vec<u64> {
        self.features
            .iter()
            .filter(|f| match &f.kind {
                FeatureKind::Sketch { sketch, .. } => sketch.text_word_exprs().iter().any(|e| references(e).iter().any(|r| names.contains(r))),
                _ => false,
            })
            .map(|f| f.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_expressions_join_quoted_texts_and_parameters() {
        let look = |n: &str, _: Show| match n {
            "Label" => Ok("TAG-1".to_string()),
            _ => Err(DocError::Expr(format!("unknown text parameter `{n}`"))),
        };
        assert_eq!(eval("Label + '-L'", &look).unwrap(), "TAG-1-L");
        assert_eq!(eval("\"it's \" + Label", &look).unwrap(), "it's TAG-1");
        assert_eq!(eval("'it''s'", &look).unwrap(), "it's");
        assert_eq!(eval("'a+b' + 'c'", &look).unwrap(), "a+bc");
        for bad in ["", "Label +", "+ 'a'", "Label 'a'", "'open", "Label - 'a'", "Other"] {
            assert!(eval(bad, &look).is_err(), "{bad}");
        }
        assert_eq!(references("Label + 'Label' + Other + Label"), ["Label", "Other"]);
        assert_eq!(rename_reference("Label+'Label-' + Label", "Label", "Tag"), "Tag+'Label-' + Tag");
        assert_eq!(unquote(" 'A B' "), "A B");
        assert_eq!(unquote("\"it''s\""), "it''s");
        assert_eq!(unquote("'it''s'"), "it's");
        assert_eq!(unquote("'a' + 'b'"), "'a' + 'b'");
        assert_eq!(unquote("TAG-1"), "TAG-1");
    }

    /// #9281: a number parameter in the words shows its value; `number()` drops the unit and
    /// can fix the decimals; a rename reaches inside `number()`.
    #[test]
    fn number_parameters_show_their_value_in_the_words() {
        let look = |n: &str, show: Show| match n {
            "Width" => show_number(85.0, 1, 0, "mm", show),
            "Gap" => show_number(12.7, 1, 0, "in", show),
            "Tilt" => show_number(30f64.to_radians(), 0, 1, "mm", show),
            "Count" => show_number(4.0, 0, 0, "mm", show),
            "Area" => show_number(1.0, 2, 0, "mm", show),
            _ => Err(DocError::Expr(format!("unknown parameter `{n}`"))),
        };
        assert_eq!(eval("'Width: ' + Width", &look).unwrap(), "Width: 85 mm");
        assert_eq!(eval("Gap", &look).unwrap(), "0.5 in");
        assert_eq!(eval("Tilt + ' / ' + Count", &look).unwrap(), "30° / 4");
        assert_eq!(eval("number(Width) + 'x' + number( Width , 2 )", &look).unwrap(), "85x85.00");
        assert_eq!(eval("number(Tilt, 1)", &look).unwrap(), "30.0");
        for bad in ["number(Width", "number()", "number(Width, x)", "number(Width, 9)", "number(Width 2)", "Area", "number('a')"] {
            assert!(eval(bad, &look).is_err(), "{bad}");
        }
        assert_eq!(references("number(Width, 1) + Count"), ["Width", "Count"]);
        assert_eq!(rename_reference("number(Width, 1) + Width + 'Width'", "Width", "W"), "number(W, 1) + W + 'Width'");
        assert_eq!(show_number(-0.0001, 1, 0, "mm", Show::Named).unwrap(), "0 mm");
        assert!(show_number(f64::NAN, 0, 0, "mm", Show::Named).is_err());
    }
}
