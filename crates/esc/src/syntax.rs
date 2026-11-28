//! The token and node kinds of the lossless syntax tree, and the lexer.

macro_rules! kinds {
    ($($kind:ident),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u16)]
        pub enum SyntaxKind {
            $($kind),*
        }

        const ALL: &[SyntaxKind] = &[$(SyntaxKind::$kind),*];
    };
}

kinds! {
    // Tokens
    Whitespace,
    Newline,
    Ident,
    Number,
    String,
    Equals,
    Colon,
    Comma,
    Dot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Plus,
    Minus,
    Star,
    Slash,
    Tilde,
    ErrorToken,
    // Nodes
    File,
    Header,
    VersionLine,
    KindLine,
    Entry,
    Item,
    Note,
    Literal,
    Unit,
    StringLit,
    Path,
    Call,
    ArgList,
    Arg,
    List,
    Seed,
    Paren,
    Unary,
    Binary,
    ErrorNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {}

impl rowan::Language for Lang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        ALL.get(usize::from(raw.0)).copied().unwrap_or(SyntaxKind::ErrorNode)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Lang>;
pub type SyntaxToken = rowan::SyntaxToken<Lang>;

/// Splits `text` into tokens that cover every byte. Never fails: anything unknown is an
/// `ErrorToken` the parser reports.
pub fn lex(text: &str) -> Vec<(SyntaxKind, &str)> {
    use SyntaxKind::*;
    let mut tokens = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let (kind, len) = match c {
            ' ' | '\t' => (Whitespace, rest.find(|c| c != ' ' && c != '\t').unwrap_or(rest.len())),
            '\n' => (Newline, 1),
            '\r' if rest.starts_with("\r\n") => (Newline, 2),
            '0'..='9' => (Number, number_len(rest)),
            'a'..='z' | 'A'..='Z' | '_' => (Ident, rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len())),
            '"' => (String, string_len(rest)),
            '=' => (Equals, 1),
            ':' => (Colon, 1),
            ',' => (Comma, 1),
            '.' => (Dot, 1),
            '(' => (LParen, 1),
            ')' => (RParen, 1),
            '[' => (LBracket, 1),
            ']' => (RBracket, 1),
            '+' => (Plus, 1),
            '-' => (Minus, 1),
            '*' => (Star, 1),
            '/' => (Slash, 1),
            '~' => (Tilde, 1),
            _ => (ErrorToken, c.len_utf8()),
        };
        let (token, tail) = rest.split_at(len.max(1).min(rest.len()));
        tokens.push((kind, token));
        rest = tail;
    }
    tokens
}

fn number_len(text: &str) -> usize {
    let digits = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let whole = digits(text);
    let fraction = text.get(whole..).and_then(|t| t.strip_prefix('.')).map_or(0, digits);
    if fraction > 0 { whole + 1 + fraction } else { whole }
}

/// A string runs to its closing quote, or to the end of the line when it has none; the parser
/// reports the second case.
fn string_len(text: &str) -> usize {
    let mut escaped = false;
    for (i, c) in text.char_indices().skip(1) {
        match c {
            '\n' | '\r' => return i,
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return i + 1,
            _ => escaped = false,
        }
    }
    text.len()
}
