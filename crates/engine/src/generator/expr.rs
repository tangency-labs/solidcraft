//! The tiny pure expression language of a generator package (docs/formats/generator-package.md,
//! section 4): numbers, text, booleans, lists; no loops, no recursion, no I/O. Every limit has a
//! typed failure and nothing here panics.

use std::collections::BTreeMap;

use super::{MAX_EXPR_CHARS, MAX_EXPR_NESTING, MAX_EXPR_NODES, MAX_TEXT_CHARS};

/// A value an expression works on.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Num(f64),
    Text(String),
    Bool(bool),
    List(Vec<Value>),
    /// A polygon from a table cell.
    Points(Vec<[f64; 2]>),
    /// A looked-up table row, read as `name.column`.
    Record(BTreeMap<String, Value>),
    /// An empty cell of a `number?` column: no value (`has(x)` is false).
    None,
}

/// Names visible to an expression.
pub type Scope = BTreeMap<String, Value>;

/// Why an expression failed.
#[derive(Clone, Debug, PartialEq)]
pub enum EvalError {
    /// A wrong type, an unknown name, a division by zero, a result that is not finite...
    Msg(String),
    /// The build's evaluation steps ran out.
    Budget,
}

/// The evaluation steps a build has left.
#[derive(Clone, Debug)]
pub struct Budget {
    pub remaining: u64,
}

fn fail<T>(msg: impl Into<String>) -> Result<T, EvalError> {
    Err(EvalError::Msg(msg.into()))
}

/// A number as text: whole numbers without a decimal point.
pub fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

impl Value {
    /// The value as text, for messages, names and `${...}` inside longer text.
    pub fn display(&self) -> String {
        match self {
            Value::Num(n) => fmt_num(*n),
            Value::Text(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::List(items) => items.iter().map(Value::display).collect::<Vec<_>>().join(", "),
            Value::Points(points) => points.iter().map(|[x, y]| format!("{} {}", fmt_num(*x), fmt_num(*y))).collect::<Vec<_>>().join("; "),
            Value::Record(_) => "(row)".into(),
            Value::None => String::new(),
        }
    }

    /// How a value is compared with a table key: numbers and text that read the same match.
    pub fn key_string(&self) -> Option<String> {
        match self {
            Value::Num(_) | Value::Text(_) | Value::Bool(_) => Some(self.display()),
            _ => None,
        }
    }

    /// The value as command-parameter JSON. A record has none.
    pub fn to_json(&self) -> Option<serde_json::Value> {
        match self {
            Value::Num(n) => Some(num_json(*n)?),
            Value::Text(s) => Some(serde_json::Value::String(s.clone())),
            Value::Bool(b) => Some(serde_json::Value::Bool(*b)),
            Value::List(items) => items.iter().map(Value::to_json).collect::<Option<Vec<_>>>().map(serde_json::Value::Array),
            Value::Points(points) => {
                let rows =
                    points.iter().map(|[x, y]| Some(serde_json::Value::Array(vec![num_json(*x)?, num_json(*y)?]))).collect::<Option<Vec<_>>>()?;
                Some(serde_json::Value::Array(rows))
            }
            Value::Record(_) => None,
            Value::None => Some(serde_json::Value::Null),
        }
    }
}

/// A finite number as JSON: whole numbers stay integers.
fn num_json(n: f64) -> Option<serde_json::Value> {
    if !n.is_finite() {
        return None;
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return Some(serde_json::Value::from(n as i64));
    }
    serde_json::Number::from_f64(n).map(serde_json::Value::Number)
}

// ---- syntax ----

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Question,
    Colon,
    End,
}

