//! Persistent face and edge names as values, and their text form.
//!
//! A face name says what made the face: a feature and its role there (`F2:top`, `F2:side:l4`), a
//! fillet or chamfer and the edge it rounds (`F5:blend:<edge>`), a shell and the face it offsets
//! (`F3:inner:<face>`), or, without history, a body and an index (`Body1:face:3`). Suffixes
//! follow: a pattern copy (`@F7.2`), a piece of a split face told apart by the faces around it
//! (`#[<face>,…]`, `#k` after it for a tie), an older design's numbered piece (`#k`). An edge is
//! the two faces it separates, `~k` for one of several between them; a seam, whose two sides are
//! the same face, names that face once.
//!
//! The text form, read by [`FaceName::parse`] and [`EdgeName::parse`] and written by `Display`:
//!
//! ```text
//! edge     = face { "|" face } [TIE]       one face for an open boundary or a seam (both of an
//!                                          edge's sides on one face), more where several meet
//! face     = base { suffix }
//! base     = FEATURE ":" "blend" ":" ( edge | NUMBER )
//!          | FEATURE ":" "inner" ":" face
//!          | FEATURE ":" role
//!          | text                              (`<body>:face:<k>`, or any other text)
//! suffix   = COPY | "#[" face { "," face } "]" [HASH] | HASH
//! FEATURE  = "F" NUMBER        COPY = "@F" NUMBER "." NUMBER
//! HASH     = "#" NUMBER        TIE  = "~" NUMBER
//! ```
//!
//! Suffixes after a nested name are ambiguous in the text: in `F5:blend:A|B#[…]` the qualifier
//! may split `B` (the fillet rounds an edge of a piece made before it) or the fillet face itself
//! (a later cut split it), and so for a shell's inner wall `F3:inner:X#[…]`. A suffix naming a
//! feature later than the outer face's belongs to the outer face; any other stays inside, so a
//! later cut told apart only by older faces reads as a piece of the inner name. A `~k` ending an
//! edge numbers that edge, not a blend's edge inside it.
//!
//! Finding a face by name still goes through `naming::strip_piece` and `parent_piece`, which
//! read a trailing qualifier as the whole name's; they move here once the text form says where a
//! qualifier belongs.
//!
//! Reading is bounded in length and depth and never panics; whatever it reads writes back as the
//! same text.

/// The longest name read.
/// The longest name read. Names of pieces list their neighbours and grow with every split: a face
/// engraved with text passes 4 KiB.
pub const MAX_NAME_LEN: usize = 1 << 20;
/// The deepest nesting read (blends of blends, pieces of pieces).
const MAX_DEPTH: usize = 32;

/// A feature of the timeline, as names refer to it (`F12`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FeatureId(pub u64);

/// A persistent face name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceName {
    pub base: Base,
    /// Innermost first.
    pub suffixes: Vec<Suffix>,
}

/// What made a face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Base {
    /// `F<feature>:<role>`.
    Made { feature: FeatureId, role: Role },
    /// `F<feature>:blend:…`: a fillet or chamfer face.
    Blend { feature: FeatureId, rounds: Rounds },
    /// `F<feature>:inner:<face>`: a shell's inner wall.
    Inner { feature: FeatureId, offsets: Box<FaceName> },
    /// `<body>:face:<k>`: a face of a body without history, by index.
    BodyFace { body: String, index: u32 },
    /// Text of no form above.
    Other(String),
}

/// What a blend face rounds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rounds {
    /// The edge, by name.
    Edge(Box<EdgeName>),
    /// The edge's index among the feature's edges (no name was known).
    Index(u32),
}

/// A face's role in the feature that made it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    Top,
    Bottom,
    Start,
    End,
    Base,
    Section,
    /// `side`: a cylinder's wall, or a tool's faces swept from its whole profile.
    Wall,
    /// `side:<curve>`, `side:s<k>.<curve>` for profile `k`: swept from a sketch curve.
    Side {
        profile: Option<u32>,
        curve: String,
    },
    /// `+x`, `-z`, …: a box's face.
    BoxFace {
        axis: Axis,
        high: bool,
    },
    /// `face:<k>`: the feature's face `k`, named from geometry.
    Index(u32),
    /// A role of another form (a cap's, a tool part's, a sheet's), as written.
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// What follows a face's base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Suffix {
    /// `@F<feature>.<instance>`: a pattern or mirror copy.
    Copy { feature: FeatureId, instance: u32 },
    /// `#[…]`, `#k` after it: a piece of a split face.
    Piece(Qualifier),
    /// `#k`: a numbered piece of a design saved before format 14.
    Numbered(u32),
}

