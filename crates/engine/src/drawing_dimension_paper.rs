//! A drawing's dimensions on paper (#4896 W19g, #4894 WE6): one group for each dimension, its
//! extension and dimension lines and its arrowheads, laid out by [`dim_layout_sized`] in paper
//! millimetres, so a dimension is the same size at any view scale. [`paper_group`] lays one out
//! centred on its view, as a view's lines are, so moving the view moves it unchanged.
//!
//! Its text is set in the drawing font ([`paper_text`]) and measured there, so the arrows go inside
//! or outside by its real length. A text the font can't set leaves the lines and says why. An
//! ordinate set has no dimension lines: [`ordinate`] lays out its leaders and numbers itself.

use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use solidcore_record::{Seg2, Vec2};
use solidcraft_doc::drawing::dimension::{Dimension, DimensionKind, LabelKind, Resolved, TextPlace};
use solidcraft_geom::drawing_measure::LinearDirection;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::paper::annotate::{ArrowShape, Terminator};
use solidcraft_geom::paper::line::{LineGroup, LineKind};
use solidcraft_geom::paper::text::{Font, TextRun};
use solidcraft_geom::paper::{Group, Item, ObjectKind};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{DimFrame, DimSizes, Outcome, PaperFonts, TextStyle, dim_layout_sized};

/// A dimension's sizes on paper, with its text above an unbroken line (ISO 129-1).
const SIZES: DimSizes = DimSizes { arrow: 3.0, gap: 1.0, overshoot: 2.0, offset: 10.0, above: true };
/// A filled arrowhead, its line stopped at its base by [`stop_at_base`]: a note's leader ends in it too.
pub(crate) const ARROW: Terminator = Terminator::Arrow { shape: ArrowShape::Filled, length_mm: SIZES.arrow, angle: PI / 9.0 };
/// The height of a dimension's capitals, and a note's unless it sets its own.
pub(crate) const TEXT_MM: f64 = 3.5;
/// How close two points of a layout are when they are one point.
const SAME_POINT_MM: f64 = 1e-6;
/// How far apart a baseline set's dimension lines stand on paper: room for a 3.5 mm text and its
/// gaps. Until the drafting profile (#4896, W09d) gives it.
const STACK_MM: f64 = 7.0;
/// How far short of the common line an ordinate leader whose number was moved turns towards it.
const JOG_MM: f64 = 2.0;

/// One laid-out piece of a dimension: a chain and a baseline set have one for each of their members.
/// An ordinate set has none ([`ordinate`]).
struct Piece {
    frame: DimFrame,
    /// Where the layout puts its text, from the frame's own origin.
    at: Vec2,
    text: Text,
}

/// A text set once and measured, upright from the sheet's origin, for a layout to place
/// ([`placed`]): a dimension's, or a note's.
pub(crate) struct Text {
    pub(crate) runs: Vec<TextRun>,
    pub(crate) fonts: Vec<Font>,
    /// Half its size.
    pub(crate) half: Vec2,
    /// Why it isn't set, when it can't be.
    pub(crate) why: Option<String>,
    /// Whether it is unset only until the search for a font to fall back to answers.
    pub(crate) pending: bool,
}

impl Text {
    /// `text` in the drawing's `lettering`, its capitals `cap_mm` high.
    pub(crate) fn set(text: Option<&str>, cap_mm: f64, lettering: &PaperFonts) -> Text {
        let unset = |why, pending| Text { runs: Vec::new(), fonts: Vec::new(), half: Vec2::new(0.0, cap_mm * 0.5), why, pending };
        match text.map(|text| Outcome::from(lettering.text(text, Vec2::ZERO, cap_mm, 0.0, &TextStyle::default()))) {
            None | Some(Outcome::Set(None)) => unset(None, false),
            Some(Outcome::Pending) => unset(None, true),
            Some(Outcome::Refused(why)) => unset(Some(why), false),
            Some(Outcome::Set(Some((runs, fonts)))) => {
                let width = runs.iter().map(TextRun::reach).fold(0.0, f64::max);
                // Its lines stand below the first one's baseline.
                let below = runs.iter().map(|run| -run.origin.y).fold(0.0, f64::max);
                Text { runs, fonts, half: Vec2::new(width, cap_mm + below) * 0.5, why: None, pending: false }
            }
        }
    }
}

