//! A mesh export's `warnings` in a compact reply (#18831). A curved part can leave a hundred
//! faces short of the smallest angle, one line each. Lines of one kind become one line with how
//! many there were, the bodies they are on, what stopped the mesh and the worst of them.
//!
//! The lines are read in the exact wording `solidcraft_io` writes them in (`mesh_notes` in
//! `crates/io/src/lib.rs`). A line that doesn't match that wording, or holds a number that isn't
//! finite, is kept as it is: a changed wording makes the compact reply longer, never shorter
//! by a warning.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

/// What a line says, with the words that tell two groups apart. Only lines with the same kind
/// are counted together: a face past its chord tolerance is never counted with one within it.
#[derive(Clone, PartialEq, Eq, Hash)]
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

/// How the export writes a number: `{}` (an angle asked), `{:.2}` (an angle) or `{:.3e}` (a
/// distance).
#[derive(Clone, Copy)]
enum Format {
    Plain,
    TwoDecimals,
    Exponent,
}

/// "123" or "-123": digits with an optional minus sign. The rest of the text after them.
fn digits(t: &str) -> Option<(&str, &str)> {
    let body = t.strip_prefix('-').unwrap_or(t);
    let end = body.find(|c: char| !c.is_ascii_digit()).unwrap_or(body.len());
    (end > 0).then(|| body.split_at(end))
}

/// The number in `t` if it is written exactly as the export writes it in `format` and finite;
/// `None` for anything else ("+10", "1e1" for an angle, "NaN", "12,94").
fn number(t: &str, format: Format) -> Option<f64> {
    let (_, rest) = digits(t)?;
    let ok = match format {
        Format::Plain => rest.is_empty() || rest.strip_prefix('.').is_some_and(|f| !f.is_empty() && f.bytes().all(|c| c.is_ascii_digit())),
        Format::TwoDecimals => rest.strip_prefix('.').is_some_and(|f| f.len() == 2 && f.bytes().all(|c| c.is_ascii_digit())),
        Format::Exponent => rest
            .strip_prefix('.')
            .and_then(|f| f.split_once('e'))
            .is_some_and(|(f, e)| f.len() == 3 && f.bytes().all(|c| c.is_ascii_digit()) && digits(e).is_some_and(|(_, after)| after.is_empty())),
    };
    if !ok {
        return None;
    }
    t.parse::<f64>().ok().filter(|x| x.is_finite())
}

/// What the export writes for a face whose shaping no bound stopped.
const NOTHING_STOPPED: &str = "nothing stopped it";

/// The reason the export writes for a face off its surface whose shared boundary was kept.
const BOUNDARY_KEPT: &str = "its shared boundary was kept";

/// Whether `reason` is one the export writes before ". Its triangles lie up to":
/// [`BOUNDARY_KEPT`] or "its refinement stopped at a bound (...)" with the planner's bounds
/// inside, as `caps` reads them.
fn surface_reason(reason: &str) -> bool {
    reason == BOUNDARY_KEPT || reason.strip_prefix("its refinement stopped at a bound (").and_then(|r| r.strip_suffix(')')).is_some_and(caps)
}

