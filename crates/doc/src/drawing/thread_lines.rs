//! A cosmetic thread's lines in a drawing view, as ISO 6410 draws them (#13050): where they lie on
//! the view's plane. Nothing here finds a body's threads or decides how a line is drawn; the view does.
//!
//! - The **crest** is the thread's outside for a screw (the major diameter) and its inside for a
//!   tapped hole (the minor); it is drawn as the part's edges are. The **root** is the other one,
//!   drawn thin: seen from the side, two lines along the thread; seen end on, three quarters of a
//!   circle, its gap in the upper right quadrant. So for an M8 tapped hole the crest is the Ø6.8
//!   drilled hole and the root a thin arc of Ø8; for an M8 screw the crest is Ø8 and the root a thin
//!   arc at the minor diameter.
//! - A **limit line** goes across the thread where its full form ends, as long as the major diameter.
//! - A thread seen at a slant draws no thread lines.
//!
//! - **Each thread line is seen where its crest is seen** ([`on_paper`]): a root line lies inside
//!   its own body's material, so no hidden-line test can see it. It takes, stretch by stretch, the
//!   visibility of the crest line the view drew beside it (end on, the crest circle at that angle);
//!   where the view drew no crest, it draws nothing.
//!
//! Lines are in the view's plane coordinates ([`Plane::to_local`]), as a view's other lines, for
//! [`ViewFrame::seg_to_paper`](solidcraft_geom::drawing_view::ViewFrame::seg_to_paper). A view's
//! paper map turns nothing, so an angle in the plane is the same angle on paper.

use std::f64::consts::{PI, TAU};

use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::UnitVec3;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::prelude::*;
use solidcraft_geom::section::validate_plane;
use solidcraft_kernel::hlr::Visibility;

use crate::ThreadInfo;

/// How near square or parallel to the view's normal a thread's axis must be, by the sine or cosine
/// of their angle, to be seen end on or from the side: the view's own test for a parallel line.
const PARALLEL: f64 = 1e-9;
/// The largest size of any number a thread is drawn from, in millimetres: the section's cap.
const MAX_NUMBER: f64 = 1e12;

/// A thread as a view needs it: its axis, the threaded span along it, and its two radii.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thread {
    pub axis_point: Vec3,
    pub axis: UnitVec3,
    /// The threaded span, along the axis from `axis_point`.
    pub start: f64,
    pub end: f64,
    /// The crest's and the root's radii: an internal thread's root is outside its crest, an
    /// external one's inside.
    pub crest_radius: f64,
    pub root_radius: f64,
    pub internal: bool,
    /// Whether a limit line goes across at `start` and at `end`.
    pub limits: [bool; 2],
}

/// A thread's lines on a view's plane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadLines {
    pub crest: Vec<Seg2>,
    pub root: Vec<Seg2>,
    pub limits: Vec<Seg2>,
}

impl Thread {
    /// A cosmetic thread as a view draws it: its crest is the threaded face's own radius; its root
    /// is the major radius for a tapped hole, and for a screw the minor radius a modelled thread is
    /// cut to ([`crate::thread_forms::ThreadForm::groove_of`]), so the two draw alike. A taper thread
    /// draws none: its lines are not parallel to its axis. No limit lines: [`on_paper`] finds them.
    pub fn of(info: &ThreadInfo) -> Option<Thread> {
        if info.form.is_taper() {
            return None;
        }
        let root_radius = if info.internal {
            info.major_diameter / 2.0
        } else {
            info.form.groove_of(&info.designation, info.major_diameter, info.pitch, false).ok()?.root_r
        };
        Some(Thread {
            axis_point: info.axis_point,
            axis: info.axis,
            start: info.start,
            end: info.end,
            crest_radius: info.radius,
            root_radius,
            internal: info.internal,
            limits: [false, false],
        })
    }

    /// Every number finite and within 1e12 mm, a span that runs forward, radii above zero, and the
    /// root on its kind's side of the crest.
    fn is_drawable(&self) -> bool {
        let within = |v: f64| v.is_finite() && v.abs() <= MAX_NUMBER;
        let p = self.axis_point;
        let numbers = [p.x, p.y, p.z, self.start, self.end, self.crest_radius, self.root_radius].into_iter().all(within);
        let root_side = if self.internal { self.root_radius > self.crest_radius } else { self.root_radius < self.crest_radius };
        numbers && self.start < self.end && self.crest_radius > 0.0 && self.root_radius > 0.0 && root_side
    }
}