/// A dimension's lines, arrowheads and texts as its layout gives them, before its lines are stroked.
#[derive(Default)]
struct Drawn {
    lines: Vec<Vec<Vec2>>,
    /// Each one's tip and the unit direction it points in.
    arrows: Vec<(Vec2, Vec2)>,
    texts: Vec<Item>,
    fonts: Vec<Font>,
    why: Option<String>,
    pending: bool,
}

impl Drawn {
    /// `line`, unless a line already drawn covers it; a line it covers goes. A chain's neighbours
    /// share an extension line, and a set's lines from its origin lie along each other.
    fn line(&mut self, line: Vec<Vec2>) {
        if !self.lines.iter().any(|other| covered(&line, other)) {
            self.lines.retain(|other| !covered(other, &line));
            self.lines.push(line);
        }
    }

    /// `text` centred on `centre`, running along `dir`.
    fn text(&mut self, text: Text, centre: Vec2, dir: Vec2) {
        self.texts.extend(placed(text.runs, text.half, TEXT_MM, centre, dir));
        self.fonts.extend(text.fonts);
        self.why = self.why.take().or(text.why);
        self.pending |= text.pending;
    }
}

/// A dimension on paper.
pub struct OnPaper {
    pub group: Group,
    /// The fonts its text uses, in no order, the same one perhaps more than once.
    pub fonts: Vec<Font>,
    /// Why its text isn't there, when it can't be set.
    pub why: Option<String>,
    /// Whether its text isn't there only until the search for a font to fall back to answers.
    pub pending: bool,
}

/// `dimension`, read as `resolved`, on the view through `frame` (centred on the view), its lines in
/// `group` and its text in the drawing's `lettering`: its group, or `None` when what it measures has no place to draw on the
/// view: it wasn't found again, or it is a single point there.
pub fn paper_group(dimension: &Dimension, resolved: &Resolved, frame: &ViewFrame, group: LineGroup, lettering: &PaperFonts) -> Option<OnPaper> {
    let line_mm = LineKind::Thin.width_mm(group);
    let on: Vec<Vec2> = resolved.points.iter().map(|p| frame.to_paper(*p)).collect();
    let set = |text: Option<&str>| Text::set(text, TEXT_MM, lettering);
    let mut drawn = Drawn::default();
    if let DimensionKind::Ordinate { direction, .. } = &dimension.kind {
        ordinate(&on, *direction, dimension.offset_mm, resolved, &set, &mut drawn)?;
    } else {
        for piece in pieces(dimension, resolved, &on, &set)? {
            let layout = dim_layout_sized(&piece.frame, Some(piece.at), &SIZES, piece.text.half, [None, None]);
            drawn.text(piece.text, layout.text, layout.text_dir);
            layout.lines.into_iter().for_each(|line| drawn.line(line));
            drawn.arrows.extend(layout.arrows);
        }
    }
    let Drawn { mut lines, arrows, texts, fonts, why, pending } = drawn;
    for (tip, dir) in &arrows {
        lines.iter_mut().for_each(|line| stop_at_base(line, *tip, *dir));
    }
    let mut items: Vec<Item> = lines.iter().filter_map(|line| stroke(line, line_mm)).collect();
    for (tip, dir) in &arrows {
        items.extend(ARROW.place(*tip, *tip - *dir, line_mm).ok()?.0);
    }
    items.extend(texts);
    Some(OnPaper { group: Group { object: dimension.id, kind: ObjectKind::Dimension, items }, fonts, why, pending })
}