/// A parsed expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Path(Vec<String>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while chars.get(i).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                i += 1;
            }
            if chars.get(i).is_some_and(|c| *c == 'e' || *c == 'E') {
                let mut j = i + 1;
                if chars.get(j).is_some_and(|c| *c == '+' || *c == '-') {
                    j += 1;
                }
                if chars.get(j).is_some_and(char::is_ascii_digit) {
                    while chars.get(j).is_some_and(char::is_ascii_digit) {
                        j += 1;
                    }
                    i = j;
                }
            }
            let text: String = chars.get(start..i).unwrap_or_default().iter().collect();
            let n: f64 = text.parse().map_err(|_| format!("`{text}` is not a number"))?;
            if !n.is_finite() {
                return Err(format!("`{text}` is not a finite number"));
            }
            toks.push(Tok::Num(n));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut name = String::new();
            loop {
                while let Some(&ch) = chars.get(i) {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        name.push(ch);
                        i += 1;
                    } else {
                        break;
                    }
                }
                let more = chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == '_');
                if more {
                    name.push('.');
                    i += 1;
                } else {
                    break;
                }
            }
            toks.push(Tok::Ident(name));
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                match chars.get(i) {
                    None => return Err("a text value is not closed".into()),
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some('\\') => {
                        match chars.get(i + 1) {
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            _ => return Err("a text value allows only \\\" and \\\\ escapes".into()),
                        }
                        i += 2;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
                if s.chars().count() > MAX_TEXT_CHARS {
                    return Err("a text value is too long".into());
                }
            }
            toks.push(Tok::Str(s));
        } else {
            let two: String = chars.get(i..i + 2).unwrap_or_default().iter().collect();
            let op2 = ["==", "!=", "<=", ">=", "&&", "||"].into_iter().find(|o| *o == two);
            if let Some(op) = op2 {
                toks.push(Tok::Op(op));
                i += 2;
                continue;
            }
            let tok = match c {
                '+' => Tok::Op("+"),
                '-' => Tok::Op("-"),
                '*' => Tok::Op("*"),
                '/' => Tok::Op("/"),
                '%' => Tok::Op("%"),
                '<' => Tok::Op("<"),
                '>' => Tok::Op(">"),
                '!' => Tok::Op("!"),
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' => Tok::Comma,
                '?' => Tok::Question,
                ':' => Tok::Colon,
                other => return Err(format!("unexpected `{other}`")),
            };
            toks.push(tok);
            i += 1;
        }
    }
    toks.push(Tok::End);
    Ok(toks)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
    nodes: usize,
}

fn prec(op: &str) -> Option<u8> {
    match op {
        "||" => Some(1),
        "&&" => Some(2),
        "==" | "!=" => Some(3),
        "<" | "<=" | ">" | ">=" => Some(4),
        "+" | "-" => Some(5),
        "*" | "/" | "%" => Some(6),
        _ => None,
    }
}

impl Parser {
    fn peek(&self) -> &Tok {
        self.toks.get(self.pos).unwrap_or(&Tok::End)
    }

    fn bump(&mut self) -> Tok {
        let t = self.peek().clone();
        self.pos += 1;
        t
    }

    fn node(&mut self) -> Result<(), String> {
        self.nodes += 1;
        if self.nodes > MAX_EXPR_NODES { Err(format!("more than {MAX_EXPR_NODES} parts")) } else { Ok(()) }
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_EXPR_NESTING { Err(format!("nested deeper than {MAX_EXPR_NESTING}")) } else { Ok(()) }
    }

    fn cond(&mut self) -> Result<Expr, String> {
        self.enter()?;
        let c = self.bin(1)?;
        let out = if *self.peek() == Tok::Question {
            self.bump();
            let a = self.cond()?;
            if self.bump() != Tok::Colon {
                return Err("`?` needs a `:`".into());
            }
            let b = self.cond()?;
            self.node()?;
            Expr::Cond(Box::new(c), Box::new(a), Box::new(b))
        } else {
            c
        };
        self.depth -= 1;
        Ok(out)
    }