/// The faces only one piece of a split face touches, and its number among pieces they don't
/// tell apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Qualifier {
    around: Vec<FaceName>,
    pub tie: Option<u32>,
}

/// A persistent edge name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeName {
    /// The faces it separates, in order: two, one for an open boundary, more where several
    /// faces meet.
    pub faces: Vec<FaceName>,
    pub tie: Option<u32>,
}

/// Why a name could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameError {
    TooLong,
    TooDeep,
    /// At byte `at`, something else was expected.
    Expected {
        at: usize,
        what: &'static str,
    },
}

/// Whether `text` can be written into a name whole.
///
/// A curve's id goes into the name of the faces the curve makes (`F2:side:<id>`), and a
/// name reads some characters as its own: `|` separates the faces of an edge, `,` the faces
/// around a piece, `~` numbers the edges between two faces, `#` numbers a piece and `]`
/// closes one. An id holding one of them splits every name it lands in: the id `c|1` gave
/// the edges of an extrusion the names `F2:side:c|1#1|F2:side:c|1#2~1`, three separators
/// where an edge names two faces and separates them once (#5858).
///
/// The test is the name's own round trip rather than a list of characters: the text is
/// written into a name and read back, and it must read back as itself. A list would have
/// to follow every change of the grammar (#5861), and would miss an id that reads back as
/// something else (`s1.l4` reads as profile 1 of curve `l4`).
pub fn text_fits_name(text: &str) -> bool {
    let name = format!("F0:side:{text}");
    match FaceName::parse(&name) {
        Ok(f) => f.to_string() == name && matches!(&f.base, Base::Made { role: Role::Side { profile: None, curve }, .. } if curve == text),
        Err(_) => false,
    }
}

impl FaceName {
    /// The feature that made the face: its base's, a copy's when it is one.
    pub fn feature(&self) -> Option<FeatureId> {
        let copied = self.suffixes.iter().rev().find_map(|s| match s {
            Suffix::Copy { feature, .. } => Some(*feature),
            _ => None,
        });
        copied.or(match &self.base {
            Base::Made { feature, .. } | Base::Blend { feature, .. } | Base::Inner { feature, .. } => Some(*feature),
            Base::BodyFace { .. } | Base::Other(_) => None,
        })
    }

    /// The face this one is a piece of: the trailing piece suffixes off.
    pub fn stem(&self) -> FaceName {
        let keep = self.suffixes.iter().rposition(|s| matches!(s, Suffix::Copy { .. })).map_or(0, |i| i + 1);
        FaceName { base: self.base.clone(), suffixes: self.suffixes.get(..keep).unwrap_or_default().to_vec() }
    }
}

impl Qualifier {
    /// A qualifier listing `around`, at least one face.
    pub fn new(around: Vec<FaceName>, tie: Option<u32>) -> Option<Qualifier> {
        (!around.is_empty()).then_some(Qualifier { around, tie })
    }

    pub fn around(&self) -> &[FaceName] {
        &self.around
    }
}

/// Where a feature id sits in a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// It names what made a face: a base, a nested face's base, a copy.
    Name,
    /// It names a face around a piece (`#[…]`).
    Qualifier,
}

impl FaceName {
    /// The feature whose `F<n>:` starts the name.
    pub fn made_by(&self) -> Option<FeatureId> {
        match &self.base {
            Base::Made { feature, .. } | Base::Blend { feature, .. } | Base::Inner { feature, .. } => Some(*feature),
            Base::BodyFace { .. } | Base::Other(_) => None,
        }
    }

    /// Every feature id in the name, in the order the text gives them.
    pub fn features(&self) -> Vec<FeatureId> {
        let mut out = Vec::new();
        self.push_features(&mut out);
        out
    }