/// Whether `t` is the planner's bounds as the export writes them (`{:?}` of each, joined by
/// ", "): `Refine` or `GridThinned { asked: [N, N], kept: [N, N] }`, at least one. A bound in
/// any other wording leaves its line unfolded.
fn caps(mut t: &str) -> bool {
    /// A count (digits); the rest of the text after it.
    fn count(t: &str) -> Option<&str> {
        let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
        (end > 0).then(|| t.get(end..)).flatten()
    }
    /// "N, N" (two counts); the rest of the text after them.
    fn pair(t: &str) -> Option<&str> {
        count(count(t)?.strip_prefix(", ")?)
    }
    loop {
        let rest = match t.strip_prefix("Refine") {
            Some(r) => Some(r),
            None => t
                .strip_prefix("GridThinned { asked: [")
                .and_then(pair)
                .and_then(|r| r.strip_prefix("], kept: ["))
                .and_then(pair)
                .and_then(|r| r.strip_prefix("] }")),
        };
        match rest {
            Some("") => return true,
            Some(r) => match r.strip_prefix(", ") {
                Some(r) => t = r,
                None => return false,
            },
            None => return false,
        }
    }
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
        let (s, t) = (number(sag, Format::Exponent)?, number(tol, Format::Exponent)?);
        return Some(base(Kind::OffCurve, list(cap), sag, tol, if t > 0.0 { s / t } else { s }, 0));
    }
    if let Some(r) = rest.strip_prefix("smallest angle ") {
        // smallest angle {x}°, short of the {asked}° asked (stopped by: {limits})
        let (angle, r) = r.split_once("°, short of the ")?;
        let (asked, r) = r.split_once("° asked (stopped by: ")?;
        let limits = r.strip_suffix(')')?;
        number(asked, Format::Plain)?;
        return Some(base(Kind::ShortOfAngle(asked), list(limits), angle, "", number(angle, Format::TwoDecimals)?, 0));
    }
    if let Some(r) = rest.strip_prefix("its shaping stopped at a bound (") {
        // its shaping stopped at a bound ({limits}) with the {asked}° asked reached (smallest angle {x}°)
        let (limits, r) = r.split_once(") with the ")?;
        let (asked, r) = r.split_once("° asked reached (smallest angle ")?;
        let angle = r.strip_suffix("°)")?;
        number(asked, Format::Plain)?;
        return Some(base(Kind::BoundReached(asked), list(limits), angle, "", number(angle, Format::TwoDecimals)?, 0));
    }
    // {reason}. Its triangles lie up to {sag} mm off the surface, {how} (chord tolerance {tol} mm[, {n} middles unmeasured])
    let (reason, r) = rest.split_once(". Its triangles lie up to ")?;
    if !surface_reason(reason) {
        return None;
    }
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
    let (s, t) = (number(sag, Format::Exponent)?, number(tol, Format::Exponent)?);
    Some(base(Kind::OffSurface(how), vec![reason], sag, tol, if t > 0.0 { s / t } else { s }, unmeasured))
}

/// A body's name quoted as a JSON string, as the bodies list and the worst line's location
/// write it.
fn quoted(body: &str) -> String {
    Value::String(body.to_string()).to_string()
}

