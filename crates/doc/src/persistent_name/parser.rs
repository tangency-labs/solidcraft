//! The parser: the grammar in the module's doc, by recursive descent.

use super::lexer::*;
use super::*;

impl FaceName {
    pub fn parse(s: &str) -> Result<FaceName, NameError> {
        let mut p = Parser::new(s)?;
        let f = p.face(0)?;
        p.end()?;
        Ok(f)
    }
}

impl EdgeName {
    pub fn parse(s: &str) -> Result<EdgeName, NameError> {
        let mut p = Parser::new(s)?;
        // A `~k` ending the text numbers this edge.
        let tie = match p.toks.last() {
            Some(Tok { kind: Kind::Tie(k), .. }) => {
                let k = *k;
                p.toks.pop();
                Some(k)
            }
            _ => None,
        };
        let e = p.edge(0, usize::MAX)?;
        p.end()?;
        Ok(EdgeName { tie, ..e })
    }
}

impl Role {
    fn from_text(t: &str) -> Role {
        match t {
            "top" => return Role::Top,
            "bottom" => return Role::Bottom,
            "start" => return Role::Start,
            "end" => return Role::End,
            "base" => return Role::Base,
            "section" => return Role::Section,
            "side" => return Role::Wall,
            _ => {}
        }
        if let Some(rest) = t.strip_prefix("side:").filter(|r| !r.is_empty()) {
            let profiled =
                rest.strip_prefix('s').and_then(|r| r.split_once('.')).and_then(|(k, c)| Some((number(k)?, c))).filter(|(_, c)| !c.is_empty());
            return match profiled {
                Some((k, c)) => Role::Side { profile: Some(k), curve: c.to_string() },
                None => Role::Side { profile: None, curve: rest.to_string() },
            };
        }
        if let Some(k) = t.strip_prefix("face:").and_then(number) {
            return Role::Index(k);
        }
        let mut c = t.chars();
        let box_face = match (c.next(), c.next(), c.next()) {
            (Some(sign @ ('+' | '-')), Some(a), None) => match a {
                'x' => Some(Axis::X),
                'y' => Some(Axis::Y),
                'z' => Some(Axis::Z),
                _ => None,
            }
            .map(|axis| Role::BoxFace { axis, high: sign == '+' }),
            _ => None,
        };
        box_face.unwrap_or_else(|| Role::Other(t.to_string()))
    }
}

/// The trailing suffixes of `inner`, the last name nested in a face of `feature`, that name a
/// later feature, taken off it: they are the outer face's (a later cut split it, a later pattern
/// copied it).
fn later_suffixes(feature: FeatureId, inner: &mut FaceName) -> Vec<Suffix> {
    let later = |s: &Suffix| match s {
        Suffix::Copy { feature: f, .. } => *f > feature,
        Suffix::Piece(q) => q.around.iter().filter_map(FaceName::feature).max().is_some_and(|f| f > feature),
        Suffix::Numbered(_) => false,
    };
    let keep = inner.suffixes.iter().rposition(|s| !later(s)).map_or(0, |i| i + 1);
    inner.suffixes.split_off(keep)
}