    fn push_features(&self, out: &mut Vec<FeatureId>) {
        match &self.base {
            Base::Made { feature, .. } | Base::Blend { feature, rounds: Rounds::Index(_) } => out.push(*feature),
            Base::Blend { feature, rounds: Rounds::Edge(e) } => {
                out.push(*feature);
                e.faces.iter().for_each(|f| f.push_features(out));
            }
            Base::Inner { feature, offsets } => {
                out.push(*feature);
                offsets.push_features(out);
            }
            Base::BodyFace { .. } | Base::Other(_) => {}
        }
        for s in &self.suffixes {
            match s {
                Suffix::Copy { feature, .. } => out.push(*feature),
                Suffix::Piece(q) => q.around.iter().for_each(|f| f.push_features(out)),
                Suffix::Numbered(_) => {}
            }
        }
    }

    /// A fillet's or chamfer's face (a piece or copy of one too).
    pub fn is_blend(&self) -> bool {
        matches!(self.base, Base::Blend { .. })
    }

    /// A pattern's or mirror's copy of a face (a piece of one too).
    pub fn is_copy(&self) -> bool {
        self.suffixes.iter().any(|s| matches!(s, Suffix::Copy { .. }))
    }

    /// A piece told apart by a number: an older design's numbered piece, or a tie.
    pub fn is_numbered_piece(&self) -> bool {
        self.suffixes.iter().any(|s| matches!(s, Suffix::Numbered(_) | Suffix::Piece(Qualifier { tie: Some(_), .. })))
    }

    /// The name with each feature id mapped by `f` (given where it sits) and each body face's
    /// body by `body`; `None` when `f` refuses one. A role's text is left as written.
    pub fn map(&self, f: &mut impl FnMut(FeatureId, Place) -> Option<FeatureId>, body: &mut impl FnMut(&str) -> Option<String>) -> Option<FaceName> {
        self.map_in(Place::Name, f, body)
    }

    fn map_in(
        &self,
        place: Place,
        f: &mut impl FnMut(FeatureId, Place) -> Option<FeatureId>,
        body: &mut impl FnMut(&str) -> Option<String>,
    ) -> Option<FaceName> {
        let base = match &self.base {
            Base::Made { feature, role } => Base::Made { feature: f(*feature, place)?, role: role.clone() },
            Base::Blend { feature, rounds } => {
                let rounds = match rounds {
                    Rounds::Edge(e) => Rounds::Edge(Box::new(e.map_in(place, f, body)?)),
                    Rounds::Index(k) => Rounds::Index(*k),
                };
                Base::Blend { feature: f(*feature, place)?, rounds }
            }
            Base::Inner { feature, offsets } => {
                let offsets = Box::new(offsets.map_in(place, f, body)?);
                Base::Inner { feature: f(*feature, place)?, offsets }
            }
            Base::BodyFace { body: b, index } => Base::BodyFace { body: body(b).unwrap_or_else(|| b.clone()), index: *index },
            Base::Other(t) => Base::Other(t.clone()),
        };
        let mut suffixes = Vec::with_capacity(self.suffixes.len());
        for s in &self.suffixes {
            suffixes.push(match s {
                Suffix::Copy { feature, instance } => Suffix::Copy { feature: f(*feature, place)?, instance: *instance },
                Suffix::Piece(q) => {
                    let around = q.around.iter().map(|a| a.map_in(Place::Qualifier, f, body)).collect::<Option<Vec<_>>>()?;
                    Suffix::Piece(Qualifier { around, tie: q.tie })
                }
                Suffix::Numbered(k) => Suffix::Numbered(*k),
            });
        }
        Some(FaceName { base, suffixes })
    }
}

impl EdgeName {
    fn map_in(
        &self,
        place: Place,
        f: &mut impl FnMut(FeatureId, Place) -> Option<FeatureId>,
        body: &mut impl FnMut(&str) -> Option<String>,
    ) -> Option<EdgeName> {
        let faces = self.faces.iter().map(|x| x.map_in(place, f, body)).collect::<Option<Vec<_>>>()?;
        Some(EdgeName { faces, tie: self.tie })
    }
}

mod lexer;
mod parser;
#[cfg(test)]
mod tests;
mod text;
mod write;

pub use text::*;
