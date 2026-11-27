//! The typed layer over a syntax tree without errors: entries, sketch lines, notes and values,
//! each with its range in the text. `fmt`, `check` and the edit intents all work on this.

use rowan::{NodeOrToken, TextRange};

use crate::syntax::{SyntaxKind, SyntaxNode};

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub text: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub version: Spanned,
    pub kind: Spanned,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: Spanned,
    pub value: Expr,
    pub children: Vec<Child>,
    /// The whole block: its line, the lines under it and the last newline.
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Child {
    Note(Note),
    Item(Item),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub kind: Spanned,
    /// As written, quotes and escapes included.
    pub text: Spanned,
}

/// A sketch line: an element or a constraint, with its notes.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: Spanned,
    pub value: Expr,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Number {
        text: String,
        unit: Option<String>,
    },
    /// As written, quotes and escapes included.
    Str(String),
    Path(Vec<Spanned>),
    Call {
        name: Vec<Spanned>,
        args: Vec<Arg>,
    },
    List(Vec<Expr>),
    Seed(Vec<Arg>),
    Paren(Box<Expr>),
    Unary {
        op: char,
        expr: Box<Expr>,
    },
    Binary {
        op: char,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Missing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: Option<Spanned>,
    pub value: Expr,
}

impl Entry {
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.children.iter().filter_map(|c| match c {
            Child::Item(i) => Some(i),
            Child::Note(_) => None,
        })
    }

    pub fn call_name(&self) -> Option<&str> {
        self.value.call_name()
    }

    /// The path an `import` or `import_step` entry writes as its first argument, unquoted, and
    /// where it is written.
    pub fn written_path(&self) -> Option<(String, TextRange)> {
        let arg = self.value.args().first()?;
        let ExprKind::Str(raw) = &arg.value.kind else { return None };
        Some((unquote(raw), arg.value.range))
    }
}

impl Expr {
    pub fn call_name(&self) -> Option<&str> {
        match &self.kind {
            ExprKind::Call { name, .. } if name.len() == 1 => name.first().map(|n| n.text.as_str()),
            _ => None,
        }
    }

    pub fn args(&self) -> &[Arg] {
        match &self.kind {
            ExprKind::Call { args, .. } | ExprKind::Seed(args) => args,
            _ => &[],
        }
    }

    pub fn single_name(&self) -> Option<&str> {
        match &self.kind {
            ExprKind::Path(segs) if segs.len() == 1 => segs.first().map(|s| s.text.as_str()),
            _ => None,
        }
    }
}

/// The text a string literal stands for, without quotes and escapes.
pub fn unquote(raw: &str) -> String {
    let inner = raw.strip_prefix('"').unwrap_or(raw);
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    out
}

pub fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn tokens(node: &SyntaxNode) -> impl Iterator<Item = crate::syntax::SyntaxToken> {
    node.children_with_tokens().filter_map(NodeOrToken::into_token)
}

fn first_token(node: &SyntaxNode, kind: SyntaxKind) -> Option<Spanned> {
    tokens(node).find(|t| t.kind() == kind).map(|t| Spanned { text: t.text().to_owned(), range: t.text_range() })
}

fn first_token_or_empty(node: &SyntaxNode, kind: SyntaxKind) -> Spanned {
    first_token(node, kind).unwrap_or(Spanned { text: String::new(), range: node.text_range() })
}

/// The document of a tree the parser found no errors in.
pub fn lower(root: &SyntaxNode) -> Document {
    let empty = Spanned { text: String::new(), range: TextRange::default() };
    let header = root.children().find(|n| n.kind() == SyntaxKind::Header);
    let line = |kind: SyntaxKind, token: SyntaxKind| {
        header.as_ref().and_then(|h| h.children().find(|n| n.kind() == kind)).and_then(|l| first_token(&l, token))
    };
    let version = line(SyntaxKind::VersionLine, SyntaxKind::Number).unwrap_or_else(|| empty.clone());
    let kind = line(SyntaxKind::KindLine, SyntaxKind::Ident).unwrap_or(empty);
    let entries = root.children().filter(|n| n.kind() == SyntaxKind::Entry).map(|n| entry(&n)).collect();
    Document { version, kind, entries }
}

fn missing(node: &SyntaxNode) -> Expr {
    Expr { kind: ExprKind::Missing, range: TextRange::empty(node.text_range().end()) }
}

