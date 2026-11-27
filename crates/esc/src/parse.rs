//! The hand-written parser: text to a lossless syntax tree that keeps every byte, with every
//! syntax error in the file.

use rowan::{GreenNode, GreenNodeBuilder, TextRange, TextSize};

use crate::diagnostic::{Code, Diagnostic};
use crate::syntax::{SyntaxKind, SyntaxKind::*, SyntaxNode, lex};
use crate::units::is_unit_word;
use crate::vocabulary::NOTE_KINDS;

/// Nesting deeper than this is refused, so hostile input can't exhaust the stack.
const MAX_DEPTH: u32 = 64;

#[derive(Debug, Clone)]
pub struct Parse {
    pub green: GreenNode,
    pub errors: Vec<Diagnostic>,
}

impl Parse {
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }
}

pub fn parse(text: &str) -> Parse {
    let tokens = lex(text);
    let mut offsets = Vec::with_capacity(tokens.len());
    let mut at = 0u32;
    for (_, t) in &tokens {
        offsets.push(at);
        at = at.saturating_add(u32::try_from(t.len()).unwrap_or(u32::MAX));
    }
    let mut p = Parser { tokens, offsets, end: at, pos: 0, builder: GreenNodeBuilder::new(), errors: Vec::new(), brackets: 0, depth: 0 };
    p.file();
    Parse { green: p.builder.finish(), errors: p.errors }
}

struct Line {
    indent: usize,
    tab: bool,
    first: usize,
    blank: bool,
}

struct Parser<'t> {
    tokens: Vec<(SyntaxKind, &'t str)>,
    offsets: Vec<u32>,
    end: u32,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<Diagnostic>,
    brackets: u32,
    depth: u32,
}

impl<'t> Parser<'t> {
    fn kind_at(&self, i: usize) -> Option<SyntaxKind> {
        self.tokens.get(i).map(|(k, _)| *k)
    }

