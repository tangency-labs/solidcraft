//! Pure planning for the backdrop blur: which markers share one snapshot of the frame and one
//! blur pyramid, and which pixels each of them needs blurred. No GPU, no egui frame state.
//!
//! A *run* is one snapshot of the composition plus one blur of every member's padded region.
//! Markers are visited in paint order. A marker joins the current run only when nothing painted
//! since the run's snapshot touches its padded region; otherwise it starts a new run and its
//! snapshot includes what was painted in between. That makes a card over a card blur the card
//! below, and cards side by side cost one pyramid.

use egui::Rect;

/// A pixel rectangle, half open: `x0..x1` by `y0..y1`. Always inside the screen when made here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl PxRect {
    /// `None` when the rectangle has no area.
    pub fn new(x0: u32, y0: u32, x1: u32, y1: u32) -> Option<Self> {
        (x1 > x0 && y1 > y0).then_some(Self { x0, y0, x1, y1 })
    }

    pub fn width(&self) -> u32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> u32 {
        self.y1 - self.y0
    }

    pub fn area(&self) -> u64 {
        u64::from(self.width()) * u64::from(self.height())
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }

    /// Grown by `pad` on every side and clamped to a screen of `screen` pixels.
    pub fn padded(&self, pad: u32, screen: [u32; 2]) -> Option<Self> {
        Self::new(
            self.x0.saturating_sub(pad),
            self.y0.saturating_sub(pad),
            self.x1.saturating_add(pad).min(screen[0]),
            self.y1.saturating_add(pad).min(screen[1]),
        )
    }

    /// The same area on a pyramid level `level` halvings down: the outer edge rounds out, so the
    /// level covers everything the full-size rectangle does.
    pub fn at_level(&self, level: u32, dims: [u32; 2]) -> Option<Self> {
        let shift = level.min(31);
        let up = |v: u32| (u64::from(v) + (1_u64 << shift) - 1) >> shift;
        Self::new(self.x0 >> shift, self.y0 >> shift, (up(self.x1) as u32).min(dims[0]), (up(self.y1) as u32).min(dims[1]))
    }
}

/// A marker as the frame emitted it: its paint job and its pixel rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    pub job: usize,
    pub rect: PxRect,
}

/// A marker with the region its blur must cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub job: usize,
    pub region: PxRect,
}

/// Markers that share one snapshot taken just before `start_job`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub start_job: usize,
    pub members: Vec<Member>,
}

/// Split `markers` into runs. `jobs[i]` is the pixel extent of paint job `i` with its clip
/// applied (`None` when it paints nothing). `pad` is the blur's reach in pixels. Markers whose
/// job is outside `jobs`, or that have no area on screen, are dropped.
pub fn plan(jobs: &[Option<PxRect>], markers: &[Marker], pad: u32, screen: [u32; 2]) -> Vec<Run> {
    let mut ordered: Vec<Marker> = markers.iter().copied().filter(|m| m.job < jobs.len()).collect();
    ordered.sort_by_key(|m| m.job);
    let mut runs: Vec<Run> = Vec::new();
    for marker in ordered {
        let Some(region) = marker.rect.padded(pad, screen) else { continue };
        let member = Member { job: marker.job, region };
        if let Some(run) = runs.last_mut() {
            let painted = jobs.get(run.start_job..marker.job).unwrap_or(&[]);
            if !painted.iter().flatten().any(|extent| extent.intersects(&region)) {
                run.members.push(member);
                continue;
            }
        }
        runs.push(Run { start_job: marker.job, members: vec![member] });
    }
    runs
}

/// The pixel extent of something painted in points: `bounds` limited to `clip`, rounded outward.
pub fn px_extent(bounds: Rect, clip: Rect, scale: f32, screen: [u32; 2]) -> Option<PxRect> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let r = bounds.intersect(clip);
    if !r.min.x.is_finite() || !r.min.y.is_finite() || !r.max.x.is_finite() || !r.max.y.is_finite() {
        return None;
    }
    let cut = |v: f32, max: u32, up: bool| {
        let v = if up { (v * scale).ceil() } else { (v * scale).floor() };
        v.clamp(0.0, max as f32) as u32
    };
    PxRect::new(cut(r.min.x, screen[0], false), cut(r.min.y, screen[1], false), cut(r.max.x, screen[0], true), cut(r.max.y, screen[1], true))
}

/// How far, in full-size pixels, a blur of `steps` down and `steps` up passes with `offset`
/// (input texels) reads beyond the pixel it writes. Every tap counts its bilinear footprint, so
/// the padded region of a marker holds everything its blur reads. Slightly generous on purpose.
pub fn reach(steps: usize, offset: f32) -> u32 {
    if !offset.is_finite() || offset < 0.0 {
        return 0;
    }
    let steps = steps.min(8) as i32;
    let mut total = 0.0_f64;
    for level in 0..steps {
        total += (f64::from(offset) + 1.0) * 2.0_f64.powi(level);
    }
    for level in 1..=steps {
        total += (2.0 * f64::from(offset) + 1.0) * 2.0_f64.powi(level);
    }
    (total.ceil() + 2.0_f64.powi(steps)).min(f64::from(u32::MAX)) as u32
}

/// What one frame of one window may spend on blur. Beyond it the later markers keep their plain
/// translucent fill, one marker at a time, and the earlier ones keep blurring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Summed region pixels over all members.
    pub area: u64,
    /// Snapshots, each costing two passes per pyramid step however small its members.
    pub runs: usize,
    /// Markers that get a binding of their own.
    pub members: usize,
}

impl Budget {
    /// The budget for a window of `screen` pixels: eight screens of blurred area, 64 runs, 256 markers.
    pub fn for_screen(screen: [u32; 2]) -> Self {
        Self { area: u64::from(screen[0]).saturating_mul(u64::from(screen[1])).saturating_mul(8), runs: 64, members: 256 }
    }
}

/// Keep, in paint order, the members that fit `budget`. A member that does not fit is dropped and
/// the following ones are still tried; a run left without members is dropped.
pub fn within_budget(runs: Vec<Run>, budget: Budget) -> Vec<Run> {
    let (mut area, mut members) = (0_u64, 0_usize);
    let mut kept: Vec<Run> = Vec::new();
    for run in runs {
        if kept.len() >= budget.runs {
            break;
        }
        let mut taken = Vec::new();
        for member in run.members {
            let next = area.saturating_add(member.region.area());
            if next <= budget.area && members < budget.members {
                area = next;
                members += 1;
                taken.push(member);
            }
        }
        if !taken.is_empty() {
            kept.push(Run { start_job: taken[0].job, members: taken });
        }
    }
    kept
}