/// `thread`'s lines in the view through `basis`: end on (its axis within [`PARALLEL`] of the view's
/// normal, by the sine of their angle), the crest's circle and the root's three-quarter arc; side
/// on (within [`PARALLEL`] of square to it, by the cosine), the crest's and the root's lines along
/// the span and the limit lines asked for. Seen at a slant, a thread that can't be drawn (any number
/// not finite, a span that doesn't run forward, a radius not above zero, the root on the wrong side
/// of the crest) or a basis that isn't one: none.
pub fn thread_lines(basis: &Plane, thread: &Thread) -> Option<ThreadLines> {
    validate_plane(basis).ok()?;
    if !thread.is_drawable() {
        return None;
    }
    let axis = thread.axis.vec().normalized()?;
    let normal = basis.x.cross(basis.y);
    let on_plane = |along: f64| basis.to_local(thread.axis_point + axis * along);
    if axis.cross(normal).len() <= PARALLEL {
        let centre = on_plane(thread.start);
        let circle = |radius: f64, start: f64, sweep: f64| Seg2::Arc { center: centre, radius, start, sweep };
        let quarter = std::f64::consts::FRAC_PI_2;
        return Some(ThreadLines {
            crest: vec![circle(thread.crest_radius, 0.0, 4.0 * quarter)],
            root: vec![circle(thread.root_radius, quarter, 3.0 * quarter)],
            limits: Vec::new(),
        });
    }
    if axis.dot(normal).abs() > PARALLEL {
        return None;
    }
    let (from, to) = (on_plane(thread.start), on_plane(thread.end));
    let across = (to - from).normalized()?.perp();
    let pair = |radius: f64| [-1.0, 1.0].map(|side| Seg2::Line { a: from + across * (side * radius), b: to + across * (side * radius) }).to_vec();
    let major = thread.crest_radius.max(thread.root_radius);
    let limits = [(thread.limits[0], from), (thread.limits[1], to)];
    Some(ThreadLines {
        crest: pair(thread.crest_radius),
        root: pair(thread.root_radius),
        limits: limits.into_iter().filter(|(asked, _)| *asked).map(|(_, at)| Seg2::Line { a: at - across * major, b: at + across * major }).collect(),
    })
}

/// One run of a line the view drew, at one visibility, on paper: where a crest may lie.
#[derive(Clone, Copy, Debug)]
pub struct Drawn<'a> {
    pub visibility: Visibility,
    pub path: &'a [Seg2],
}

/// Which of a thread's lines a piece is: a root line (drawn thin) or a limit line (drawn as the
/// part's edges are).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadStroke {
    Root,
    Limit,
}

/// A run of a thread line at one visibility, on paper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreadPiece {
    pub stroke: ThreadStroke,
    pub visibility: Visibility,
    pub seg: Seg2,
}

