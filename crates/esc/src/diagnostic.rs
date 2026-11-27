//! Problems found in a file, each with a stable code that tools and tests match on.

use std::fmt::Write as _;

use rowan::TextRange;
use serde::Serialize;

macro_rules! codes {
    ($($variant:ident = $code:literal),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Code {
            $($variant),*
        }

        impl Code {
            pub const ALL: &[Code] = &[$(Code::$variant),*];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Code::$variant => $code),*
                }
            }
        }
    };
}

codes! {
    UnexpectedCharacter = "E0101",
    Expected = "E0102",
    UnterminatedString = "E0103",
    Tab = "E0104",
    StrayIndent = "E0105",
    TooDeep = "E0106",
    BadEscape = "E0107",
    MissingVersion = "E0201",
    NewerVersion = "E0202",
    MissingKind = "E0203",
    NotCanonical = "E0301",
    BadName = "E0401",
    DuplicateName = "E0402",
    ReservedName = "E0403",
    UnknownName = "E0404",
    DefinedLater = "E0405",
    NoMember = "E0406",
    UnknownOperation = "E0501",
    NotAllowedHere = "E0502",
    PositionalCount = "E0503",
    UnknownArgument = "E0504",
    DuplicateArgument = "E0505",
    PositionalAfterNamed = "E0506",
    UnknownNoteKind = "E0507",
    UnknownChoice = "E0508",
    MissingUnit = "E0601",
    UnknownUnit = "E0602",
    MixedKinds = "E0603",
    LengthProduct = "E0604",
    WrongKind = "E0605",
    AbsolutePath = "E0701",
    FileNotFound = "E0702",
    ImportCycle = "E0703",
    NotAParam = "E0704",
    ImportHasErrors = "E0705",
    MassAndDensity = "E0801",
}

impl Serialize for Code {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub message: String,
    pub range: TextRange,
}

impl Diagnostic {
    pub fn new(code: Code, range: TextRange, message: impl Into<String>) -> Self {
        Diagnostic { code, message: message.into(), range }
    }
}

/// A diagnostic placed in its file, with 1-based lines and columns counted in characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Located {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub code: Code,
    pub message: String,
}

impl Located {
    pub fn new(file: &str, text: &str, d: &Diagnostic) -> Self {
        let (line, column) = line_column(text, d.range.start().into());
        let (end_line, end_column) = line_column(text, d.range.end().into());
        Located { file: file.to_owned(), line, column, end_line, end_column, code: d.code, message: d.message.clone() }
    }

    /// `file:line:col: error[E0404]: message`, then the line and a caret under the problem.
    pub fn render(&self, text: &str) -> String {
        let mut out = format!("{}:{}:{}: error[{}]: {}\n", self.file, self.line, self.column, self.code.as_str(), self.message);
        if let Some(source) = text.lines().nth(self.line.saturating_sub(1)) {
            let width = if self.end_line == self.line { self.end_column.saturating_sub(self.column).max(1) } else { 1 };
            let _ = writeln!(out, "  | {source}");
            let _ = writeln!(out, "  | {}{}", " ".repeat(self.column.saturating_sub(1)), "^".repeat(width));
        }
        out
    }
}

fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}
