//! The canonical layout. A file is canonical exactly when formatting leaves it unchanged.

use crate::diagnostic::{Code, Diagnostic};
use crate::model::{Arg, Child, Document, Entry, Expr, ExprKind, Item, Note, Spanned, lower};
use crate::parse::parse;

/// The file in canonical layout, or its syntax errors.
pub fn format(text: &str) -> Result<String, Vec<Diagnostic>> {
    let parsed = parse(text);
    if !parsed.errors.is_empty() {
        return Err(parsed.errors);
    }
    Ok(document(&lower(&parsed.syntax())))
}

/// The problem with a file that parses but isn't canonical, at the first line that differs.
pub fn layout(text: &str, doc: &Document) -> Option<Diagnostic> {
    let canonical = document(doc);
    if canonical == text {
        return None;
    }
    let differ = text.bytes().zip(canonical.bytes()).position(|(a, b)| a != b).unwrap_or_else(|| text.len().min(canonical.len()));
    let start = text.get(..differ).and_then(|t| t.rfind('\n')).map_or(0, |i| i + 1);
    let end = text.get(start..).and_then(|t| t.find('\n')).map_or(text.len(), |i| start + i);
    let range = rowan::TextRange::new(offset(start), offset(end));
    Some(Diagnostic::new(Code::NotCanonical, range, "not in canonical layout from this line; run `fmt`"))
}

fn offset(i: usize) -> rowan::TextSize {
    rowan::TextSize::from(u32::try_from(i).unwrap_or(u32::MAX))
}

fn document(doc: &Document) -> String {
    let mut out = format!("esc {}\n{}\n", doc.version.text, doc.kind.text);
    for e in &doc.entries {
        out.push('\n');
        out.push_str(&entry(e));
    }
    out
}

/// An entry's whole block in canonical layout, ending in a newline.
pub fn entry(e: &Entry) -> String {
    let mut out = format!("{} = {}\n", e.name.text, expr(&e.value));
    for child in &e.children {
        match child {
            Child::Note(n) => out.push_str(&note(n, 1)),
            Child::Item(i) => out.push_str(&item(i)),
        }
    }
    out
}

fn item(i: &Item) -> String {
    let mut out = format!("  {} = {}\n", i.name.text, expr(&i.value));
    for n in &i.notes {
        out.push_str(&note(n, 2));
    }
    out
}

fn note(n: &Note, depth: usize) -> String {
    format!("{}{}: {}\n", "  ".repeat(depth), n.kind.text, n.text.text)
}

fn expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Number { text, unit: Some(unit) } => format!("{text} {unit}"),
        ExprKind::Number { text, unit: None } => text.clone(),
        ExprKind::Str(raw) => raw.clone(),
        ExprKind::Path(segs) => dotted(segs),
        ExprKind::Call { name, args: a } => format!("{}({})", dotted(name), args(a)),
        ExprKind::List(items) => format!("[{}]", items.iter().map(expr).collect::<Vec<_>>().join(", ")),
        ExprKind::Seed(a) => format!("~({})", args(a)),
        ExprKind::Paren(inner) => format!("({})", expr(inner)),
        ExprKind::Unary { op, expr: inner } => format!("{op}{}", expr(inner)),
        ExprKind::Binary { op, lhs, rhs } => format!("{} {op} {}", expr(lhs), expr(rhs)),
        ExprKind::Missing => String::new(),
    }
}

fn dotted(segs: &[Spanned]) -> String {
    segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(".")
}

fn args(args: &[Arg]) -> String {
    args.iter()
        .map(|a| match &a.name {
            Some(name) => format!("{}: {}", name.text, expr(&a.value)),
            None => expr(&a.value),
        })
        .collect::<Vec<_>>()
        .join(", ")
}
