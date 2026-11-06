//! Questions callers ask of a name kept as a `String`.

use super::lexer::number;
use super::*;

/// The feature whose `F<n>:` starts the face name `name` (a copy's is its source's). Every name a
/// feature made starts so (`F2:top`, `F5:blend:…`, `F3:inner:…`), so only that token is read:
/// cheap enough to ask of every face each frame.
pub fn made_by(name: &str) -> Option<u64> {
    let rest = name.strip_prefix('F')?;
    let end = rest.find(|c: char| !c.is_ascii_digit())?;
    if rest.get(end..end + 1) != Some(":") {
        return None;
    }
    number(rest.get(..end)?)
}

/// `name` without the `F<n>:` of the feature that made it, and `n`.
pub fn split_feature(name: &str) -> Option<(u64, &str)> {
    let id = made_by(name)?;
    Some((id, name.strip_prefix(&format!("F{id}:"))?))
}

/// Whether `name` is a pattern's or mirror's copy of a face.
pub fn is_copy(name: &str) -> bool {
    FaceName::parse(name).map_or_else(|_| name.contains('@'), |f| f.is_copy())
}

/// Whether `name` is a fillet's or chamfer's face.
pub fn is_blend(name: &str) -> bool {
    FaceName::parse(name).map_or_else(|_| name.contains(":blend:"), |f| f.is_blend())
}

/// Whether `name` is a piece told apart by a number (an older design's piece, or a tie).
pub fn is_numbered_piece(name: &str) -> bool {
    FaceName::parse(name).map_or_else(
        |_| name.match_indices('#').any(|(i, _)| name.get(i + 1..).is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))),
        |f| f.is_numbered_piece(),
    )
}

/// Whether `name` is a whole `side` face: a cylinder's wall, or a tool's faces swept from its
/// whole profile.
pub fn is_wall(name: &str) -> bool {
    FaceName::parse(name).map_or_else(|_| name.ends_with(":side"), |f| f.suffixes.is_empty() && matches!(f.base, Base::Made { role: Role::Wall, .. }))
}

/// `name` with every feature id `ids` maps replaced, the rest kept; `None` when it doesn't read.
pub fn map_features(name: &str, ids: &std::collections::BTreeMap<u64, u64>) -> Option<String> {
    let f = FaceName::parse(name).ok()?;
    let mapped = f.map(&mut |id, _| Some(ids.get(&id.0).map_or(id, |n| FeatureId(*n))), &mut |_| None)?;
    Some(mapped.to_string())
}