    fn bin(&mut self, min: u8) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        loop {
            let Tok::Op(op) = self.peek().clone() else { break };
            let Some(p) = prec(op) else { break };
            if p < min {
                break;
            }
            self.bump();
            let rhs = self.bin(p + 1)?;
            self.node()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        self.enter()?;
        let e = match self.peek().clone() {
            Tok::Op("-") => {
                self.bump();
                let inner = self.unary()?;
                self.node()?;
                Expr::Neg(Box::new(inner))
            }
            Tok::Op("!") => {
                self.bump();
                let inner = self.unary()?;
                self.node()?;
                Expr::Not(Box::new(inner))
            }
            _ => self.primary()?,
        };
        self.depth -= 1;
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        self.node()?;
        match self.bump() {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::LParen => {
                let e = self.cond()?;
                if self.bump() != Tok::RParen {
                    return Err("a `(` is not closed".into());
                }
                Ok(e)
            }
            Tok::Ident(name) => {
                if name == "true" {
                    return Ok(Expr::Bool(true));
                }
                if name == "false" {
                    return Ok(Expr::Bool(false));
                }
                if *self.peek() == Tok::LParen && !name.contains('.') {
                    self.bump();
                    let mut args = Vec::new();
                    if *self.peek() == Tok::RParen {
                        self.bump();
                    } else {
                        loop {
                            args.push(self.cond()?);
                            match self.bump() {
                                Tok::Comma => {}
                                Tok::RParen => break,
                                _ => return Err(format!("`{name}(` is not closed")),
                            }
                            if args.len() > 32 {
                                return Err("a call has more than 32 arguments".into());
                            }
                        }
                    }
                    return Ok(Expr::Call(name, args));
                }
                Ok(Expr::Path(name.split('.').map(str::to_string).collect()))
            }
            Tok::End => Err("the expression ends too soon".into()),
            other => Err(format!("unexpected {other:?}")),
        }
    }
}

/// Parse an expression: at most [`MAX_EXPR_CHARS`] characters, [`MAX_EXPR_NODES`] parts and
/// [`MAX_EXPR_NESTING`] levels of nesting.
pub fn parse(src: &str) -> Result<Expr, String> {
    if src.chars().count() > MAX_EXPR_CHARS {
        return Err(format!("longer than {MAX_EXPR_CHARS} characters"));
    }
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, depth: 0, nodes: 0 };
    let e = p.cond()?;
    if *p.peek() != Tok::End {
        return Err("something follows the expression".into());
    }
    Ok(e)
}

// ---- evaluation ----

fn finite(n: f64) -> Result<Value, EvalError> {
    if n.is_finite() { Ok(Value::Num(n)) } else { fail("the result is not a finite number") }
}

fn text(s: String) -> Result<Value, EvalError> {
    if s.chars().count() > MAX_TEXT_CHARS { fail(format!("text longer than {MAX_TEXT_CHARS} characters")) } else { Ok(Value::Text(s)) }
}

fn num(v: Value, what: &str) -> Result<f64, EvalError> {
    match v {
        Value::Num(n) => Ok(n),
        other => fail(format!("{what} needs a number, not `{}`", other.display())),
    }
}

fn boolean(v: Value, what: &str) -> Result<bool, EvalError> {
    match v {
        Value::Bool(b) => Ok(b),
        other => fail(format!("{what} needs true or false, not `{}`", other.display())),
    }
}

fn digits(v: Value) -> Result<i32, EvalError> {
    let d = num(v, "the digit count")?;
    if d.fract() != 0.0 || !(0.0..=12.0).contains(&d) { fail("the digit count must be a whole number from 0 to 12") } else { Ok(d as i32) }
}

fn path(scope: &Scope, p: &[String]) -> Result<Value, EvalError> {
    let Some((first, rest)) = p.split_first() else { return fail("an empty name") };
    let mut cur = scope.get(first).ok_or_else(|| EvalError::Msg(format!("unknown name `{first}`")))?;
    for seg in rest {
        match cur {
            Value::Record(r) => cur = r.get(seg).ok_or_else(|| EvalError::Msg(format!("no column `{seg}` in `{first}`")))?,
            _ => return fail(format!("`{first}` has no part `{seg}`")),
        }
    }
    Ok(cur.clone())
}

fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => x == y,
        (Value::Text(x), Value::Text(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::None, Value::None) => true,
        (Value::None, _) | (_, Value::None) => false,
        _ => match (a.key_string(), b.key_string()) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
    }
}

/// Evaluate `e` in `scope`, spending from `budget`.
pub fn eval(e: &Expr, scope: &Scope, budget: &mut Budget) -> Result<Value, EvalError> {
    if budget.remaining == 0 {
        return Err(EvalError::Budget);
    }
    budget.remaining -= 1;
    match e {
        Expr::Num(n) => Ok(Value::Num(*n)),
        Expr::Str(s) => Ok(Value::Text(s.clone())),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Path(p) => path(scope, p),
        Expr::Neg(a) => finite(-num(eval(a, scope, budget)?, "`-`")?),
        Expr::Not(a) => Ok(Value::Bool(!boolean(eval(a, scope, budget)?, "`!`")?)),
        Expr::Cond(c, a, b) => {
            if boolean(eval(c, scope, budget)?, "`?`")? {
                eval(a, scope, budget)
            } else {
                eval(b, scope, budget)
            }
        }
        Expr::Bin(op, a, b) => bin(op, a, b, scope, budget),
        Expr::Call(name, args) => call(name, args, scope, budget),
    }
}