/// The bodies of `notes` in the order they first appear, each with how many lines it has:
/// `"Base" (2), "Lid" (1)`. Names are quoted as JSON strings, so a name holding " (" or ", "
/// can't be mistaken for two bodies or a count.
fn bodies(notes: &[&Note<'_>]) -> String {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for n in notes {
        #[cfg(test)]
        tests::LOOKUPS.with(|c| c.set(c.get() + 1));
        let i = *index.entry(n.body).or_insert_with(|| {
            seen.push((n.body, 0));
            seen.len() - 1
        });
        if let Some((_, c)) = seen.get_mut(i) {
            *c += 1;
        }
    }
    seen.iter()
        .map(|(b, c)| if b.is_empty() { format!("no body named ({c})") } else { format!("{} ({c})", quoted(b)) })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The reasons of `notes`, each once, in the order they first appear.
fn reasons(notes: &[&Note<'_>]) -> String {
    let mut all: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for r in notes.iter().flat_map(|n| n.reasons.iter()).filter(|r| **r != NOTHING_STOPPED) {
        #[cfg(test)]
        tests::LOOKUPS.with(|c| c.set(c.get() + 1));
        if seen.insert(r) {
            all.push(r);
        }
    }
    all.join(", ")
}

/// What stopped the shaping of the faces of an angle group: "stopped by: Corner, Points", with
/// "nothing stopped 1 face" for the faces no bound stopped, or "nothing stopped them".
fn stopped(notes: &[&Note<'_>]) -> String {
    let free = notes.iter().filter(|n| n.reasons.iter().all(|r| *r == NOTHING_STOPPED)).count();
    let bounds = reasons(notes);
    let faces = if free == 1 { "1 face".to_string() } else { format!("{free} faces") };
    match (bounds.is_empty(), free) {
        (true, _) => "nothing stopped them".into(),
        (false, 0) => format!("stopped by: {bounds}"),
        (false, _) => format!("stopped by: {bounds}; nothing stopped {faces}"),
    }
}

/// The one line that stands for `group` (two or more lines of one kind).
fn summary(kind: &Kind<'_>, group: &[&Note<'_>]) -> Option<String> {
    let worst = match kind {
        Kind::ShortOfAngle(_) | Kind::BoundReached(_) => group.iter().min_by(|a, b| a.badness.total_cmp(&b.badness)),
        Kind::OffSurface(_) | Kind::OffCurve => group.iter().max_by(|a, b| a.badness.total_cmp(&b.badness)),
    }?;
    // Quoted like the bodies list, so a name ending in ": face 9" can't be read as the face.
    let at = if worst.body.is_empty() { worst.item.clone() } else { format!("{}: {}", quoted(worst.body), worst.item) };
    let (n, why, on) = (group.len(), reasons(group), bodies(group));
    Some(match kind {
        Kind::ShortOfAngle(asked) => {
            format!("{n} faces short of the {asked}° asked ({}); the sharpest: smallest angle {}° ({at}); bodies: {on}", stopped(group), worst.value)
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

/// The lines of each kind, in the order the kinds first appear, each as the indices of its lines
/// in `notes`; and the group of each line (`None` for a line that is no mesh warning). Built in
/// one pass over the lines. With the bodies and reasons also counted through maps, a reply's
/// warnings take time in proportion to their length, however many kinds, bodies or reasons.
fn groups<'a>(notes: &[Option<Note<'a>>]) -> (Vec<Vec<usize>>, Vec<Option<usize>>) {
    let mut index: HashMap<&Kind<'a>, usize> = HashMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut of_line: Vec<Option<usize>> = Vec::new();
    for (i, n) in notes.iter().enumerate() {
        #[cfg(test)]
        tests::LOOKUPS.with(|c| c.set(c.get() + 1));
        of_line.push(n.as_ref().map(|n| {
            let g = *index.entry(&n.kind).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            if let Some(lines) = groups.get_mut(g) {
                lines.push(i);
            }
            g
        }));
    }
    (groups, of_line)
}

/// A compact reply's `warnings`: two or more lines of one kind become one line, and
/// `warning_count` keeps the number of lines the full reply has. Other warnings, and a kind with
/// one line, stay as they are. Nothing changes when no kind has two lines.
pub(crate) fn compact(o: &mut Map<String, Value>) {
    let Some(lines) = o.get("warnings").and_then(Value::as_array).cloned() else { return };
    let notes: Vec<Option<Note<'_>>> = lines.iter().map(|w| w.as_str().and_then(note)).collect();
    let (groups, of_line) = groups(&notes);
    if !groups.iter().any(|g| g.len() >= 2) {
        return;
    }
    let total = lines.len();
    let mut out: Vec<Value> = Vec::new();
    let mut done = vec![false; groups.len()];
    for ((line, n), g) in lines.iter().zip(&notes).zip(&of_line) {
        let (Some(n), Some(g)) = (n, g) else {
            out.push(line.clone());
            continue;
        };
        let members = groups.get(*g).map_or(&[][..], Vec::as_slice);
        if members.len() < 2 {
            out.push(line.clone());
            continue;
        }
        // The summary goes where the group's first line was; its other lines leave nothing.
        match done.get_mut(*g) {
            Some(d) if !*d => *d = true,
            _ => continue,
        }
        #[cfg(test)]
        tests::LOOKUPS.with(|c| c.set(c.get() + members.len()));
        let same: Vec<&Note<'_>> = members.iter().filter_map(|&i| notes.get(i).and_then(Option::as_ref)).collect();
        // A group always has a worst line; were it to have none, its lines stay.
        match summary(&n.kind, &same) {
            Some(s) => out.push(Value::String(s)),
            None => out.extend(members.iter().filter_map(|&i| lines.get(i)).cloned()),
        }
    }
    o.insert("warnings".into(), Value::Array(out));
    o.insert("warning_count".into(), json!(total));
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        /// How many steps `compact` has taken: reading a line to place it in its group or to
        /// write its group's summary, and finding a body or a reason among those already seen.
        /// Each step is one map lookup at most, never a scan of what came before.
        pub(super) static LOOKUPS: Cell<usize> = const { Cell::new(0) };
    }

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
                "A: face {f}: its refinement stopped at a bound (Refine). Its triangles lie up to {sag} mm off the surface, past it (chord tolerance 1.000e-3 mm)"
            )
        };
        let within = |f: u32, sag: &str, tol: &str| {
            format!(
                "B: face {f}: its refinement stopped at a bound (GridThinned {{ asked: [400, 300], kept: [200, 150] }}). Its triangles lie up to {sag} mm off the surface, still within it (chord tolerance {tol} mm, 3 middles unmeasured)"
            )
        };
        let lines = [past(1, "2.000e-3"), within(2, "9.000e-3", "5.000e-2"), past(3, "1.500e-3"), within(4, "1.000e-3", "2.000e-3")];
        let r = compacted(&lines.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(r["warning_count"], json!(4), "{r}");
        assert_eq!(
            r["warnings"],
            json!([
                "2 faces whose triangles lie off the surface, past it (its refinement stopped at a bound (Refine)); the farthest for its tolerance: up to 2.000e-3 mm (chord tolerance 1.000e-3 mm, \"A\": face 1); bodies: \"A\" (2)",
                "2 faces whose triangles lie off the surface, still within it (its refinement stopped at a bound (GridThinned { asked: [400, 300], kept: [200, 150] })); the farthest for its tolerance: up to 1.000e-3 mm (chord tolerance 2.000e-3 mm, \"B\": face 4); 6 middles unmeasured; bodies: \"B\" (2)",
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
                "3 faces short of the 25° asked (stopped by: Corner, Points, Side); the sharpest: smallest angle 3.00° (\"Base\": face 9); bodies: \"Lid\" (1), \"Base\" (2)"
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
                "2 edges whose chords lie off the curve, past the chord tolerance (stopped by: Points); the farthest for its tolerance: up to 2.000e-2 mm (chord tolerance 5.000e-3 mm, \"body `Disc`\": edge 4); bodies: \"body `Disc`\" (2)"
            ]),
            "{r}"
        );
    }

    /// Body names holding " (" or ", " stay readable: each name is quoted (review of #18831).
    #[test]
    fn body_names_are_quoted() {
        let r = compacted(&[
            "Lid (1): face 1: smallest angle 10.00°, short of the 25° asked (stopped by: Corner)",
            "Lid, Base: face 2: smallest angle 11.00°, short of the 25° asked (stopped by: Corner)",
            "Lid: face 3: smallest angle 12.00°, short of the 25° asked (stopped by: Corner)",
            r#"Say "hi": face 4: smallest angle 13.00°, short of the 25° asked (stopped by: Corner)"#,
        ]);
        assert_eq!(
            r["warnings"],
            json!([
                r#"4 faces short of the 25° asked (stopped by: Corner); the sharpest: smallest angle 10.00° ("Lid (1)": face 1); bodies: "Lid (1)" (1), "Lid, Base" (1), "Lid" (1), "Say \"hi\"" (1)"#
            ]),
            "{r}"
        );
    }

    /// A face no bound stopped is counted on its own, not listed as a bound.
    #[test]
    fn faces_nothing_stopped_are_counted() {
        let line = |f: u32, by: &str| format!("A: face {f}: smallest angle 1{f}.00°, short of the 25° asked (stopped by: {by})");
        let r = compacted(&[&line(1, "nothing stopped it"), &line(2, "Corner"), &line(3, "Corner, Points")]);
        assert_eq!(
            r["warnings"],
            json!([
                r#"3 faces short of the 25° asked (stopped by: Corner, Points; nothing stopped 1 face); the sharpest: smallest angle 11.00° ("A": face 1); bodies: "A" (3)"#
            ]),
            "{r}"
        );
        let r = compacted(&[&line(1, "nothing stopped it"), &line(2, "nothing stopped it")]);
        assert_eq!(
            r["warnings"],
            json!([r#"2 faces short of the 25° asked (nothing stopped them); the sharpest: smallest angle 11.00° ("A": face 1); bodies: "A" (2)"#]),
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
                "2 faces short of the 25° asked (stopped by: Corner); the sharpest: smallest angle 10.00° (\"Top: face plate\": face 1); bodies: \"Top: face plate\" (2)"
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
                "A: face 1: smallest angle +10.00°, short of the 25° asked (stopped by: Corner)",
                "A: face 2: smallest angle 1e1°, short of the 25° asked (stopped by: Corner)",
            ],
            [
                "A: face 1: smallest angle 10.0°, short of the +25° asked (stopped by: Corner)",
                "A: face 2: smallest angle 10.00°, short of the 2.5e1° asked (stopped by: Corner)",
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

    /// A surface line whose reason is not one the export writes stays as it is, while the two
    /// reasons it does write are still counted (#19250).
    #[test]
    fn a_surface_line_with_an_unknown_reason_stays() {
        let line = |f: u32, reason: &str| {
            format!("A: face {f}: {reason}. Its triangles lie up to 2.000e-3 mm off the surface, past it (chord tolerance 1.000e-3 mm)")
        };
        for reason in [
            "its boundary was moved",
            "its refinement stopped at a bound ()",
            "Its shared boundary was kept",
            "its refinement stopped at a bound (Points",
            "its refinement stopped at a bound (Points)",
            "its refinement stopped at a bound (Refined)",
            "its refinement stopped at a bound (Refine,)",
            "its refinement stopped at a bound (Refine; Refine)",
            "its refinement stopped at a bound (GridThinned { asked: [400, 300] })",
            "its refinement stopped at a bound (GridThinned { asked: [400, -300], kept: [200, 150] })",
            "its refinement stopped at a bound (GridThinned { asked: [400 300], kept: [200 150] })",
        ] {
            let odd = [line(1, reason), line(2, reason)];
            let r = compacted(&odd.iter().map(String::as_str).collect::<Vec<_>>());
            assert_eq!(r["warnings"], json!(odd), "{r}");
            assert!(r.get("warning_count").is_none(), "{r}");
        }
        // Each bound the planner writes, alone or joined, is counted.
        for reason in [
            "its refinement stopped at a bound (Refine)",
            "its refinement stopped at a bound (GridThinned { asked: [400, 300], kept: [200, 150] })",
            "its refinement stopped at a bound (GridThinned { asked: [400, 300], kept: [200, 150] }, Refine)",
        ] {
            let r = compacted(&[&line(1, reason), &line(2, reason)]);
            assert_eq!(
                r["warnings"],
                json!([format!(
                    r#"2 faces whose triangles lie off the surface, past it ({reason}); the farthest for its tolerance: up to 2.000e-3 mm (chord tolerance 1.000e-3 mm, "A": face 2); bodies: "A" (2)"#
                )]),
                "{r}"
            );
        }
        // The exact reasons are counted; an unknown one beside them stays on its own line.
        let lines = [line(1, "its shared boundary was kept"), line(2, "its boundary was moved"), line(3, "its shared boundary was kept")];
        let r = compacted(&lines.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(r["warning_count"], json!(3), "{r}");
        assert_eq!(
            r["warnings"],
            json!([
                r#"2 faces whose triangles lie off the surface, past it (its shared boundary was kept); the farthest for its tolerance: up to 2.000e-3 mm (chord tolerance 1.000e-3 mm, "A": face 3); bodies: "A" (2)"#,
                lines[1],
            ]),
            "{r}"
        );
    }

    /// The worst line's location quotes the body name like the bodies list, so a name that ends
    /// in ": face 9" can't be read as the face (#19250).
    #[test]
    fn the_worst_location_quotes_the_body_name() {
        let r = compacted(&[
            "Lid: face 9: face 2: smallest angle 10.00°, short of the 25° asked (stopped by: Corner)",
            "Lid (rev A): face 3: smallest angle 11.00°, short of the 25° asked (stopped by: Corner)",
        ]);
        assert_eq!(
            r["warnings"],
            json!([
                r#"2 faces short of the 25° asked (stopped by: Corner); the sharpest: smallest angle 10.00° ("Lid: face 9": face 2); bodies: "Lid: face 9" (1), "Lid (rev A)" (1)"#
            ]),
            "{r}"
        );
        let r = compacted(&[
            "Lid (rev A): face 3: smallest angle 11.00°, short of the 25° asked (stopped by: Corner)",
            "Lid (rev A): face 4: smallest angle 10.50°, short of the 25° asked (stopped by: Corner)",
        ]);
        assert_eq!(
            r["warnings"],
            json!([
                r#"2 faces short of the 25° asked (stopped by: Corner); the sharpest: smallest angle 10.50° ("Lid (rev A)": face 4); bodies: "Lid (rev A)" (2)"#
            ]),
            "{r}"
        );
    }

    /// 10,000 warnings are folded in steps in proportion to their number, whether they are of
    /// three kinds on one body, each on a body of its own, or each with a reason of its own
    /// (#19250). A step reads one line or looks up one body or reason; none scans the others.
    #[test]
    fn ten_thousand_lines_are_grouped_in_one_pass() {
        let steps = |lines: &[String]| {
            LOOKUPS.with(|c| c.set(0));
            let r = compacted(&lines.iter().map(String::as_str).collect::<Vec<_>>());
            (r, LOOKUPS.with(Cell::get))
        };
        // Three kinds on one body.
        let mut lines = vec![String::from("Part has a print face but was written as it lies")];
        for i in 0..10_000u32 {
            lines.push(match i % 3 {
                0 => format!("Part: face {i}: smallest angle {:.2}°, short of the 25° asked (stopped by: Corner)", 10.0 + f64::from(i % 997) / 100.0),
                1 => format!(
                    "Part: face {i}: its refinement stopped at a bound (Refine). Its triangles lie up to {} mm off the surface, past it (chord tolerance 1.000e-3 mm)",
                    if i == 4999 { "9.000e-3" } else { "2.000e-3" }
                ),
                _ => format!(
                    "Part: edge {i}: its sampling stopped at a bound (Depth); its chords lie up to {} mm off the curve, past the 5.000e-3 mm chord tolerance",
                    if i == 7001 { "8.000e-2" } else { "1.000e-2" }
                ),
            });
        }
        let (r, n) = steps(&lines);
        assert_eq!(r["warning_count"], json!(10_001));
        assert_eq!(
            r["warnings"],
            json!([
                "Part has a print face but was written as it lies",
                r#"3334 faces short of the 25° asked (stopped by: Corner); the sharpest: smallest angle 10.00° ("Part": face 0); bodies: "Part" (3334)"#,
                r#"3333 faces whose triangles lie off the surface, past it (its refinement stopped at a bound (Refine)); the farthest for its tolerance: up to 9.000e-3 mm (chord tolerance 1.000e-3 mm, "Part": face 4999); bodies: "Part" (3333)"#,
                r#"3333 edges whose chords lie off the curve, past the chord tolerance (stopped by: Depth); the farthest for its tolerance: up to 8.000e-2 mm (chord tolerance 5.000e-3 mm, "Part": edge 7001); bodies: "Part" (3333)"#,
            ])
        );
        // Each line read to group it and again for its summary, each body looked up once, and
        // each reason once for the summary (twice for an angle line, whose bounds are also
        // listed under "stopped by").
        assert_eq!(n, 10_001 + 10_000 + 10_000 + 3334 * 2 + 3333 + 3333);
        // 10,000 lines, each on a body of its own.
        let lines: Vec<String> = (0..10_000u32)
            .map(|i| format!("B{i}: face 1: smallest angle {:.2}°, short of the 25° asked (stopped by: Corner)", 10.0 + f64::from(i % 997) / 100.0))
            .collect();
        let (r, n) = steps(&lines);
        let w = r["warnings"].as_array().cloned().unwrap_or_default();
        assert_eq!(w.len(), 1, "{r}");
        let w = w[0].as_str().unwrap_or_default();
        assert!(w.starts_with(r#"10000 faces short of the 25° asked (stopped by: Corner); the sharpest: smallest angle 10.00° ("B0": face 1); bodies: "B0" (1), "B1" (1), "#), "{w:.300}");
        assert!(w.ends_with(r#", "B9998" (1), "B9999" (1)"#), "{w:.300}");
        assert_eq!(n, 10_000 * 5);
        // 10,000 surface lines, each with a thinned grid of its own.
        let lines: Vec<String> = (0..10_000u32)
            .map(|i| {
                format!(
                    "Part: face {i}: its refinement stopped at a bound (GridThinned {{ asked: [{i}, 300], kept: [200, 150] }}). Its triangles lie up to 2.000e-3 mm off the surface, past it (chord tolerance 1.000e-3 mm)"
                )
            })
            .collect();
        let (r, n) = steps(&lines);
        let w = r["warnings"].as_array().cloned().unwrap_or_default();
        assert_eq!(w.len(), 1, "{r}");
        let w = w[0].as_str().unwrap_or_default();
        assert!(
            w.starts_with("10000 faces whose triangles lie off the surface, past it (its refinement stopped at a bound (GridThinned { asked: [0, 300], kept: [200, 150] }), its refinement stopped at a bound (GridThinned { asked: [1, 300]"),
            "{w:.300}"
        );
        assert!(w.ends_with(r#"(GridThinned { asked: [9999, 300], kept: [200, 150] })); the farthest for its tolerance: up to 2.000e-3 mm (chord tolerance 1.000e-3 mm, "Part": face 9999); bodies: "Part" (10000)"#), "{w:.300}");
        assert_eq!(n, 10_000 * 4);
    }
}