/// `runs` of a text `half` in size, its capitals `cap_mm` high, centred on `centre` and running along
/// `dir`.
pub(crate) fn placed(runs: Vec<TextRun>, half: Vec2, cap_mm: f64, centre: Vec2, dir: Vec2) -> impl Iterator<Item = Item> {
    let up = dir.perp();
    let start = centre - dir * half.x + up * (half.y - cap_mm);
    runs.into_iter().map(move |run| Item::Text(TextRun { origin: start + dir * run.origin.x + up * run.origin.y, angle: dir.angle(), ..run }))
}

/// What `dimension` lays out, from what it measures on paper (`on`, as [`Resolved::points`] orders
/// them), each text set by `set`.
fn pieces(dimension: &Dimension, resolved: &Resolved, on: &[Vec2], set: &impl Fn(Option<&str>) -> Text) -> Option<Vec<Piece>> {
    let (offset, place) = (dimension.offset_mm, &dimension.text);
    let text = || set(resolved.text.as_deref());
    match (&dimension.kind, on) {
        (DimensionKind::Linear { direction, .. } | DimensionKind::Length { direction, .. }, &[a, b]) => {
            let dir = line_direction(*direction, b - a)?;
            Some(vec![linear(a, b, dir, offset, place, text())])
        }
        (DimensionKind::Angle { other_side, .. }, &[a0, a1, b0, b1]) => Some(vec![angular([a0, a1], [b0, b1], *other_side, offset, place, text())?]),
        // As an angle's, between its silhouettes running out from the apex, whatever its size: the
        // larger side when they are more than a right angle apart.
        (DimensionKind::Cone { .. }, &[a0, a1, b0, b1]) => {
            Some(vec![angular([a0, a1], [b0, b1], (a1 - a0).dot(b1 - b0) < 0.0, offset, place, text())?])
        }
        (DimensionKind::Radial { diameter, .. }, &[centre, start, mid, end]) => {
            Some(vec![radial(centre, [start, mid, end], *diameter, offset, place, text())?])
        }
        (DimensionKind::Chain { direction, .. }, points) => chain(points, *direction, offset, resolved, set),
        (DimensionKind::Baseline { direction, .. }, points) => baseline(points, *direction, offset, resolved, set),
        // As a circle's or an arc's, on the arc of its contour it reads on.
        (DimensionKind::Sphere { .. }, &[centre, start, mid, end]) => {
            let diameter = resolved.label == Some(LabelKind::SphericalDiameter);
            Some(vec![radial(centre, [start, mid, end], diameter, offset, place, text())?])
        }
        (DimensionKind::Cylinder { .. }, &[a0, a1, b0, b1]) => across([a0, a1], [b0, b1], LinearDirection::Aligned, offset, place, text()),
        (DimensionKind::Silhouettes { direction, .. } | DimensionKind::Faces { direction, .. }, &[a0, a1, b0, b1]) => {
            across([a0, a1], [b0, b1], *direction, offset, place, text())
        }
        _ => None,
    }
}

/// The unit direction a linear dimension's line runs in, on paper. Turned to run right, or up when
/// it is upright, so a positive offset is above it or to its left whichever way round its points are.
pub(crate) fn line_direction(direction: LinearDirection, seen: Vec2) -> Option<Vec2> {
    let dir = match direction {
        LinearDirection::Horizontal => Vec2::X,
        LinearDirection::Vertical => Vec2::new(0.0, 1.0),
        LinearDirection::Aligned => seen.normalized()?,
        // A view's paper map only scales and shifts, so its plane's directions are paper's.
        LinearDirection::Along(along) => along.normalized()?,
    };
    let forward = if dir.x.abs() > 1e-9 { dir.x > 0.0 } else { dir.y > 0.0 };
    Some(if forward { dir } else { -dir })
}

/// A line from `a` to `b` along `dir`, `across` from their middle.
fn linear(a: Vec2, b: Vec2, dir: Vec2, across: f64, place: &TextPlace, text: Text) -> Piece {
    let along = at_of(place).map_or(0.0, |at| (at - (a + b) * 0.5).dot(dir));
    Piece { frame: DimFrame::Linear { p0: a, p1: b, dir }, at: Vec2::new(along, across), text }
}