    fn text_at(&self, i: usize) -> &'t str {
        self.tokens.get(i).map_or("", |(_, t)| *t)
    }

    fn current(&self) -> Option<SyntaxKind> {
        self.kind_at(self.pos)
    }

    fn range_at(&self, i: usize) -> TextRange {
        match (self.offsets.get(i), self.tokens.get(i)) {
            (Some(&start), Some((_, t))) => {
                let len = u32::try_from(t.len()).unwrap_or(0);
                TextRange::at(TextSize::from(start), TextSize::from(len))
            }
            _ => TextRange::empty(TextSize::from(self.end)),
        }
    }

    fn error(&mut self, code: Code, range: TextRange, message: impl Into<std::string::String>) {
        // One error per place: recovery after the first would only repeat it.
        if self.errors.last().is_some_and(|d| d.range.start() == range.start()) {
            return;
        }
        self.errors.push(Diagnostic::new(code, range, message));
    }

    fn error_here(&mut self, code: Code, message: impl Into<std::string::String>) {
        let range = match self.current() {
            Some(Newline) | None => TextRange::empty(self.range_at(self.pos).start()),
            _ => self.range_at(self.pos),
        };
        if self.current() == Some(ErrorToken) {
            let text = self.text_at(self.pos);
            self.error(Code::UnexpectedCharacter, range, format!("unexpected character `{}`", text.escape_debug()));
        } else {
            self.error(code, range, message);
        }
    }

    fn bump(&mut self) {
        if let Some(&(kind, text)) = self.tokens.get(self.pos) {
            if kind == ErrorToken {
                let range = self.range_at(self.pos);
                self.error(Code::UnexpectedCharacter, range, format!("unexpected character `{}`", text.escape_debug()));
            }
            self.builder.token(rowan::SyntaxKind(kind as u16), text);
            self.pos += 1;
        }
    }

    fn start(&mut self, kind: SyntaxKind) {
        self.builder.start_node(rowan::SyntaxKind(kind as u16));
    }

    fn finish(&mut self) {
        self.builder.finish_node();
    }

    fn line(&self, start: usize) -> Line {
        let mut i = start;
        let mut indent = 0;
        let mut tab = false;
        while self.kind_at(i) == Some(Whitespace) {
            let t = self.text_at(i);
            indent += t.len();
            tab |= t.contains('\t');
            i += 1;
        }
        let blank = matches!(self.kind_at(i), Some(Newline) | None);
        Line { indent, tab, first: i, blank }
    }

    fn next_line(&self, mut i: usize) -> usize {
        while let Some(k) = self.kind_at(i) {
            i += 1;
            if k == Newline {
                break;
            }
        }
        i
    }

    /// The first token from `i` on that isn't whitespace.
    fn after_ws(&self, mut i: usize) -> usize {
        while self.kind_at(i) == Some(Whitespace) {
            i += 1;
        }
        i
    }

    /// Does the line starting at `start` begin a new entry, sketch line or note? Then a newline
    /// inside brackets before it ends the brackets instead of continuing them.
    fn starts_statement(&self, start: usize) -> bool {
        let line = self.line(start);
        if line.blank {
            return true;
        }
        if self.kind_at(line.first) != Some(Ident) {
            return false;
        }
        let next = self.after_ws(line.first + 1);
        match self.kind_at(next) {
            Some(Equals) => true,
            Some(Colon) => NOTE_KINDS.contains(&self.text_at(line.first)) && self.kind_at(self.after_ws(next + 1)) == Some(String),
            _ => false,
        }
    }

    /// Is token `i` a newline inside brackets that the value carries on past?
    fn newline_continues(&self, i: usize) -> bool {
        self.brackets > 0 && self.kind_at(i) == Some(Newline) && self.kind_at(i + 1).is_some() && !self.starts_statement(i + 1)
    }

    /// Whitespace, and newlines while inside brackets.
    fn trivia(&mut self) {
        while self.current() == Some(Whitespace) || self.newline_continues(self.pos) {
            self.bump();
        }
    }

    /// The kind of the next significant token, without consuming trivia.
    fn peek(&self) -> Option<SyntaxKind> {
        self.kind_at(self.peek_index())
    }

    fn peek_index(&self) -> usize {
        let mut i = self.pos;
        while self.kind_at(i) == Some(Whitespace) || self.newline_continues(i) {
            i += 1;
        }
        i
    }

    fn expect(&mut self, kind: SyntaxKind, what: &str) -> bool {
        self.trivia();
        if self.current() == Some(kind) {
            self.bump();
            true
        } else {
            self.error_here(Code::Expected, format!("expected {what}"));
            false
        }
    }

    /// Ends a line: trailing whitespace, then the newline. Anything else left on the line is an
    /// error, kept in the tree.
    fn rest_of_line(&mut self) {
        while self.current() == Some(Whitespace) {
            self.bump();
        }
        match self.current() {
            None => {}
            Some(Newline) => self.bump(),
            Some(_) => {
                self.error_here(Code::Expected, "expected the end of the line");
                self.error_line();
            }
        }
    }

    /// The rest of the line, kept in the tree as an error node, and its newline.
    fn error_line(&mut self) {
        self.start(ErrorNode);
        while !matches!(self.current(), Some(Newline) | None) {
            self.bump();
        }
        self.finish();
        self.bump();
    }

    fn file(&mut self) {
        self.start(File);
        self.header();
        while self.current().is_some() {
            let line = self.line(self.pos);
            if line.blank {
                let end = self.next_line(self.pos);
                while self.pos < end {
                    self.bump();
                }
            } else if line.indent > 0 {
                let range = self.range_at(line.first);
                self.error(Code::StrayIndent, range, "an indented line goes under an entry");
                self.error_line();
            } else {
                self.start(Entry);
                self.assignment(true);
                self.children(0, true);
                self.finish();
            }
        }
        self.finish();
    }

    fn header(&mut self) {
        self.start(Header);
        if self.current() == Some(Ident) && self.text_at(self.pos) == "esc" {
            self.start(VersionLine);
            self.bump();
            self.trivia();
            if self.current() == Some(Number) {
                self.bump();
            } else {
                self.error_here(Code::MissingVersion, "expected the language version, `esc 1`");
            }
            self.rest_of_line();
            self.finish();
        } else {
            let range = TextRange::empty(self.range_at(self.pos).start());
            self.error(Code::MissingVersion, range, "the first line is the language version, `esc 1`");
        }
        let line = self.line(self.pos);
        let alone = self.kind_at(line.first) == Some(Ident) && matches!(self.kind_at(self.after_ws(line.first + 1)), Some(Newline) | None);
        if line.indent == 0 && alone {
            self.start(KindLine);
            self.bump();
            self.rest_of_line();
            self.finish();
        } else {
            let range = TextRange::empty(self.range_at(self.pos).start());
            self.error(Code::MissingKind, range, "the second line is the file kind, `part` or `assembly`");
        }
        self.finish();
    }

    /// `name = value` to the end of the line.
    fn assignment(&mut self, top: bool) {
        let name = self.pos;
        let next = self.after_ws(name + 1);
        if self.current() == Some(Ident) && self.kind_at(next) == Some(Equals) {
            self.bump();
            self.trivia();
            self.bump();
            self.expr();
            self.rest_of_line();
        } else if top && self.current() == Some(Ident) && self.kind_at(next) == Some(Colon) {
            let range = self.range_at(name);
            self.error(Code::StrayIndent, range, "a note goes indented under the entry it's about");
            self.error_line();
        } else {
            self.error_here(Code::Expected, "expected an entry, `name = ...`");
            self.error_line();
        }
    }

    /// The lines indented under an entry (notes and, for a sketch, its lines) or under a sketch
    /// line (notes). Blank lines between them stay inside the node, for `fmt` to drop.
    fn children(&mut self, level: usize, items: bool) {
        loop {
            let mut j = self.pos;
            while self.kind_at(j).is_some() && self.line(j).blank {
                j = self.next_line(j);
            }
            if self.kind_at(j).is_none() {
                break;
            }
            let line = self.line(j);
            if line.indent.div_ceil(2) <= level {
                break;
            }
            while self.pos < j {
                self.bump();
            }
            self.child(line, level, items);
        }
    }

    fn child(&mut self, line: Line, level: usize, items: bool) {
        if line.tab {
            let range = self.range_at(self.pos);
            self.error(Code::Tab, range, "indent with spaces, not tabs");
        }
        let first = line.first;
        let next = self.after_ws(first + 1);
        let is_ident = self.kind_at(first) == Some(Ident);
        match self.kind_at(next) {
            Some(Colon) if is_ident => {
                self.start(Note);
                while self.pos < first {
                    self.bump();
                }
                self.note();
                self.finish();
            }
            Some(Equals) if is_ident => {
                self.start(Item);
                while self.pos < first {
                    self.bump();
                }
                if !items {
                    let range = self.range_at(first);
                    self.error(Code::StrayIndent, range, "only a sketch has indented `name = ...` lines, and they don't nest");
                }
                self.assignment(false);
                self.children(level + 1, false);
                self.finish();
            }
            _ => {
                while self.pos < first {
                    self.bump();
                }
                self.error_here(Code::Expected, "expected a note (`why: \"...\"`) or a sketch line (`name = ...`)");
                self.error_line();
            }
        }
    }

    fn note(&mut self) {
        self.bump();
        self.trivia();
        self.bump();
        self.trivia();
        if self.current() == Some(String) {
            self.string();
        } else {
            self.error_here(Code::Expected, "expected the note's text in double quotes");
        }
        self.rest_of_line();
    }

    fn string(&mut self) {
        let text = self.text_at(self.pos);
        let range = self.range_at(self.pos);
        if !string_closed(text) {
            self.error(Code::UnterminatedString, range, "this string has no closing `\"` on its line");
        }
        let mut chars = text.chars().skip(1);
        while let Some(c) = chars.next() {
            if c == '\\' && !matches!(chars.next(), Some('"' | '\\')) {
                self.error(Code::BadEscape, range, "a string only takes the escapes `\\\"` and `\\\\`");
                break;
            }
        }
        self.start(StringLit);
        self.bump();
        self.finish();
    }

    fn expr(&mut self) {
        self.binary(1);
    }

    fn enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.error_here(Code::TooDeep, format!("nested more than {MAX_DEPTH} deep"));
            self.depth -= 1;
            return false;
        }
        true
    }

    fn binary(&mut self, min: u8) {
        if !self.enter() {
            return;
        }
        self.trivia();
        let checkpoint = self.builder.checkpoint();
        self.unary();
        loop {
            let prec = match self.peek() {
                Some(Plus | Minus) => 1,
                Some(Star | Slash) => 2,
                _ => break,
            };
            if prec < min {
                break;
            }
            self.builder.start_node_at(checkpoint, rowan::SyntaxKind(Binary as u16));
            self.trivia();
            self.bump();
            self.binary(prec + 1);
            self.finish();
        }
        self.depth -= 1;
    }

    fn unary(&mut self) {
        self.trivia();
        if matches!(self.current(), Some(Plus | Minus)) {
            if !self.enter() {
                return;
            }
            self.start(Unary);
            self.bump();
            self.unary();
            self.finish();
            self.depth -= 1;
        } else {
            self.primary();
        }
    }

    fn primary(&mut self) {
        self.trivia();
        match self.current() {
            Some(Number) => {
                self.start(Literal);
                self.bump();
                let i = self.peek_index();
                if self.kind_at(i) == Some(Ident) && is_unit_word(self.text_at(i)) {
                    self.trivia();
                    self.start(Unit);
                    self.bump();
                    let slash = self.peek_index();
                    if self.kind_at(slash) == Some(Slash) {
                        let word = self.after_ws(slash + 1);
                        if self.kind_at(word) == Some(Ident) && is_unit_word(self.text_at(word)) {
                            self.trivia();
                            self.bump();
                            self.trivia();
                            self.bump();
                        }
                    }
                    self.finish();
                }
                self.finish();
            }
            Some(String) => self.string(),
            Some(Ident) => {
                let checkpoint = self.builder.checkpoint();
                self.start(Path);
                self.bump();
                loop {
                    let dot = self.peek_index();
                    if self.kind_at(dot) != Some(Dot) {
                        break;
                    }
                    self.trivia();
                    self.bump();
                    self.trivia();
                    if self.current() == Some(Ident) {
                        self.bump();
                    } else {
                        self.error_here(Code::Expected, "expected a name after `.`");
                        break;
                    }
                }
                self.finish();
                if self.peek() == Some(LParen) {
                    self.builder.start_node_at(checkpoint, rowan::SyntaxKind(Call as u16));
                    self.trivia();
                    self.args();
                    self.finish();
                }
            }
            Some(LParen) => {
                self.start(Paren);
                self.bump();
                self.brackets += 1;
                self.expr();
                self.expect(RParen, "`)`");
                self.brackets -= 1;
                self.finish();
            }
            Some(LBracket) => {
                self.start(List);
                self.bump();
                self.brackets += 1;
                loop {
                    self.trivia();
                    if matches!(self.current(), Some(RBracket | Newline) | None) {
                        break;
                    }
                    let before = self.pos;
                    self.expr();
                    self.progress(before);
                    if self.peek() == Some(Comma) {
                        self.trivia();
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.expect(RBracket, "`]`");
                self.brackets -= 1;
                self.finish();
            }
            Some(Tilde) => {
                self.start(Seed);
                self.bump();
                self.trivia();
                if self.current() == Some(LParen) {
                    self.args();
                } else {
                    self.error_here(Code::Expected, "expected `(` after `~`");
                }
                self.finish();
            }
            _ => self.error_here(Code::Expected, "expected a value"),
        }
    }

    /// Makes sure a loop moves on: a token nothing could parse becomes an error node.
    fn progress(&mut self, before: usize) {
        if self.pos == before && !matches!(self.current(), Some(Newline | RParen | RBracket | Comma) | None) {
            self.start(ErrorNode);
            self.bump();
            self.finish();
        }
    }

    fn args(&mut self) {
        self.start(ArgList);
        self.bump();
        self.brackets += 1;
        loop {
            self.trivia();
            if matches!(self.current(), Some(RParen | Newline) | None) {
                break;
            }
            self.start(Arg);
            if self.current() == Some(Ident) {
                let colon = self.after_ws(self.pos + 1);
                if self.kind_at(colon) == Some(Colon) {
                    self.bump();
                    self.trivia();
                    self.bump();
                }
            }
            let before = self.pos;
            self.expr();
            self.progress(before);
            self.finish();
            if self.peek() == Some(Comma) {
                self.trivia();
                self.bump();
            } else {
                break;
            }
        }
        self.expect(RParen, "`)`");
        self.brackets -= 1;
        self.finish();
    }
}

fn string_closed(text: &str) -> bool {
    let mut escaped = false;
    for (i, c) in text.char_indices().skip(1) {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return i + 1 == text.len(),
            _ => escaped = false,
        }
    }
    false
}