/// `thread`'s root and limit lines in the view `frame` places on paper, each seen where the crest
/// beside it is among `drawn` (the lines the view drew of the thread's body), to within `near_mm` on
/// paper. Side on, a root line takes the visibility of the crest line on its own side; a limit line
/// goes where the thread ends while a crest runs on (the thread stops inside its face) and takes the
/// crests' visibility there. End on, the root arc takes the crest circle's by angle. Where crests of
/// several visibilities overlap, the one most seen wins ([`rank`]). Nothing for a thread
/// [`thread_lines`] draws nothing for, or a `near_mm` that isn't a finite size.
pub fn on_paper(frame: &ViewFrame, thread: &Thread, drawn: &[Drawn<'_>], near_mm: f64) -> Vec<ThreadPiece> {
    let basis = frame.basis();
    let Some(lines) = thread_lines(&basis, thread) else { return Vec::new() };
    if !(near_mm.is_finite() && near_mm >= 0.0) {
        return Vec::new();
    }
    let segs = || drawn.iter().flat_map(|d| d.path.iter().map(move |seg| (d.visibility, seg)));
    let paper = |segs: &[Seg2]| segs.iter().map(|s| frame.seg_to_paper(s)).collect::<Vec<_>>();
    match (paper(&lines.crest).as_slice(), paper(&lines.root).as_slice()) {
        ([crest], [root]) => end_on(crest, root, segs(), near_mm),
        ([crest_a, crest_b], [root_a, root_b]) => {
            // Along each side's crest, from the thread's start: the stretches the view drew of it.
            let mut sides = Vec::with_capacity(2);
            for (crest, root) in [(crest_a, root_a), (crest_b, root_b)] {
                let Some(dir) = (crest.end() - crest.start()).normalized() else { return Vec::new() };
                let spans: Vec<Span> = segs().filter_map(|(visibility, seg)| along_line(seg, crest.start(), dir, near_mm, visibility)).collect();
                sides.push((*root, dir, spans));
            }
            let length = crest_a.length();
            let mut out = Vec::new();
            for (root, dir, spans) in &sides {
                let stretch = |(lo, hi, visibility)| ThreadPiece {
                    stroke: ThreadStroke::Root,
                    visibility,
                    seg: Seg2::Line { a: root.start() + *dir * lo, b: root.start() + *dir * hi },
                };
                out.extend(runs(0.0, length, spans, near_mm).into_iter().map(stretch));
            }
            let all: Vec<Span> = sides.into_iter().flat_map(|(.., spans)| spans).collect();
            let first = all.iter().map(|s| s.lo).fold(f64::INFINITY, f64::min);
            let last = all.iter().map(|s| s.hi).fold(f64::NEG_INFINITY, f64::max);
            let ends = [(first < -near_mm, 0.0), (last > length + near_mm, length)];
            // thread_lines gives the limit lines asked for, in the order of the ends.
            let Some(limits) = thread_lines(&basis, &Thread { limits: ends.map(|(inside, _)| inside), ..*thread }) else { return out };
            let at = ends.into_iter().filter(|(inside, _)| *inside).map(|(_, t)| t);
            for (t, limit) in at.zip(&limits.limits) {
                if let Some(visibility) = seen_at(&all, t, near_mm) {
                    out.push(ThreadPiece { stroke: ThreadStroke::Limit, visibility, seg: frame.seg_to_paper(limit) });
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// A stretch of a crest the view drew: from `lo` to `hi` along it (millimetres on paper, or radians
/// round a circle), at one visibility.
#[derive(Clone, Copy, Debug)]
struct Span {
    lo: f64,
    hi: f64,
    visibility: Visibility,
}

/// Which visibility wins where crests overlap: the most seen.
fn rank(visibility: Visibility) -> u8 {
    match visibility {
        Visibility::Visible => 2,
        Visibility::Uncertain => 1,
        Visibility::Hidden => 0,
    }
}

/// End on: the root arc, cut where the crest circle's visibility changes.
fn end_on<'a>(crest: &Seg2, root: &Seg2, segs: impl Iterator<Item = (Visibility, &'a Seg2)>, near_mm: f64) -> Vec<ThreadPiece> {
    let (&Seg2::Arc { center, radius, .. }, &Seg2::Arc { radius: root_radius, start, sweep, .. }) = (crest, root) else { return Vec::new() };
    let mut spans = Vec::new();
    for (visibility, seg) in segs {
        if let Some((lo, turn)) = round_circle(seg, center, radius, near_mm) {
            // A full turn either way too, so the stretch meets the root arc however it is numbered.
            spans.extend([-TAU, 0.0, TAU].map(|shift| Span { lo: lo + shift, hi: lo + turn + shift, visibility }));
        }
    }
    let arc = |(lo, hi, visibility)| ThreadPiece {
        stroke: ThreadStroke::Root,
        visibility,
        seg: Seg2::Arc { center, radius: root_radius, start: lo, sweep: hi - lo },
    };
    runs(start, start + sweep, &spans, near_mm / radius).into_iter().map(arc).collect()
}

/// Where `seg` runs round the circle about `center` of `radius`, both its ends within `near_mm` of
/// it: its start angle (from 0 to a full turn) and how far it turns (above 0, at most a full turn).
/// An arc must be of that circle; a line goes the short way round between its ends.
fn round_circle(seg: &Seg2, center: Vec2, radius: f64, near_mm: f64) -> Option<(f64, f64)> {
    let on = |p: Vec2| (p.dist(center) - radius).abs() <= near_mm;
    let (lo, turn) = match *seg {
        Seg2::Arc { center: c, radius: r, start, sweep } => {
            if !(c.dist(center) <= near_mm && (r - radius).abs() <= near_mm) {
                return None;
            }
            if sweep < 0.0 { (start + sweep, -sweep) } else { (start, sweep) }
        }
        Seg2::Line { a, b } => {
            if !(on(a) && on(b)) {
                return None;
            }
            let (from, to) = ((a - center).angle(), (b - center).angle());
            let turn = (to - from + PI).rem_euclid(TAU) - PI;
            if turn < 0.0 { (to, -turn) } else { (from, turn) }
        }
        // A view draws no free-form curve where a crest is a circle.
        _ => return None,
    };
    (lo.is_finite() && turn.is_finite() && turn > 0.0).then(|| (lo.rem_euclid(TAU), turn.min(TAU)))
}

/// Side on: where `seg`, a line with both ends within `near_mm` of the line through `from` along
/// `dir`, runs along it.
fn along_line(seg: &Seg2, from: Vec2, dir: Vec2, near_mm: f64, visibility: Visibility) -> Option<Span> {
    let Seg2::Line { a, b } = *seg else { return None };
    let off = |p: Vec2| (p - from).cross(dir).abs();
    if !(off(a) <= near_mm && off(b) <= near_mm) {
        return None;
    }
    let (ta, tb) = ((a - from).dot(dir), (b - from).dot(dir));
    (ta.is_finite() && tb.is_finite()).then(|| Span { lo: ta.min(tb), hi: ta.max(tb), visibility })
}

/// The stretches from `lo` to `hi` that `spans` cover, each at the winning visibility of the spans
/// over it ([`rank`]). Runs no longer than `join` are left out, and runs of one visibility that close
/// to each other are one: a crest drawn in chords meets itself only to within that.
fn runs(lo: f64, hi: f64, spans: &[Span], join: f64) -> Vec<(f64, f64, Visibility)> {
    // Each visibility's spans merged first, so the walk below meets a handful of stretches however
    // finely the crest was drawn: O(n log n) in the drawn chords.
    let mut merged: Vec<Span> = Vec::new();
    for visibility in [Visibility::Visible, Visibility::Uncertain, Visibility::Hidden] {
        let mut own: Vec<Span> = spans.iter().filter(|s| s.visibility == visibility && s.hi > lo && s.lo < hi).copied().collect();
        own.sort_by(|a, b| a.lo.total_cmp(&b.lo));
        let first = merged.len();
        for span in own {
            let joins = merged.len() > first && merged.last().is_some_and(|last| span.lo <= last.hi + join);
            match merged.last_mut() {
                Some(last) if joins => last.hi = last.hi.max(span.hi),
                _ => merged.push(span),
            }
        }
    }
    let mut cuts: Vec<f64> = merged.iter().flat_map(|s| [s.lo, s.hi]).filter(|t| *t > lo && *t < hi).chain([lo, hi]).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut out: Vec<(f64, f64, Visibility)> = Vec::new();
    for (&a, &b) in cuts.iter().zip(cuts.iter().skip(1)) {
        let mid = 0.5 * (a + b);
        let over = merged.iter().filter(|s| s.lo <= mid && mid <= s.hi).map(|s| s.visibility).max_by_key(|v| rank(*v));
        let Some(visibility) = over else { continue };
        if b - a <= join {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.2 == visibility && a - last.1 <= join => last.1 = b,
            _ => out.push((a, b, visibility)),
        }
    }
    out
}

/// The winning visibility of the crest stretches within `near_mm` of `t`, if any.
fn seen_at(spans: &[Span], t: f64, near_mm: f64) -> Option<Visibility> {
    spans.iter().filter(|s| s.lo - near_mm <= t && t <= s.hi + near_mm).map(|s| s.visibility).max_by_key(|v| rank(*v))
}

#[cfg(test)]
#[path = "thread_lines_tests.rs"]
mod tests;