/// A chain's members, each a line from one point to the next, all on one line `offset` from the
/// middle of the chain's two ends, each member's text set by `set`.
fn chain(points: &[Vec2], direction: LinearDirection, offset: f64, resolved: &Resolved, set: &impl Fn(Option<&str>) -> Text) -> Option<Vec<Piece>> {
    let (first, last) = (*points.first()?, *points.last()?);
    let dir = line_direction(direction, last - first)?;
    let n = dir.perp();
    let line = ((first + last) * 0.5).dot(n) + offset;
    let member = |(i, (a, b)): (usize, (&Vec2, &Vec2))| {
        let text = resolved.members.get(i).map(|m| m.text.as_str());
        linear(*a, *b, dir, line - ((*a + *b) * 0.5).dot(n), &TextPlace::Centred, set(text))
    };
    Some(points.iter().zip(points.iter().skip(1)).enumerate().map(member).collect())
}

/// A baseline set's members, each a line from the origin (`points`' first) to one target, stacked
/// [`STACK_MM`] apart, nearest target first, from `offset` past the outermost point on its side, so
/// every line clears what it measures. Targets that read the same keep their order.
fn baseline(
    points: &[Vec2],
    direction: LinearDirection,
    offset: f64,
    resolved: &Resolved,
    set: &impl Fn(Option<&str>) -> Text,
) -> Option<Vec<Piece>> {
    let (origin, targets) = points.split_first()?;
    let dir = line_direction(direction, *targets.last()? - *origin)?;
    let n = dir.perp();
    let side = if offset < 0.0 { -1.0 } else { 1.0 };
    let outer = side * points.iter().map(|p| side * p.dot(n)).fold(f64::NEG_INFINITY, f64::max);
    let value = |i: usize| resolved.members.get(i).map_or(0.0, |m| m.measured.value);
    let mut nearest_first: Vec<usize> = (0..targets.len()).collect();
    nearest_first.sort_by(|a, b| value(*a).total_cmp(&value(*b)));
    let member = |(k, i): (usize, usize)| {
        let target = *targets.get(i)?;
        let line = outer + offset + side * STACK_MM * k as f64;
        let text = resolved.members.get(i).map(|m| m.text.as_str());
        Some(linear(*origin, target, dir, line - ((*origin + target) * 0.5).dot(n), &TextPlace::Centred, set(text)))
    };
    nearest_first.into_iter().enumerate().map(member).collect()
}

/// An ordinate set: the origin (the first of `points`), shown as 0, and each target, each with a
/// leader from a gap off its point out to one line `offset` past the outermost point on its side,
/// and its number past the leader's end, reading along it. Numbers that would overlap along the
/// set's line are moved apart as little as can be ([`spread`]); the leader of one that moved runs
/// straight to [`JOG_MM`] short of the line, then on a slant to its number.
fn ordinate(
    points: &[Vec2],
    direction: LinearDirection,
    offset: f64,
    resolved: &Resolved,
    set: &impl Fn(Option<&str>) -> Text,
    out: &mut Drawn,
) -> Option<()> {
    let (origin, targets) = points.split_first()?;
    let dir = line_direction(direction, *targets.last()? - *origin)?;
    let n = dir.perp();
    let side = if offset < 0.0 { -1.0 } else { 1.0 };
    let end = side * points.iter().map(|p| side * p.dot(n)).fold(f64::NEG_INFINITY, f64::max) + offset;
    let numbers = std::iter::once(Some("0")).chain((0..targets.len()).map(|i| resolved.members.get(i).map(|m| m.text.as_str())));
    let mut places: Vec<(Vec2, Text)> = points.iter().copied().zip(numbers.map(set)).collect();
    places.sort_by(|(p, _), (q, _)| p.dot(dir).total_cmp(&q.dot(dir)));
    // Each number's height lies along the set's line: the tallest one's, and a gap, apart.
    let pitch = places.iter().map(|(_, text)| text.half.y * 2.0).fold(0.0, f64::max) + SIZES.gap;
    let ideal: Vec<f64> = places.iter().map(|(p, _)| p.dot(dir)).collect();
    let reading = line_direction(LinearDirection::Aligned, n)?;
    let at = |along: f64, across: f64| dir * along + n * across;
    for ((p, text), (&want, got)) in places.into_iter().zip(ideal.iter().zip(spread(&ideal, pitch))) {
        let start = p + n * (side * SIZES.gap);
        let leader =
            if (got - want).abs() <= SAME_POINT_MM { vec![start, at(want, end)] } else { vec![start, at(want, end - side * JOG_MM), at(got, end)] };
        out.line(leader);
        let centre = at(got, end + side * (SIZES.gap + text.half.x));
        out.text(text, centre, reading);
    }
    Some(())
}

