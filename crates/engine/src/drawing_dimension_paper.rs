//! A drawing's dimensions on paper (#4896 W19g, #4894 WE6): one group for each dimension, its
//! extension and dimension lines and its arrowheads, laid out by [`dim_layout_sized`] in paper
//! millimetres, so a dimension is the same size at any view scale. [`paper_group`] lays one out
//! centred on its view, as a view's lines are, so moving the view moves it unchanged.
//!
//! Its text is set in the drawing font ([`paper_text`]) and measured there, so the arrows go inside
//! or outside by its real length. A text the font can't set leaves the lines and says why.

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
use solidcraft_sketch::{DimFrame, DimSizes, DimText, Outcome, PaperFonts, TextStyle, dim_layout_sized, encode_text};

/// A dimension's sizes on paper, with its text above an unbroken line (ISO 129-1).
const SIZES: DimSizes = DimSizes { arrow: 3.0, gap: 1.0, overshoot: 2.0, offset: 10.0, above: true };
/// A filled arrowhead, its line stopped at its base by [`stop_at_base`]: a note's leader ends in it too.
pub(crate) const ARROW: Terminator = Terminator::Arrow { shape: ArrowShape::Filled, length_mm: SIZES.arrow, angle: PI / 9.0 };
/// The height of a dimension's capitals, and a note's unless it sets its own.
pub(crate) const TEXT_MM: f64 = 3.5;
/// How close two points of a layout are when they are one point.
const SAME_POINT_MM: f64 = 1e-6;

/// One laid-out piece of a dimension: a chain has one for each of its members.
struct Piece {
    frame: DimFrame,
    /// Where the layout puts its text, from the frame's own origin.
    at: DimText,
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

/// What a line of a dimension's layout is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineRole {
    /// From near what it measures to past the dimension line.
    Extension,
    /// A dimension line or arc, from arrow tip to arrow tip, with any run-on to a text outside.
    Dimension,
}

/// Where a dimension's parts sit, centred on its view as its group is: its lines as laid out, before
/// an arrowhead stops one at its base, a chain's shared extension line once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub lines: Vec<(LineRole, Vec<Vec2>)>,
    /// Each arrowhead's tip and the unit direction it points in.
    pub arrows: Vec<(Vec2, Vec2)>,
    /// Each set text's box: its cap height and measured width, as the arrows fit by, its corners
    /// in reading order: start and end on the line's side, then end and start on the far side. A
    /// text that isn't set has none.
    pub text_boxes: Vec<[Vec2; 4]>,
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
    /// Where its parts sit ([`Layout`]): a note's is empty.
    pub layout: Layout,
}

/// `dimension`, read as `resolved`, on the view through `frame` (centred on the view), its lines in
/// `group` and its text in the drawing's `lettering`: its group, or `None` when what it measures has no place to draw on the
/// view: it wasn't found again, or it is a single point there.
pub fn paper_group(dimension: &Dimension, resolved: &Resolved, frame: &ViewFrame, group: LineGroup, lettering: &PaperFonts) -> Option<OnPaper> {
    let line_mm = LineKind::Thin.width_mm(group);
    let on: Vec<Vec2> = resolved.points.iter().map(|p| frame.to_paper(*p)).collect();
    let mut lines: Vec<Vec<Vec2>> = Vec::new();
    let mut arrows = Vec::new();
    let (mut texts, mut fonts, mut why, mut pending) = (Vec::new(), Vec::new(), None, false);
    let mut laid = Layout::default();
    let set = |text: Option<&str>| Text::set(text, TEXT_MM, lettering);
    for piece in pieces(dimension, resolved, &on, &set)? {
        let layout = dim_layout_sized(&piece.frame, piece.at, &SIZES, piece.text.half, [None, None]);
        if !piece.text.runs.is_empty() {
            laid.text_boxes.push(text_box(layout.text, layout.text_dir, piece.text.half));
        }
        texts.extend(placed(piece.text.runs, piece.text.half, TEXT_MM, layout.text, layout.text_dir));
        fonts.extend(piece.text.fonts);
        why = why.or(piece.text.why);
        pending |= piece.text.pending;
        // A chain's neighbours share an extension line.
        for (i, line) in layout.lines.into_iter().enumerate() {
            if !lines.iter().any(|other| same_line(other, &line)) {
                let role = if i < layout.extensions { LineRole::Extension } else { LineRole::Dimension };
                laid.lines.push((role, line.clone()));
                lines.push(line);
            }
        }
        laid.arrows.extend(&layout.arrows);
        arrows.extend(layout.arrows);
    }
    for (tip, dir) in &arrows {
        lines.iter_mut().for_each(|line| stop_at_base(line, *tip, *dir));
    }
    let mut items: Vec<Item> = lines.iter().filter_map(|line| stroke(line, line_mm)).collect();
    for (tip, dir) in &arrows {
        items.extend(ARROW.place(*tip, *tip - *dir, line_mm).ok()?.0);
    }
    items.extend(texts);
    Some(OnPaper { group: Group { object: dimension.id, kind: ObjectKind::Dimension, items }, fonts, why, pending, layout: laid })
}