struct Parser<'a> {
    s: &'a str,
    toks: Vec<Tok>,
    at: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Result<Parser<'a>, NameError> {
        if s.len() > MAX_NAME_LEN {
            return Err(NameError::TooLong);
        }
        Ok(Parser { s, toks: lex(s), at: 0 })
    }

    fn peek(&self, ahead: usize) -> Option<Tok> {
        self.toks.get(self.at + ahead).copied()
    }

    fn byte(&self) -> usize {
        self.peek(0).map_or(self.s.len(), |t| t.start)
    }

    fn expected<T>(&self, what: &'static str) -> Result<T, NameError> {
        Err(NameError::Expected { at: self.byte(), what })
    }

    fn end(&self) -> Result<(), NameError> {
        if self.peek(0).is_some() { self.expected("the end of the name") } else { Ok(()) }
    }

    fn word(&self, ahead: usize) -> Option<&'a str> {
        self.peek(ahead).filter(|t| t.kind == Kind::Word).and_then(|t| self.s.get(t.start..t.end))
    }

    /// `<word>:` at the cursor.
    fn keyword(&self, w: &str) -> bool {
        self.word(0) == Some(w) && self.peek(1).is_some_and(|t| t.kind == Kind::Colon)
    }

    /// An edge: its faces joined by `|`, at most `max` of them.
    fn edge(&mut self, depth: usize, max: usize) -> Result<EdgeName, NameError> {
        let mut faces = vec![self.face(depth + 1)?];
        while faces.len() < max && self.peek(0).map(|t| t.kind) == Some(Kind::Pipe) {
            self.at += 1;
            faces.push(self.face(depth + 1)?);
        }
        let tie = match self.peek(0).map(|t| t.kind) {
            Some(Kind::Tie(k)) => {
                self.at += 1;
                Some(k)
            }
            _ => None,
        };
        Ok(EdgeName { faces, tie })
    }

    fn face(&mut self, depth: usize) -> Result<FaceName, NameError> {
        if depth > MAX_DEPTH {
            return Err(NameError::TooDeep);
        }
        let mut face = match (self.peek(0).map(|t| t.kind), self.peek(1).map(|t| t.kind)) {
            (Some(Kind::Feature(n)), Some(Kind::Colon)) => {
                self.at += 2;
                let feature = FeatureId(n);
                if self.keyword("blend") {
                    self.at += 2;
                    self.blend(feature, depth)?
                } else if self.keyword("inner") {
                    self.at += 2;
                    let mut offsets = self.face(depth + 1)?;
                    let moved = later_suffixes(feature, &mut offsets);
                    FaceName { base: Base::Inner { feature, offsets: Box::new(offsets) }, suffixes: moved }
                } else {
                    let role = Role::from_text(self.text()?);
                    FaceName { base: Base::Made { feature, role }, suffixes: Vec::new() }
                }
            }
            _ => {
                let t = self.text()?;
                let body_face = t.rsplit_once(":face:").and_then(|(b, k)| Some((b, number(k)?))).filter(|(b, _)| !b.is_empty());
                let base = match body_face {
                    Some((body, index)) => Base::BodyFace { body: body.to_string(), index },
                    None => Base::Other(t.to_string()),
                };
                FaceName { base, suffixes: Vec::new() }
            }
        };
        self.suffixes(&mut face, depth)?;
        Ok(face)
    }

    /// A blend face of `feature`, the cursor after `blend:`.
    fn blend(&mut self, feature: FeatureId, depth: usize) -> Result<FaceName, NameError> {
        let ends_face = |k: Option<Kind>| !matches!(k, Some(Kind::Word | Kind::Colon | Kind::Feature(_)));
        if let Some(k) = self.word(0).and_then(number::<u32>)
            && ends_face(self.peek(1).map(|t| t.kind))
        {
            self.at += 1;
            return Ok(FaceName { base: Base::Blend { feature, rounds: Rounds::Index(k) }, suffixes: Vec::new() });
        }
        // A blend names the edge it rounds by its faces, two at most: the rest is the outer edge's.
        let mut edge = self.edge(depth + 1, 2)?;
        let moved = edge.faces.last_mut().map(|b| later_suffixes(feature, b)).unwrap_or_default();
        Ok(FaceName { base: Base::Blend { feature, rounds: Rounds::Edge(Box::new(edge)) }, suffixes: moved })
    }

    /// Words and colons up to the next other token, as written.
    fn text(&mut self) -> Result<&'a str, NameError> {
        let start = self.byte();
        let mut end = start;
        while let Some(t) = self.peek(0).filter(|t| matches!(t.kind, Kind::Word | Kind::Colon | Kind::Feature(_))) {
            end = t.end;
            self.at += 1;
        }
        match self.s.get(start..end).filter(|t| !t.is_empty()) {
            Some(t) => Ok(t),
            None => self.expected("a face"),
        }
    }

    fn suffixes(&mut self, face: &mut FaceName, depth: usize) -> Result<(), NameError> {
        while let Some(t) = self.peek(0) {
            match t.kind {
                Kind::Copy(feature, instance) => {
                    self.at += 1;
                    face.suffixes.push(Suffix::Copy { feature: FeatureId(feature), instance });
                }
                Kind::Open => {
                    self.at += 1;
                    let mut around = vec![self.face(depth + 1)?];
                    loop {
                        match self.peek(0).map(|t| t.kind) {
                            Some(Kind::Comma) => {
                                self.at += 1;
                                around.push(self.face(depth + 1)?);
                            }
                            Some(Kind::Close) => {
                                self.at += 1;
                                break;
                            }
                            _ => return self.expected("`,` or `]` in a piece's faces"),
                        }
                    }
                    let tie = match self.peek(0).map(|t| t.kind) {
                        Some(Kind::Hash(k)) => {
                            self.at += 1;
                            Some(k)
                        }
                        _ => None,
                    };
                    face.suffixes.push(Suffix::Piece(Qualifier { around, tie }));
                }
                Kind::Hash(k) => {
                    self.at += 1;
                    face.suffixes.push(Suffix::Numbered(k));
                }
                _ => return Ok(()),
            }
        }
        Ok(())
    }
}