fn value_of(node: &SyntaxNode) -> Expr {
    node.children().find(|n| is_expr(n.kind())).map_or_else(|| missing(node), |n| expr(&n))
}

fn entry(node: &SyntaxNode) -> Entry {
    let children = node
        .children()
        .filter_map(|c| match c.kind() {
            SyntaxKind::Note => Some(Child::Note(note(&c))),
            SyntaxKind::Item => Some(Child::Item(item(&c))),
            _ => None,
        })
        .collect();
    Entry { name: first_token_or_empty(node, SyntaxKind::Ident), value: value_of(node), children, range: node.text_range() }
}

fn item(node: &SyntaxNode) -> Item {
    let notes = node.children().filter(|c| c.kind() == SyntaxKind::Note).map(|c| note(&c)).collect();
    Item { name: first_token_or_empty(node, SyntaxKind::Ident), value: value_of(node), notes }
}

fn note(node: &SyntaxNode) -> Note {
    let text = match node.children().find(|c| c.kind() == SyntaxKind::StringLit) {
        Some(s) => first_token_or_empty(&s, SyntaxKind::String),
        None => Spanned { text: String::new(), range: node.text_range() },
    };
    Note { kind: first_token_or_empty(node, SyntaxKind::Ident), text }
}

fn is_expr(kind: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(kind, Literal | StringLit | Path | Call | List | Seed | Paren | Unary | Binary)
}

fn op_of(node: &SyntaxNode) -> char {
    tokens(node)
        .find_map(|t| match t.kind() {
            SyntaxKind::Plus => Some('+'),
            SyntaxKind::Minus => Some('-'),
            SyntaxKind::Star => Some('*'),
            SyntaxKind::Slash => Some('/'),
            _ => None,
        })
        .unwrap_or('+')
}

fn path(node: &SyntaxNode) -> Vec<Spanned> {
    tokens(node).filter(|t| t.kind() == SyntaxKind::Ident).map(|t| Spanned { text: t.text().to_owned(), range: t.text_range() }).collect()
}

fn args(node: &SyntaxNode) -> Vec<Arg> {
    let Some(list) = node.children().find(|n| n.kind() == SyntaxKind::ArgList) else {
        return Vec::new();
    };
    list.children().filter(|n| n.kind() == SyntaxKind::Arg).map(|a| Arg { name: first_token(&a, SyntaxKind::Ident), value: value_of(&a) }).collect()
}

fn expr(node: &SyntaxNode) -> Expr {
    let range = node.text_range();
    let kind = match node.kind() {
        SyntaxKind::Literal => {
            let text = first_token(node, SyntaxKind::Number).map(|s| s.text).unwrap_or_default();
            let unit = node.children().find(|n| n.kind() == SyntaxKind::Unit).map(|u| {
                tokens(&u).filter(|t| t.kind() != SyntaxKind::Whitespace && t.kind() != SyntaxKind::Newline).map(|t| t.text().to_owned()).collect()
            });
            ExprKind::Number { text, unit }
        }
        SyntaxKind::StringLit => ExprKind::Str(first_token(node, SyntaxKind::String).map(|s| s.text).unwrap_or_default()),
        SyntaxKind::Path => ExprKind::Path(path(node)),
        SyntaxKind::Call => {
            let name = node.children().find(|n| n.kind() == SyntaxKind::Path).map(|p| path(&p)).unwrap_or_default();
            ExprKind::Call { name, args: args(node) }
        }
        SyntaxKind::Seed => ExprKind::Seed(args(node)),
        SyntaxKind::List => ExprKind::List(node_exprs(node)),
        SyntaxKind::Paren => ExprKind::Paren(Box::new(value_of(node))),
        SyntaxKind::Unary => ExprKind::Unary { op: op_of(node), expr: Box::new(value_of(node)) },
        SyntaxKind::Binary => {
            let mut parts = node_exprs(node).into_iter();
            let lhs = parts.next().unwrap_or_else(|| missing(node));
            let rhs = parts.next().unwrap_or_else(|| missing(node));
            ExprKind::Binary { op: op_of(node), lhs: Box::new(lhs), rhs: Box::new(rhs) }
        }
        _ => ExprKind::Missing,
    };
    Expr { kind, range }
}

fn node_exprs(node: &SyntaxNode) -> Vec<Expr> {
    node.children().filter(|n| is_expr(n.kind())).map(|n| expr(&n)).collect()
}