/// Places along one line for texts whose ideal places are `ideal` (least first), at least `pitch`
/// apart and moved as little as can be in all (least squares). Pool-adjacent-violators on the places
/// less their spacing: texts that would overlap pool into blocks, each centred on its members' ideal
/// places, and every other text stays where it is.
fn spread(ideal: &[f64], pitch: f64) -> Vec<f64> {
    // Each block: the sum of its members' places less their spacing, and how many they are.
    let mut blocks: Vec<(f64, usize)> = Vec::with_capacity(ideal.len());
    for (i, x) in ideal.iter().enumerate() {
        blocks.push((x - pitch * i as f64, 1));
        while let [.., (a, m), (b, k)] = blocks[..] {
            if a / m as f64 <= b / k as f64 {
                break;
            }
            blocks.truncate(blocks.len() - 2);
            blocks.push((a + b, m + k));
        }
    }
    let shifted = blocks.into_iter().flat_map(|(sum, k)| std::iter::repeat_n(sum / k as f64, k));
    shifted.enumerate().map(|(i, place)| place + pitch * i as f64).collect()
}

/// A line between two silhouettes or faces, each from one end to the other, at the ends on the side `offset`
/// is on, so a positive one clears what it measures. `Aligned` runs straight across them.
fn across(a: [Vec2; 2], b: [Vec2; 2], direction: LinearDirection, offset: f64, place: &TextPlace, text: Text) -> Option<Vec<Piece>> {
    let [a0, a1] = a;
    let square = (a1 - a0).normalized()?.perp();
    let dir = line_direction(direction, square)?;
    let n = dir.perp();
    let end = |[p, q]: [Vec2; 2]| if (p.dot(n) >= q.dot(n)) == (offset >= 0.0) { p } else { q };
    Some(vec![linear(end(a), end(b), dir, offset, place, text)])
}

/// The angle between two lines (each from one end to the other), at the point they meet on the view,
/// `offset` out from it. `None` when they are parallel there, as they then never meet.
fn angular(a: [Vec2; 2], b: [Vec2; 2], other_side: bool, offset: f64, place: &TextPlace, text: Text) -> Option<Piece> {
    let [a0, a1] = a;
    let [b0, b1] = b;
    let (along_a, along_b) = ((a1 - a0).normalized()?, (b1 - b0).normalized()?);
    let turn = along_a.perp().dot(along_b);
    if turn.abs() < 1e-9 {
        return None;
    }
    let vertex = a0 + along_a * ((b0 - a0).perp().dot(along_b) / turn);
    let ray = |p: Vec2, q: Vec2| (if p.dist(vertex) >= q.dist(vertex) { p } else { q } - vertex).normalized();
    let (from_a, from_b) = (ray(a0, a1)?, ray(b0, b1)?);
    // The smaller angle between the lines, or with `other_side` the larger: the rays' own, or one ray
    // turned round to make it so.
    let from_b = if (from_a.dot(from_b) < 0.0) != other_side { -from_b } else { from_b };
    let cross = from_a.perp().dot(from_b);
    let d0 = if cross >= 0.0 { from_a } else { from_b };
    let sweep = cross.abs().atan2(from_a.dot(from_b));
    let middle = Vec2::from_angle(d0.angle() + sweep * 0.5);
    let towards = at_of(place).and_then(|at| (at - vertex).normalized()).unwrap_or(middle);
    Some(Piece { frame: DimFrame::Angular { vertex, d0, sweep }, at: towards * offset.abs(), text })
}