/// The box of a text `half` in size centred on `centre` and running along `dir`, its corners as
/// [`Layout::text_boxes`] has them. For one line, its first corner is where [`placed`] starts the run.
fn text_box(centre: Vec2, dir: Vec2, half: Vec2) -> [Vec2; 4] {
    let (along, up) = (dir * half.x, dir.perp() * half.y);
    [centre - along - up, centre + along - up, centre + along + up, centre - along + up]
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

/// A point a dimension is dragged to on paper, from its view's centre as its group is laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Drag {
    /// Where its text's centre goes.
    Text(Vec2),
    /// A point its dimension line passes through: an angle's arc, or the leader a radial one's text
    /// turns on.
    Line(Vec2),
}

impl Drag {
    /// The same point seen from `origin`: a point of the sheet, from its view's centre there.
    pub(crate) fn seen_from(self, origin: Vec2) -> Drag {
        match self {
            Drag::Text(p) => Drag::Text(p - origin),
            Drag::Line(p) => Drag::Line(p - origin),
        }
    }
}

/// Why a dimension can't be laid out where it is dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NotDragged {
    /// What it measures has no place on its view.
    NoPlace,
    /// A chain's texts each sit over their own member.
    ChainText,
    /// Between the ends of two silhouettes or faces: a line across them runs beyond one of them.
    BetweenEnds,
    /// Its text's size waits for a font to fall back to, and the drag needs it.
    TextWaits,
}