fn bin(op: &str, a: &Expr, b: &Expr, scope: &Scope, budget: &mut Budget) -> Result<Value, EvalError> {
    if op == "&&" || op == "||" {
        let left = boolean(eval(a, scope, budget)?, &format!("`{op}`"))?;
        if (op == "&&" && !left) || (op == "||" && left) {
            return Ok(Value::Bool(left));
        }
        return Ok(Value::Bool(boolean(eval(b, scope, budget)?, &format!("`{op}`"))?));
    }
    let x = eval(a, scope, budget)?;
    let y = eval(b, scope, budget)?;
    match op {
        "==" => Ok(Value::Bool(equal(&x, &y))),
        "!=" => Ok(Value::Bool(!equal(&x, &y))),
        "+" if matches!(x, Value::Text(_)) || matches!(y, Value::Text(_)) => text(format!("{}{}", x.display(), y.display())),
        "<" | "<=" | ">" | ">=" => {
            let (l, r) = (num(x, &format!("`{op}`"))?, num(y, &format!("`{op}`"))?);
            Ok(Value::Bool(match op {
                "<" => l < r,
                "<=" => l <= r,
                ">" => l > r,
                _ => l >= r,
            }))
        }
        _ => {
            let what = format!("`{op}`");
            let (l, r) = (num(x, &what)?, num(y, &what)?);
            match op {
                "+" => finite(l + r),
                "-" => finite(l - r),
                "*" => finite(l * r),
                "/" if r == 0.0 => fail("division by zero"),
                "/" => finite(l / r),
                "%" if r == 0.0 => fail("remainder of a division by zero"),
                "%" => finite(l % r),
                _ => fail(format!("unknown operator `{op}`")),
            }
        }
    }
}

fn arity(name: &str, args: &[Expr], min: usize, max: usize) -> Result<(), EvalError> {
    if args.len() < min || args.len() > max {
        let want = if min == max { format!("{min}") } else { format!("{min} to {max}") };
        return fail(format!("`{name}` takes {want} argument(s), not {}", args.len()));
    }
    Ok(())
}

