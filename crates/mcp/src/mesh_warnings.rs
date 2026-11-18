//! A mesh export's `warnings` in a compact reply (#18831). A curved part can leave a hundred
//! faces short of the smallest angle, one line each. Lines of one kind become one line with how
//! many there were, the bodies they are on, what stopped the mesh and the worst of them.
//!
//! The lines are read in the exact wording `solidcraft_io` writes them in (`mesh_notes` in
//! `crates/io/src/lib.rs`). A line that doesn't match that wording, or holds a number that isn't
//! finite, is kept as it is: a changed wording makes the compact reply longer, never shorter
//! by a warning.

use serde_json::{Map, Value, json};

/// What a line says, with the words that tell two groups apart. Only lines with the same kind
/// are counted together: a face past its chord tolerance is never counted with one within it.
#[derive(Clone, PartialEq)]
enum Kind<'a> {
    /// A face short of the smallest angle asked (the angle asked, in degrees).
    ShortOfAngle(&'a str),
    /// A face whose shaping stopped at a bound with the angle asked reached.
    BoundReached(&'a str),
    /// A face whose triangles lie off the surface: "past it", "still within it" or "within it
    /// where measured", as the line says.
    OffSurface(&'a str),
    /// An edge whose chords lie past its chord tolerance.
    OffCurve,
}

/// One line read.
struct Note<'a> {
    kind: Kind<'a>,
    /// The body's name as the line leads with it ("" when the line names no body).
    body: &'a str,
    /// "face 3" or "edge 3".
    item: String,
    /// What stopped the mesh: the bounds of an angle line, the reason of a surface line.
    reasons: Vec<&'a str>,
    /// The angle, or the distance off the surface or curve, as written.
    value: &'a str,
    /// The chord tolerance as written (surface and curve lines).
    tolerance: &'a str,
    /// How bad it is, to pick the worst: the smaller angle, or the larger distance against the
    /// line's own tolerance.
    badness: f64,
    /// Middles the line says it could not measure.
    unmeasured: u64,
}

/// A finite number from the text the export wrote; `None` for anything else.
fn finite(t: &str) -> Option<f64> {
    t.parse::<f64>().ok().filter(|x| x.is_finite())
}

/// The bounds in "Points, Side".
fn list(t: &str) -> Vec<&str> {
    t.split(", ").filter(|x| !x.is_empty()).collect()
}

/// The line split at its last "face N: " or "edge N: ": the body before it (without the ": "
/// that leads to it), whether it is an edge, N and the rest. Split from the right, so a body
/// whose name holds ": face " keeps its whole name.
fn split_item(w: &str) -> Option<(&str, bool, &str, &str)> {
    let mut best: Option<(&str, bool, &str, &str)> = None;
    for (marker, edge) in [("face ", false), ("edge ", true)] {
        for (at, _) in w.rmatch_indices(marker) {
            let before = w.get(..at)?;
            if !(before.is_empty() || before.ends_with(": ")) {
                continue;
            }
            let after = w.get(at + marker.len()..)?;
            let Some((n, rest)) = after.split_once(": ") else { continue };
            if n.is_empty() || !n.bytes().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if best.is_none_or(|(b, ..)| before.len() > b.len()) {
                best = Some((before.strip_suffix(": ").unwrap_or(before), edge, n, rest));
            }
            break;
        }
    }
    best
}

/// `w` read as a mesh export warning; `None` for any other line.
fn note(w: &str) -> Option<Note<'_>> {
    let (body, edge, n, rest) = split_item(w)?;
    let item = format!("{} {n}", if edge { "edge" } else { "face" });
    let base = |kind, reasons, value, tolerance, badness, unmeasured| Note {
        kind,
        body,
        item: item.clone(),
        reasons,
        value,
        tolerance,
        badness,
        unmeasured,
    };
    if edge {
        // its sampling stopped at a bound ({cap}); its chords lie up to {sag} mm off the curve, past the {tol} mm chord tolerance
        let r = rest.strip_prefix("its sampling stopped at a bound (")?;
        let (cap, r) = r.split_once("); its chords lie up to ")?;
        let (sag, r) = r.split_once(" mm off the curve, past the ")?;
        let tol = r.strip_suffix(" mm chord tolerance")?;
        let (s, t) = (finite(sag)?, finite(tol)?);
        return Some(base(Kind::OffCurve, list(cap), sag, tol, if t > 0.0 { s / t } else { s }, 0));
    }
    if let Some(r) = rest.strip_prefix("smallest angle ") {
        // smallest angle {x}°, short of the {asked}° asked (stopped by: {limits})
        let (angle, r) = r.split_once("°, short of the ")?;
        let (asked, r) = r.split_once("° asked (stopped by: ")?;
        let limits = r.strip_suffix(')')?;
        finite(asked)?;
        return Some(base(Kind::ShortOfAngle(asked), list(limits), angle, "", finite(angle)?, 0));
    }
    if let Some(r) = rest.strip_prefix("its shaping stopped at a bound (") {
        // its shaping stopped at a bound ({limits}) with the {asked}° asked reached (smallest angle {x}°)
        let (limits, r) = r.split_once(") with the ")?;
        let (asked, r) = r.split_once("° asked reached (smallest angle ")?;
        let angle = r.strip_suffix("°)")?;
        finite(asked)?;
        return Some(base(Kind::BoundReached(asked), list(limits), angle, "", finite(angle)?, 0));
    }
    // {reason}. Its triangles lie up to {sag} mm off the surface, {how} (chord tolerance {tol} mm[, {n} middles unmeasured])
    let (reason, r) = rest.split_once(". Its triangles lie up to ")?;
    let (sag, r) = r.split_once(" mm off the surface, ")?;
    let (how, r) = r.split_once(" (chord tolerance ")?;
    if !matches!(how, "past it" | "still within it" | "within it where measured") {
        return None;
    }
    let (tol, r) = r.strip_suffix(')')?.split_once(" mm")?;
    let unmeasured = match r {
        "" => 0,
        r => r.strip_prefix(", ")?.strip_suffix(" middles unmeasured")?.parse::<u64>().ok()?,
    };
    let (s, t) = (finite(sag)?, finite(tol)?);
    Some(base(Kind::OffSurface(how), vec![reason], sag, tol, if t > 0.0 { s / t } else { s }, unmeasured))
}