/// The offset and text place that lay `dimension` out where `drag` puts it ([`pieces`] turned round),
/// read as `resolved` on the view through `frame`, its text in the drawing's `lettering`: its text's
/// centre at a [`Drag::Text`] point, its line through a [`Drag::Line`] one. A line drag keeps the
/// text's place, but a radial one's text turns with its leader; an angle's offset keeps its sign.
pub(crate) fn dragged(
    dimension: &Dimension,
    resolved: &Resolved,
    frame: &ViewFrame,
    lettering: &PaperFonts,
    drag: Drag,
) -> Result<(f64, TextPlace), NotDragged> {
    let on: Vec<Vec2> = resolved.points.iter().map(|p| frame.to_paper(*p)).collect();
    let (offset, place) = (dimension.offset_mm, dimension.text);
    // The text's half size, which a linear one's lift above its line depends on.
    let half = || {
        let text = Text::set(resolved.text.as_deref(), TEXT_MM, lettering);
        if text.pending { Err(NotDragged::TextWaits) } else { Ok(text.half) }
    };
    match (&dimension.kind, on.as_slice(), drag) {
        (DimensionKind::Linear { direction, .. } | DimensionKind::Length { direction, .. }, &[a, b], _) => {
            let dir = line_direction(*direction, b - a).ok_or(NotDragged::NoPlace)?;
            linear_dragged(a, b, dir, place, drag, &half)
        }
        (DimensionKind::Cylinder { .. }, &[a0, a1, b0, b1], _) => across_dragged([a0, a1], [b0, b1], LinearDirection::Aligned, place, drag, &half),
        (DimensionKind::Silhouettes { direction, .. } | DimensionKind::Faces { direction, .. }, &[a0, a1, b0, b1], _) => {
            across_dragged([a0, a1], [b0, b1], *direction, place, drag, &half)
        }
        (DimensionKind::Chain { .. }, _, Drag::Text(_)) => Err(NotDragged::ChainText),
        // Its members share the line `offset` from the middle of its two ends.
        (DimensionKind::Chain { direction, .. }, points, Drag::Line(p)) => {
            let (Some(first), Some(last)) = (points.first(), points.last()) else { return Err(NotDragged::NoPlace) };
            let dir = line_direction(*direction, *last - *first).ok_or(NotDragged::NoPlace)?;
            Ok((encode_text(&DimFrame::Linear { p0: *first, p1: *last, dir }, p).y, place))
        }
        // Its arc is the offset's size round the vertex, its text towards its place.
        (DimensionKind::Angle { .. } | DimensionKind::Cone { .. }, &[a0, a1, b0, b1], _) => {
            let vertex = meet([a0, a1], [b0, b1]).ok_or(NotDragged::NoPlace)?;
            Ok(match drag {
                Drag::Text(p) => (p.dist(vertex).copysign(offset), TextPlace::At([p.x, p.y])),
                Drag::Line(p) => (p.dist(vertex).copysign(offset), place),
            })
        }
        // Its text is the offset past the rim, towards its place.
        (DimensionKind::Radial { .. } | DimensionKind::Sphere { .. }, &[centre, start, _, _], _) => Ok(match drag {
            Drag::Text(p) => (p.dist(centre) - centre.dist(start), TextPlace::At([p.x, p.y])),
            Drag::Line(p) => (offset, TextPlace::At([p.x, p.y])),
        }),
        _ => Err(NotDragged::NoPlace),
    }
}

/// A line from `a` to `b` along `dir` ([`linear`]) dragged: through a line point, or with its text's
/// centre on a text point, as far across as it is less the lift above the line the layout gives a text.
fn linear_dragged(
    a: Vec2,
    b: Vec2,
    dir: Vec2,
    place: TextPlace,
    drag: Drag,
    half: &dyn Fn() -> Result<Vec2, NotDragged>,
) -> Result<(f64, TextPlace), NotDragged> {
    let frame = DimFrame::Linear { p0: a, p1: b, dir };
    Ok(match drag {
        Drag::Line(p) => (encode_text(&frame, p).y, place),
        Drag::Text(p) => {
            // The layout's own lift ([`dim_layout_sized`]), read from a text placed on the line's middle.
            let lift = dim_layout_sized(&frame, DimText::At(Vec2::ZERO), &SIZES, half()?, [None, None]).text - (a + b) * 0.5;
            (encode_text(&frame, p - lift).y, TextPlace::At([p.x, p.y]))
        }
    })
}