/// A circle's or an arc's radius or diameter, from its centre and the arc's start, middle and end,
/// with its leader `offset` past the rim, towards its text's place or else the arc's middle.
fn radial(centre: Vec2, arc: [Vec2; 3], diameter: bool, offset: f64, place: &TextPlace, text: Text) -> Option<Piece> {
    let [start, mid, end] = arc;
    let radius = centre.dist(start);
    let whole = start.dist(end) <= SAME_POINT_MM;
    let turned = if whole {
        None
    } else {
        let from = (start - centre).angle();
        let sweep = ((end - centre).angle() - from).rem_euclid(TAU);
        // Counter-clockwise from its start, unless its middle says it runs the other way.
        Some(if ((mid - centre).angle() - from).rem_euclid(TAU) <= sweep { (from, sweep) } else { ((end - centre).angle(), TAU - sweep) })
    };
    let default = if whole { Some(Vec2::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2)) } else { (mid - centre).normalized() };
    let towards = at_of(place).and_then(|at| (at - centre).normalized()).or(default)?;
    Some(Piece { frame: DimFrame::Radial { center: centre, radius, diameter, arc: turned }, at: towards * (radius + offset).max(0.0), text })
}

/// A text's place on paper, when it has its own.
fn at_of(place: &TextPlace) -> Option<Vec2> {
    match place {
        TextPlace::At([x, y]) => Some(Vec2::new(*x, *y)),
        TextPlace::Centred => None,
    }
}

fn same_line(a: &[Vec2], b: &[Vec2]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.dist(*q) <= SAME_POINT_MM)
}

/// Whether `by` already draws `line`: the same line, or for two straight lines, `line` lies along
/// `by` within its ends. Only one dimension's own lines are weighed, so no other's change.
fn covered(line: &[Vec2], by: &[Vec2]) -> bool {
    let (&[p, q], &[a, b]) = (line, by) else { return same_line(line, by) };
    let Some(u) = (b - a).normalized() else { return same_line(line, by) };
    let within = |x: Vec2| {
        let along = (x - a).dot(u);
        (x - a).dot(u.perp()).abs() <= SAME_POINT_MM && (-SAME_POINT_MM..=a.dist(b) + SAME_POINT_MM).contains(&along)
    };
    within(p) && within(q)
}

/// Ends `line` at the base of the arrowhead on its end, if it runs into one there, so no line shows
/// beside the arrowhead's tip. A line running on through a tip, to a tail, stays whole, and so does
/// an extension line that only starts where the arrow is, as its offset is so small.
fn stop_at_base(line: &mut Vec<Vec2>, tip: Vec2, dir: Vec2) {
    let runs_in = |ends: &mut dyn Iterator<Item = &Vec2>| {
        let end = ends.next().is_some_and(|p| p.dist(tip) <= SAME_POINT_MM);
        end && ends.next().and_then(|p| (tip - *p).normalized()).is_some_and(|u| u.dot(dir) > 0.5)
    };
    if runs_in(&mut line.iter().rev()) {
        // An arc's points under the arrowhead go too.
        while line.last().is_some_and(|p| p.dist(tip) < SIZES.arrow) {
            line.pop();
        }
        line.push(tip - dir * SIZES.arrow);
    } else if runs_in(&mut line.iter()) {
        line.reverse();
        stop_at_base(line, tip, dir);
    }
}

/// `line` as a stroke `width_mm` wide, or nothing when it has no length.
fn stroke(line: &[Vec2], width_mm: f64) -> Option<Item> {
    let pieces = line.iter().zip(line.iter().skip(1)).filter(|(a, b)| a != b);
    let path: Vec<Seg2> = pieces.map(|(a, b)| Seg2::Line { a: *a, b: *b }).collect();
    (!path.is_empty()).then_some(Item::Stroke { path, closed: false, width_mm, pattern: None })
}

#[cfg(test)]
#[path = "drawing_dimension_paper_tests.rs"]
mod tests;