/// The bodies of `notes` in the order they first appear, each with how many lines it has:
/// "Base (2), Lid (1)".
fn bodies(notes: &[&Note<'_>]) -> String {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for n in notes {
        match seen.iter_mut().find(|(b, _)| *b == n.body) {
            Some((_, c)) => *c += 1,
            None => seen.push((n.body, 1)),
        }
    }
    seen.iter().map(|(b, c)| format!("{} ({c})", if b.is_empty() { "no body named" } else { b })).collect::<Vec<_>>().join(", ")
}

/// The reasons of `notes`, each once, in the order they first appear.
fn reasons(notes: &[&Note<'_>]) -> String {
    let mut all: Vec<&str> = Vec::new();
    for r in notes.iter().flat_map(|n| n.reasons.iter()) {
        if !all.contains(r) {
            all.push(r);
        }
    }
    all.join(", ")
}

/// The one line that stands for `group` (two or more lines of one kind).
fn summary(kind: &Kind<'_>, group: &[&Note<'_>]) -> Option<String> {
    let worst = match kind {
        Kind::ShortOfAngle(_) | Kind::BoundReached(_) => group.iter().min_by(|a, b| a.badness.total_cmp(&b.badness)),
        Kind::OffSurface(_) | Kind::OffCurve => group.iter().max_by(|a, b| a.badness.total_cmp(&b.badness)),
    }?;
    let at = if worst.body.is_empty() { worst.item.clone() } else { format!("{}: {}", worst.body, worst.item) };
    let (n, why, on) = (group.len(), reasons(group), bodies(group));
    Some(match kind {
        Kind::ShortOfAngle(asked) => {
            format!("{n} faces short of the {asked}° asked (stopped by: {why}); the sharpest: smallest angle {}° ({at}); bodies: {on}", worst.value)
        }
        Kind::BoundReached(asked) => format!(
            "{n} faces whose shaping stopped at a bound ({why}) with the {asked}° asked reached; the sharpest: smallest angle {}° ({at}); bodies: {on}",
            worst.value
        ),
        Kind::OffSurface(how) => {
            let unmeasured: u64 = group.iter().map(|g| g.unmeasured).sum();
            let unmeasured = if unmeasured > 0 { format!("; {unmeasured} middles unmeasured") } else { String::new() };
            format!(
                "{n} faces whose triangles lie off the surface, {how} ({why}); the farthest for its tolerance: up to {} mm (chord tolerance {} mm, {at}){unmeasured}; bodies: {on}",
                worst.value, worst.tolerance
            )
        }
        Kind::OffCurve => format!(
            "{n} edges whose chords lie off the curve, past the chord tolerance (stopped by: {why}); the farthest for its tolerance: up to {} mm (chord tolerance {} mm, {at}); bodies: {on}",
            worst.value, worst.tolerance
        ),
    })
}