/// A line across two silhouettes or faces ([`across`]) dragged: on the side whose ends give an offset of
/// that side's sign, as the layout picks the ends by the sign. Between them no offset draws it.
fn across_dragged(
    a: [Vec2; 2],
    b: [Vec2; 2],
    direction: LinearDirection,
    place: TextPlace,
    drag: Drag,
    half: &dyn Fn() -> Result<Vec2, NotDragged>,
) -> Result<(f64, TextPlace), NotDragged> {
    for positive in [true, false] {
        let (p, q, dir) = across_ends(a, b, direction, positive).ok_or(NotDragged::NoPlace)?;
        let (offset, place) = linear_dragged(p, q, dir, place, drag, half)?;
        if (offset >= 0.0) == positive {
            return Ok((offset, place));
        }
    }
    Err(NotDragged::BetweenEnds)
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

/// A line from `a` to `b` along `dir`, `across` from their middle. Its text is at its `place`, or
/// in the middle when it fits there, or else beyond an arrow ([`DimText::Across`]).
fn linear(a: Vec2, b: Vec2, dir: Vec2, across: f64, place: &TextPlace, text: Text) -> Piece {
    let at = at_of(place).map_or(DimText::Across(across), |at| DimText::At(Vec2::new((at - (a + b) * 0.5).dot(dir), across)));
    Piece { frame: DimFrame::Linear { p0: a, p1: b, dir }, at, text }
}

/// A line from `a` to `b` along `dir`, `across` from their middle, with its text in the middle
/// whether it fits there or not: a set's member, whose texts beside each other need a rule of their own.
fn centred(a: Vec2, b: Vec2, dir: Vec2, across: f64, text: Text) -> Piece {
    Piece { frame: DimFrame::Linear { p0: a, p1: b, dir }, at: DimText::At(Vec2::new(0.0, across)), text }
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
        centred(*a, *b, dir, line - ((*a + *b) * 0.5).dot(n), set(text))
    };
    Some(points.iter().zip(points.iter().skip(1)).enumerate().map(member).collect())
}

/// A line between two silhouettes or faces, each from one end to the other, at the ends on the side `offset`
/// is on, so a positive one clears what it measures. `Aligned` runs straight across them.
fn across(a: [Vec2; 2], b: [Vec2; 2], direction: LinearDirection, offset: f64, place: &TextPlace, text: Text) -> Option<Vec<Piece>> {
    let (p, q, dir) = across_ends(a, b, direction, offset >= 0.0)?;
    Some(vec![linear(p, q, dir, offset, place, text)])
}

/// The ends a line across two silhouettes or faces (each from one end to the other) runs between: on
/// the side a `positive` offset is on, or a negative one. With the direction the line runs in.
fn across_ends(a: [Vec2; 2], b: [Vec2; 2], direction: LinearDirection, positive: bool) -> Option<(Vec2, Vec2, Vec2)> {
    let [a0, a1] = a;
    let square = (a1 - a0).normalized()?.perp();
    let dir = line_direction(direction, square)?;
    let n = dir.perp();
    let end = |[p, q]: [Vec2; 2]| if (p.dot(n) >= q.dot(n)) == positive { p } else { q };
    Some((end(a), end(b), dir))
}

/// Where two lines (each from one end to the other) meet on the view. `None` when they are parallel
/// there, as they then never meet.
fn meet(a: [Vec2; 2], b: [Vec2; 2]) -> Option<Vec2> {
    let [a0, a1] = a;
    let [b0, b1] = b;
    let (along_a, along_b) = ((a1 - a0).normalized()?, (b1 - b0).normalized()?);
    let turn = along_a.perp().dot(along_b);
    if turn.abs() < 1e-9 {
        return None;
    }
    Some(a0 + along_a * ((b0 - a0).perp().dot(along_b) / turn))
}

/// The angle between two lines (each from one end to the other), at the point they meet on the view,
/// `offset` out from it. `None` when they are parallel there, as they then never meet.
fn angular(a: [Vec2; 2], b: [Vec2; 2], other_side: bool, offset: f64, place: &TextPlace, text: Text) -> Option<Piece> {
    let [a0, a1] = a;
    let [b0, b1] = b;
    let vertex = meet(a, b)?;
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
    Some(Piece { frame: DimFrame::Angular { vertex, d0, sweep }, at: DimText::At(towards * offset.abs()), text })
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
    Some(Piece {
        frame: DimFrame::Radial { center: centre, radius, diameter, arc: turned },
        at: DimText::At(towards * (radius + offset).max(0.0)),
        text,
    })
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
pub(crate) mod tests;