fn call(name: &str, args: &[Expr], scope: &Scope, budget: &mut Budget) -> Result<Value, EvalError> {
    // `if` evaluates only the branch it takes.
    if name == "if" {
        arity(name, args, 3, 3)?;
        let (Some(c), Some(a), Some(b)) = (args.first(), args.get(1), args.get(2)) else { return fail("`if` takes 3 arguments") };
        return if boolean(eval(c, scope, budget)?, "`if`")? { eval(a, scope, budget) } else { eval(b, scope, budget) };
    }
    let mut vals = Vec::with_capacity(args.len());
    for a in args {
        vals.push(eval(a, scope, budget)?);
    }
    let n = |i: usize| -> Result<f64, EvalError> { num(vals.get(i).cloned().unwrap_or(Value::Bool(false)), &format!("`{name}`")) };
    let deg = |x: f64| x.to_radians();
    match name {
        "min" | "max" => {
            arity(name, args, 1, 32)?;
            let mut best = n(0)?;
            for i in 1..vals.len() {
                let x = n(i)?;
                best = if name == "min" { best.min(x) } else { best.max(x) };
            }
            finite(best)
        }
        "abs" => {
            arity(name, args, 1, 1)?;
            finite(n(0)?.abs())
        }
        "floor" => {
            arity(name, args, 1, 1)?;
            finite(n(0)?.floor())
        }
        "ceil" => {
            arity(name, args, 1, 1)?;
            finite(n(0)?.ceil())
        }
        "round" => {
            arity(name, args, 1, 2)?;
            let d = match vals.get(1) {
                Some(v) => digits(v.clone())?,
                None => 0,
            };
            let scale = 10f64.powi(d);
            finite((n(0)? * scale).round() / scale)
        }
        "sqrt" => {
            arity(name, args, 1, 1)?;
            let x = n(0)?;
            if x < 0.0 { fail("`sqrt` of a negative number") } else { finite(x.sqrt()) }
        }
        "pow" => {
            arity(name, args, 2, 2)?;
            finite(n(0)?.powf(n(1)?))
        }
        "pi" => {
            arity(name, args, 0, 0)?;
            Ok(Value::Num(std::f64::consts::PI))
        }
        "sin" => {
            arity(name, args, 1, 1)?;
            finite(deg(n(0)?).sin())
        }
        "cos" => {
            arity(name, args, 1, 1)?;
            finite(deg(n(0)?).cos())
        }
        "tan" => {
            arity(name, args, 1, 1)?;
            let r = deg(n(0)?);
            if r.cos().abs() < 1e-12 { fail("`tan` of an angle with no tangent") } else { finite(r.tan()) }
        }
        "asin" | "acos" => {
            arity(name, args, 1, 1)?;
            let x = n(0)?;
            if !(-1.0..=1.0).contains(&x) {
                return fail(format!("`{name}` needs a number from -1 to 1"));
            }
            finite(if name == "asin" { x.asin() } else { x.acos() }.to_degrees())
        }
        "atan" => {
            arity(name, args, 1, 1)?;
            finite(n(0)?.atan().to_degrees())
        }
        "atan2" => {
            arity(name, args, 2, 2)?;
            finite(n(0)?.atan2(n(1)?).to_degrees())
        }
        "in" => {
            arity(name, args, 2, 2)?;
            match (vals.first(), vals.get(1)) {
                (Some(x), Some(Value::List(items))) => Ok(Value::Bool(items.iter().any(|i| equal(x, i)))),
                _ => fail("`in` needs a value and a list"),
            }
        }
        "len" => {
            arity(name, args, 1, 1)?;
            match vals.first() {
                Some(Value::List(items)) => Ok(Value::Num(items.len() as f64)),
                Some(Value::Points(points)) => Ok(Value::Num(points.len() as f64)),
                Some(Value::Text(s)) => Ok(Value::Num(s.chars().count() as f64)),
                _ => fail("`len` needs a list or text"),
            }
        }
        "at" => {
            arity(name, args, 2, 2)?;
            let i = n(1)?;
            let items = match vals.first() {
                Some(Value::List(items)) => items.clone(),
                Some(Value::Points(points)) => points.iter().map(|[x, y]| Value::List(vec![Value::Num(*x), Value::Num(*y)])).collect(),
                _ => return fail("`at` needs a list"),
            };
            if i.fract() != 0.0 || i < 0.0 {
                return fail("`at` needs a whole index of 0 or more");
            }
            items.get(i as usize).cloned().ok_or_else(|| EvalError::Msg(format!("`at` index {} is outside the list of {}", fmt_num(i), items.len())))
        }
        "join" => {
            arity(name, args, 2, 2)?;
            let (Some(Value::List(items)), Some(Value::Text(sep))) = (vals.first(), vals.get(1)) else {
                return fail("`join` needs a list and a text separator");
            };
            text(items.iter().map(Value::display).collect::<Vec<_>>().join(sep.as_str()))
        }
        "fmt" => {
            arity(name, args, 1, 2)?;
            // `fmt(x)`: whole numbers without a decimal point, others the shortest text that reads back
            // as the same number. `fmt(x, d)`: exactly `d` decimals.
            match vals.get(1) {
                None => text(fmt_num(n(0)?)),
                Some(v) => {
                    let d = digits(v.clone())?;
                    text(format!("{:.*}", d as usize, n(0)?))
                }
            }
        }
        "has" => {
            arity(name, args, 1, 1)?;
            Ok(Value::Bool(!matches!(vals.first(), Some(Value::None))))
        }
        "concat" => {
            arity(name, args, 1, 32)?;
            text(vals.iter().map(Value::display).collect::<String>())
        }
        "upper" | "lower" => {
            arity(name, args, 1, 1)?;
            let Some(Value::Text(s)) = vals.first() else { return fail(format!("`{name}` needs text")) };
            text(if name == "upper" { s.to_uppercase() } else { s.to_lowercase() })
        }
        other => fail(format!("unknown function `{other}`")),
    }
}