/// A compact reply's `warnings`: two or more lines of one kind become one line, and
/// `warning_count` keeps the number of lines the full reply has. Other warnings, and a kind with
/// one line, stay as they are. Nothing changes when no kind has two lines.
pub(crate) fn compact(o: &mut Map<String, Value>) {
    let Some(lines) = o.get("warnings").and_then(Value::as_array).cloned() else { return };
    let notes: Vec<Option<Note<'_>>> = lines.iter().map(|w| w.as_str().and_then(note)).collect();
    let group = |k: &Kind<'_>| notes.iter().flatten().filter(|n| n.kind == *k).collect::<Vec<_>>();
    if !notes.iter().flatten().any(|n| group(&n.kind).len() >= 2) {
        return;
    }
    let total = lines.len();
    let mut out: Vec<Value> = Vec::new();
    let mut done: Vec<Kind<'_>> = Vec::new();
    for (line, n) in lines.iter().zip(&notes) {
        let Some(n) = n else {
            out.push(line.clone());
            continue;
        };
        let same = group(&n.kind);
        if same.len() < 2 {
            out.push(line.clone());
            continue;
        }
        if done.contains(&n.kind) {
            continue;
        }
        done.push(n.kind.clone());
        // A group always has a worst line; were it to have none, its lines stay.
        match summary(&n.kind, &same) {
            Some(s) => out.push(Value::String(s)),
            None => out.extend(lines.iter().zip(&notes).filter(|(_, m)| m.as_ref().is_some_and(|m| m.kind == n.kind)).map(|(l, _)| l.clone())),
        }
    }
    o.insert("warnings".into(), Value::Array(out));
    o.insert("warning_count".into(), json!(total));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compacted(lines: &[&str]) -> Value {
        let mut o = Map::new();
        o.insert("warnings".into(), json!(lines));
        compact(&mut o);
        Value::Object(o)
    }

    /// A face past its chord tolerance is never counted with one within its tolerance, and each
    /// summary keeps the tolerance of the face it names (review of #18831).
    #[test]
    fn past_and_within_the_tolerance_stay_apart() {
        let past = |f: u32, sag: &str| {
            format!(
                "A: face {f}: its refinement stopped at a bound (Points). Its triangles lie up to {sag} mm off the surface, past it (chord tolerance 1.000e-3 mm)"
            )
        };
        let within = |f: u32, sag: &str, tol: &str| {
            format!(
                "B: face {f}: its refinement stopped at a bound (Points). Its triangles lie up to {sag} mm off the surface, still within it (chord tolerance {tol} mm, 3 middles unmeasured)"
            )
        };
        let lines = [past(1, "2.000e-3"), within(2, "9.000e-3", "5.000e-2"), past(3, "1.500e-3"), within(4, "1.000e-3", "2.000e-3")];
        let r = compacted(&lines.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(r["warning_count"], json!(4), "{r}");
        assert_eq!(
            r["warnings"],
            json!([
                "2 faces whose triangles lie off the surface, past it (its refinement stopped at a bound (Points)); the farthest for its tolerance: up to 2.000e-3 mm (chord tolerance 1.000e-3 mm, A: face 1); bodies: A (2)",
                "2 faces whose triangles lie off the surface, still within it (its refinement stopped at a bound (Points)); the farthest for its tolerance: up to 1.000e-3 mm (chord tolerance 2.000e-3 mm, B: face 4); 6 middles unmeasured; bodies: B (2)",
            ]),
            "{r}"
        );
        // One past line and one within line: nothing to count, both stay.
        let two = [past(1, "2.000e-3"), within(2, "9.000e-3", "5.000e-2")];
        let r = compacted(&two.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(r["warnings"], json!(two), "{r}");
        assert!(r.get("warning_count").is_none(), "{r}");
    }

    /// Lines from several bodies name every body and every bound that stopped the mesh.
    #[test]
    fn a_summary_names_every_body_and_bound() {
        let r = compacted(&[
            "Lid: face 1: smallest angle 20.00°, short of the 25° asked (stopped by: Corner)",
            "Base: face 9: smallest angle 3.00°, short of the 25° asked (stopped by: Points, Side)",
            "Base: face 10: smallest angle 4.00°, short of the 25° asked (stopped by: Points)",
        ]);
        assert_eq!(
            r["warnings"],
            json!([
                "3 faces short of the 25° asked (stopped by: Corner, Points, Side); the sharpest: smallest angle 3.00° (Base: face 9); bodies: Lid (1), Base (2)"
            ]),
            "{r}"
        );
        // Edges, from a URDF export whose lines lead with "body `Disc`".
        let r = compacted(&[
            "body `Disc`: edge 3: its sampling stopped at a bound (Points); its chords lie up to 1.000e-2 mm off the curve, past the 5.000e-3 mm chord tolerance",
            "body `Disc`: edge 4: its sampling stopped at a bound (Points); its chords lie up to 2.000e-2 mm off the curve, past the 5.000e-3 mm chord tolerance",
        ]);
        assert_eq!(
            r["warnings"],
            json!([
                "2 edges whose chords lie off the curve, past the chord tolerance (stopped by: Points); the farthest for its tolerance: up to 2.000e-2 mm (chord tolerance 5.000e-3 mm, body `Disc`: edge 4); bodies: body `Disc` (2)"
            ]),
            "{r}"
        );
    }

    /// A body whose name holds ": face " keeps its whole name; a number that isn't finite, or
    /// one written another way, leaves its line as it is.
    #[test]
    fn odd_names_and_numbers() {
        let r = compacted(&[
            "Top: face plate: face 1: smallest angle 10.00°, short of the 25° asked (stopped by: Corner)",
            "Top: face plate: face 2: smallest angle 11.00°, short of the 25° asked (stopped by: Corner)",
        ]);
        assert_eq!(
            r["warnings"],
            json!([
                "2 faces short of the 25° asked (stopped by: Corner); the sharpest: smallest angle 10.00° (Top: face plate: face 1); bodies: Top: face plate (2)"
            ]),
            "{r}"
        );
        for odd in [
            [
                "A: face 1: smallest angle NaN°, short of the 25° asked (stopped by: Corner)",
                "A: face 2: smallest angle -inf°, short of the 25° asked (stopped by: Corner)",
            ],
            [
                "A: face 1: smallest angle 12,94°, short of the 25° asked (stopped by: Corner)",
                "A: face 2: smallest angle 12,95°, short of the 25° asked (stopped by: Corner)",
            ],
            [
                "face plate: 1: smallest angle 10.00°, short of the 25° asked (stopped by: Corner)",
                "A: face x: smallest angle 11.00°, short of the 25° asked (stopped by: Corner)",
            ],
        ] {
            let r = compacted(&odd);
            assert_eq!(r["warnings"], json!(odd), "{r}");
            assert!(r.get("warning_count").is_none(), "{r}");
        }
        // A line that names no body.
        let r = compacted(&[
            "face 1: smallest angle 10.00°, short of the 25° asked (stopped by: Corner)",
            "face 2: smallest angle 9.00°, short of the 25° asked (stopped by: Corner)",
        ]);
        assert_eq!(
            r["warnings"],
            json!(["2 faces short of the 25° asked (stopped by: Corner); the sharpest: smallest angle 9.00° (face 2); bodies: no body named (2)"]),
            "{r}"
        );
    }
}
