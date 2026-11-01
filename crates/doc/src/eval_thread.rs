//! Threads: the cosmetic thread on a cylindrical face, and the modelled
//! thread's groove cut along a helix.

use super::*;
use crate::thread_forms::{SteepFlank, ThreadForm, ThreadHand, ThreadTip};

/// A cosmetic thread: on which cylinder, its size, and the threaded part of the axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub feature: u64,
    pub body: String,
    pub designation: String,
    pub major_diameter: f64,
    /// Radius of the actual cylindrical face when the cosmetic thread is evaluated (0 in a
    /// state saved without it: the cosmetic display then leaves the thread out).
    #[serde(default)]
    pub radius: f64,
    pub pitch: f64,
    pub internal: bool,
    pub axis_point: Vec3,
    pub axis: UnitVec3,
    /// Threaded part along the axis from `axis_point`.
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub form: ThreadForm,
    #[serde(default)]
    pub hand: ThreadHand,
    #[serde(default = "crate::thread_forms::one_start")]
    pub starts: u32,
}

/// Nominal diameters and coarse pitches (mm) used for automatic ISO metric thread sizing.
/// Other diameter/pitch pairs can be supplied as explicit designations.
pub fn metric_thread_sizes() -> &'static [(f64, f64)] {
    crate::thread_forms::ISO_COARSE
}

/// "M8" or "M8x1" → (nominal diameter, pitch).
pub fn parse_metric_thread(s: &str) -> Option<(f64, f64)> {
    ThreadForm::Metric.parse(s, 1).ok().filter(|z| (z.lead - z.pitch).abs() < 1e-12).map(|z| (z.major, z.pitch))
}

/// The automatic ISO metric designation for a cylindrical face, using the same fit check as
/// a thread feature without an explicit size. This query does not change the model.
pub fn suggested_thread(st: &ModelState, p: Vec3) -> Result<String> {
    suggested_thread_in(st, p, ThreadForm::Metric)
}

/// The standard designation of `form` that fits the cylindrical face at `p` (a shaft its major
/// diameter, a hole its drill), with the same fit check as a thread feature without an explicit
/// size. This query does not change the model.
pub fn suggested_thread_in(st: &ModelState, p: Vec3, form: ThreadForm) -> Result<String> {
    thread_on(st, 0, p, &ThreadAsk { form: Some(form), ..Default::default() }).map(|t| t.info.designation)
}

/// How far a thread runs along its face.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Extent {
    /// The whole face.
    #[default]
    Full,
    /// This long, from the end it starts at.
    Length(f64),
    /// From the end it starts at to where this plane crosses the axis.
    UpTo(Plane),
    /// From the end it starts at to this point's place along the axis.
    UpToPoint(Vec3),
}

/// What a thread feature asks for: its size, how far it runs and from which end.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ThreadAsk<'a> {
    /// The form; `None`: the one `designation`'s prefix names, else metric.
    pub form: Option<ThreadForm>,
    pub designation: Option<&'a str>,
    pub hand: ThreadHand,
    /// Starts; 0 is taken as 1.
    pub starts: u32,
    pub steep: SteepFlank,
    pub extent: Extent,
    /// A point near the end of the face the thread starts at.
    pub from: Option<Vec3>,
    /// A modelled thread needs to know how each of its ends meets the face's ends.
    pub modeled: bool,
    /// The runout at the end away from where the thread starts, as (mm, pitches): its length
    /// is mm + pitches x the pitch. The start never fades: it runs off its free end.
    pub ease_out: (f64, f64),
    /// A tapped hole's thread: `designation` is a catalogue size (ISO metric, or a catalogued
    /// UNC/UNF, G, Rp, Rc or NPT size), and a size that is not metric fits the hole drilled for it.
    pub tapped: bool,
}

/// How one end of a thread's length meets its face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    /// At an end of the face past which there is no material in the groove's way: the groove
    /// runs out into the air (or through a chamfer).
    Free,
    /// At (or within half a pitch of) an end of the face where material goes on past it, a
    /// shoulder, a head, a blind hole's bottom: the groove stops short, clear of it.
    Closed,
    /// Inside the face: the groove stops square there.
    Inside,
}

/// A thread resolved on its face.
pub(super) struct ThreadOn {
    pub info: ThreadInfo,
    cyl: kernel::CylinderFace,
    /// How the thread's start and end meet the face (modelled threads only).
    ends: [End; 2],
    steep: SteepFlank,
    /// Runout lengths (mm) at the thread's start and end along the axis.
    ease: [f64; 2],
    /// The end of the axis (0: its start, 1: its end) the thread starts from.
    from: usize,
}

/// The free-end test of a face's two ends (`side` 0 at its start, 1 at its end), each made
/// once. The groove's one-pitch overrun past a free end reaches 1.5 pitches past it. An end is
/// closed when, within that reach, material lies outside the face's cylinder (inside it, for a
/// hole) at the radii the groove cuts beyond the face: a shoulder, a plate, a head, a blind
/// hole's bottom, however thin, the material stepping out past the face. It is closed too when
/// material lies in the groove's band of radii beyond its first pitch: the face's cylinder going
/// on past a split. A chamfer, a neck, a countersink or a counterbore steps the other way and
/// leaves the end free: the groove runs out through it.
struct FaceEnds<'a> {
    body: &'a Body,
    cyl: &'a kernel::CylinderFace,
    band: (f64, f64),
    /// The radii the groove cuts beyond the face's own: from it out to the groove's outer edge
    /// (external), or in to its inner edge (internal).
    beyond: (f64, f64),
    pitch: f64,
    free: [Option<bool>; 2],
}

impl FaceEnds<'_> {
    fn free(&mut self, side: usize) -> Result<bool> {
        if let Some(Some(f)) = self.free.get(side) {
            return Ok(*f);
        }
        let p = self.pitch;
        let past =
            |from: f64, to: f64| if side == 0 { (self.cyl.start - to, self.cyl.start - from) } else { (self.cyl.end + from, self.cyl.end + to) };
        // Material that only touches the band's rims (a counterbore as wide as the thread, the
        // face's own end) is not in the groove's way.
        let e = p * 1e-3;
        let stepped_out = kernel::material_in_ring(self.body, self.cyl, self.beyond.0 + e, self.beyond.1 - e, past(e, 1.5 * p))?;
        let free = !stepped_out && !kernel::material_in_ring(self.body, self.cyl, self.band.0 + e, self.band.1 - e, past(p, 1.5 * p))?;
        if let Some(slot) = self.free.get_mut(side) {
            *slot = Some(free);
        }
        Ok(free)
    }
}

/// A thread on the cylindrical face at `p`: its size, and the part of the face it runs over.
pub(super) fn thread_on(st: &ModelState, feature: u64, p: Vec3, ask: &ThreadAsk) -> Result<ThreadOn> {
    let (bi, cyl) = st
        .bodies
        .iter()
        .enumerate()
        .filter_map(|(i, b)| kernel::cylinder_face_at(&b.body, p).map(|c| (i, c)))
        .min_by(|a, b| {
            let dist = |c: &kernel::CylinderFace| ((p - c.axis_point - c.axis * (p - c.axis_point).dot(c.axis)).len() - c.radius).abs();
            dist(&a.1).total_cmp(&dist(&b.1))
        })
        .ok_or_else(|| DocError::Invalid("no cylindrical face there to thread".into()))?;
    let mb = st.bodies.get(bi).ok_or_else(|| DocError::Invalid("threaded body".into()))?;
    let dia = cyl.radius * 2.0;
    let form = match (ask.form, ask.designation.and_then(ThreadForm::of_designation)) {
        (Some(f), _) | (None, Some(f)) => f,
        (None, None) => ThreadForm::Metric,
    };
    if let Some(why) = form.refused() {
        return Err(DocError::Invalid(why.into()));
    }
    let starts = ask.starts.max(1);
    if starts > 8 {
        return Err(DocError::Invalid("a thread has 1 to 8 starts".into()));
    }
    // A tapped hole's size is the catalogue's.
    let tapped = match (ask.tapped, ask.designation) {
        (false, _) => None,
        (true, None) => return Err(DocError::Invalid("a tapped hole needs a designation".into())),
        (true, Some(name)) => Some(crate::parse_tapped_thread(name).ok_or_else(|| {
            DocError::Invalid(format!("unknown tapped thread `{name}` (ISO metric or a catalogued UNC/UNF, G, Rp, Rc or NPT size)"))
        })?),
    };
    let size = match (&tapped, ask.designation) {
        (Some(t), _) => crate::thread_forms::ThreadSize { form, major: t.major_diameter, pitch: t.pitch, lead: t.pitch },
        (None, Some(s)) => form.parse(s, starts).map_err(DocError::Invalid)?,
        (None, None) => {
            let (d, p) = form.fit(dia, cyl.internal).ok_or_else(|| {
                DocError::Invalid(if form.sizes().is_empty() {
                    format!("{} threads have no size table: give the designation ({})", form.name(), form.format(50.8, 25.4 / 8.0, 25.4 / 8.0))
                } else {
                    format!("no {} thread fits a {dia:.2} mm cylinder", form.name())
                })
            })?;
            crate::thread_forms::ThreadSize { form, major: d, pitch: p, lead: p * f64::from(starts) }
        }
    };
    let (major, pitch) = (size.major, size.pitch);
    // A designation that gives the lead (M16xPh3P1.5, Tr40x14(P7)) gives the starts.
    let named = (size.lead / pitch).round() as u32;
    if ask.starts > 1 && named != starts {
        return Err(DocError::Invalid(format!("`{}` has {named} start(s), not {starts}", ask.designation.unwrap_or_default())));
    }
    let starts = named.max(1);
    let mut designation = tapped.as_ref().map(|t| t.designation.clone()).unwrap_or_else(|| form.format(major, pitch, size.lead));
    if !form.pipe_sizes().is_empty() && tapped.is_none() {
        if ask.hand == ThreadHand::Both {
            return Err(DocError::Invalid("a pipe thread turns one way: right or left".into()));
        }
        // ISO 7-1's Rp is the parallel internal thread an R screws into; there is no external Rp.
        if form == ThreadForm::Rp && !cyl.internal {
            return Err(DocError::Invalid(format!(
                "`{designation}` on this shaft: Rp is an internal thread (ISO 7-1); an external pipe thread is R (taper, ISO 7-1) or G (ISO 228-1)"
            )));
        }
        // ISO 7-1 names the internal taper thread Rc and the external one R.
        if form == ThreadForm::Bspt {
            if let Some(given) = ask.designation
                && ThreadForm::pipe_internal(given) != cyl.internal
            {
                let want = if cyl.internal { "an internal taper thread is Rc" } else { "an external taper thread is R" };
                return Err(DocError::Invalid(format!("`{given}` on this {}: {want} (ISO 7-1)", if cyl.internal { "hole" } else { "shaft" })));
            }
            if cyl.internal {
                designation = designation.replacen("R ", "Rc ", 1);
            }
        }
    }
    let minor = 2.0 * form.groove(major, pitch, true).crest_r;
    let fits = if let Some(t) = tapped.as_ref().filter(|t| t.family != crate::TappedThreadFamily::Metric) {
        // The hole drilled for it, without borrowing an ISO minor-diameter rule.
        cyl.internal && (t.tap_drill - dia).abs() < 0.15 * major
    } else if cyl.internal {
        (minor - dia).abs() < 0.2 * major || (form.drill(major, pitch) - dia).abs() < 0.15 * major
    } else {
        (major - dia).abs() < 0.15 * major
    };
    if !fits {
        return Err(DocError::Invalid(format!("an {designation} thread does not fit a {dia:.2} mm {}", if cyl.internal { "hole" } else { "shaft" })));
    }
    // The radii the groove cuts: from the external thread's minor diameter (a nut's: its minor)
    // to the major (a nut's root), and to the face itself where it lies outside that.
    let (r_lo, r_hi) = if cyl.internal {
        let g = form.groove(major, pitch, true);
        (g.crest_r, g.root_r)
    } else {
        (form.groove(major, pitch, false).root_r, major / 2.0)
    };
    let band = (cyl.radius.min(r_lo), cyl.radius.max(r_hi));
    // As far beyond the face as the groove's section reaches ([`thread_grooves`]).
    let beyond =
        if cyl.internal { ((cyl.radius.min(r_lo) - 0.1 * pitch).max(0.0), cyl.radius) } else { (cyl.radius, cyl.radius.max(r_hi) + 0.05 * pitch) };
    let mut ends = FaceEnds { body: &mb.body, cyl: &cyl, band, beyond, pitch, free: [None; 2] };
    let (lo, hi) = (cyl.start, cyl.end);
    let along = |q: Vec3| (q - cyl.axis_point).dot(cyl.axis);
    let span = |side: usize, to: f64| -> Result<(f64, f64)> {
        let (a, b) = if side == 0 { (lo, to.min(hi)) } else { (to.max(lo), hi) };
        if !(b - a > pitch * 1e-3) {
            return Err(DocError::Invalid("the thread's limit is not along the face from the end it starts at".into()));
        }
        Ok((a, b))
    };
    let from_side = |ends: &mut FaceEnds| start_side(ends, ask.from.map(along), along(p));
    // An external taper thread runs its standard length (ISO 7-1's useful thread, NPT's
    // effective thread L2) unless told otherwise; an internal one the face, as a tapped hole.
    let extent = match (ask.extent, form.is_taper() && !cyl.internal) {
        (Extent::Full, true) => form.pipe_size(major).map_or(Extent::Full, |z| Extent::Length(z.length)),
        (e, _) => e,
    };
    let (a, b, side) = match extent {
        Extent::Full => (lo, hi, ask.from.map(|q| usize::from((along(q) - lo).abs() > (hi - along(q)).abs())).unwrap_or(0)),
        Extent::Length(l) => {
            if !(l > 0.0) {
                return Err(DocError::Invalid("thread length must be positive".into()));
            }
            if l >= hi - lo - pitch * 1e-3 {
                // As long as the face (or longer): it runs the whole face from either end.
                (lo, hi, ask.from.map(|q| usize::from((along(q) - lo).abs() > (hi - along(q)).abs())).unwrap_or(0))
            } else {
                let side = from_side(&mut ends)?;
                let (a, b) = span(side, if side == 0 { lo + l } else { hi - l })?;
                (a, b, side)
            }
        }
        Extent::UpTo(pl) => {
            let n = pl.normal();
            let k = n.dot(*cyl.axis);
            if k.abs() < 1e-9 {
                return Err(DocError::Invalid("the plane the thread runs up to is parallel to its axis".into()));
            }
            let to = n.dot(pl.origin - cyl.axis_point) / k;
            let side = from_side(&mut ends)?;
            let (a, b) = span(side, to)?;
            (a, b, side)
        }
        Extent::UpToPoint(q) => {
            let side = from_side(&mut ends)?;
            let (a, b) = span(side, along(q))?;
            (a, b, side)
        }
    };
    // The runout at each end of the axis: only at the end away from where it starts.
    let e_out = ask.ease_out.0 + ask.ease_out.1 * pitch;
    let ease = if side == 0 { [0.0, e_out] } else { [e_out, 0.0] };
    if ease.iter().any(|e| !(*e >= 0.0 && e.is_finite())) {
        return Err(DocError::Invalid("a runout length must be positive".into()));
    }
    // How each end of the thread meets the face: at a face end, or within half a pitch (and its
    // runout) of one, where the groove would reach past it, that end's free-end test decides.
    let near = |at: f64, side: usize| if side == 0 { at - lo } else { hi - at } < pitch / 2.0 + ease.get(side).copied().unwrap_or(0.0);
    let mut kind = |at: f64, side: usize| -> Result<End> {
        Ok(if !ask.modeled || !near(at, side) {
            End::Inside
        } else if !ends.free(side)? {
            End::Closed
        } else if (if side == 0 { at - lo } else { hi - at }) < pitch * 1e-3 {
            End::Free
        } else {
            End::Inside
        })
    };
    let ends = [kind(a, 0)?, kind(b, 1)?];
    if form.is_taper() && ask.modeled && ends.get(side) != Some(&End::Free) {
        return Err(DocError::Invalid(format!(
            "a taper pipe thread starts at a free end, the pipe's end or the {}: start it there (`from`, a point near that end)",
            if cyl.internal { "hole's mouth" } else { "shaft's end" }
        )));
    }
    let info = ThreadInfo {
        feature,
        body: mb.name.clone(),
        designation,
        major_diameter: major,
        radius: cyl.radius,
        pitch,
        internal: cyl.internal,
        axis_point: cyl.axis_point,
        axis: cyl.axis,
        start: a,
        end: b,
        form,
        hand: ask.hand,
        starts,
    };
    Ok(ThreadOn { info, cyl, ends, steep: ask.steep, ease, from: side })
}

/// The end of the face (0: its start, 1: its end) a thread of limited length starts at: the end
/// nearest `from` (an axial position) when given, else the face's only free end, else the end the
/// pick `at` lies near (within a third of the face). A pick in the middle of a face whose ends are
/// alike is refused rather than guessed.
fn start_side(ends: &mut FaceEnds, from: Option<f64>, at: f64) -> Result<usize> {
    let (lo, hi) = (ends.cyl.start, ends.cyl.end);
    if let Some(s) = from {
        return match (s - lo).abs().total_cmp(&(hi - s).abs()) {
            std::cmp::Ordering::Less => Ok(0),
            std::cmp::Ordering::Greater => Ok(1),
            std::cmp::Ordering::Equal => Err(DocError::Invalid("`from` is as near one end of the face as the other".into())),
        };
    }
    match (ends.free(0)?, ends.free(1)?) {
        (true, false) => return Ok(0),
        (false, true) => return Ok(1),
        _ => {}
    }
    let third = (hi - lo) / 3.0;
    if at - lo < third {
        Ok(0)
    } else if hi - at < third {
        Ok(1)
    } else {
        let alike = if ends.free(0)? { "both ends of the face are free" } else { "material goes on past both ends of the face" };
        Err(DocError::Invalid(format!("{alike}: say which end the thread starts from (`from`, a point near that end)")))
    }
}

/// A thread's groove as a screw sweep: its `section` in `plane` (an axial plane through `base`),
/// carried `turns` turns about `axis` through `base`, rising `pitch` per turn (negative: a
/// left-handed groove).
struct GrooveShape {
    plane: Plane,
    section: Vec<Vec2>,
    base: Vec3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
    /// The section's move off the axis (turn, mm), piecewise linear: the runouts. Empty: none.
    law: Vec<(f64, f64)>,
}

/// The helical spaces between the teeth of a modelled thread of the form's profile: cut from
/// the shaft (external) or the hole wall (internal), running from `start` to `end` along the
/// axis, one groove per start and hand. `radius` is the face's own radius, `face` its extent
/// along the axis and `ends` how the thread's two ends meet it. A taper thread's `taper` is
/// (its major diameter where it starts, the end of the axis it starts from): its diameters grow
/// 1 in 16 from there into an external thread's material and shrink into an internal one's.
fn thread_grooves(
    t: &ThreadInfo,
    radius: f64,
    face: (f64, f64),
    ends: [End; 2],
    steep: SteepFlank,
    ease: [f64; 2],
    taper: Option<(f64, usize)>,
) -> Result<Vec<GrooveShape>> {
    let (p, d, form) = (t.pitch, t.major_diameter, t.form);
    let lead = p * f64::from(t.starts.max(1));
    let len = t.end - t.start;
    if !(p > 1e-3 && len > 1e-6) {
        return Err(DocError::Invalid("thread too short".into()));
    }
    if len / lead > 190.0 {
        return Err(DocError::Invalid(format!(
            "a modelled thread is limited to 190 turns ({:.0} mm at this lead); use a cosmetic one",
            190.0 * lead
        )));
    }
    // The major diameter at axial position z: fixed, or a taper's from where it starts.
    let major_at = |z: f64| match taper {
        Some((d0, from)) => {
            let (z0, into) = if from == 0 { (t.start, 1.0) } else { (t.end, -1.0) };
            let grow = if t.internal { -1.0 } else { 1.0 };
            d0 + grow * into * (z - z0) / 16.0
        }
        None => d,
    };
    let d = taper.map_or(d, |(d0, _)| d0);
    let profile = |d: f64| {
        let mut g = form.groove(d, p, t.internal);
        if steep == SteepFlank::Start {
            std::mem::swap(&mut g.flank_lo, &mut g.flank_hi);
        }
        g
    };
    let g = profile(d);
    let alt = |dia: f64| form.fit(dia, t.internal).map(|(n, q)| form.format(n, q, q)).filter(|a| *a != t.designation);
    // How deep the groove cuts into the material: from the hole wall out to the internal
    // thread's root, or from the shaft's surface in to the external thread's root. Next to
    // nothing (a hole as wide as the thread, #995) is refused: the cut would leave the face
    // unthreaded.
    let basic = g.basic_depth;
    let depth = if t.internal { g.root_r - radius } else { radius - g.root_r };
    let drill = show_mm(form.drill(d, p));
    if !(depth >= basic / 4.0) {
        let (m, dia) = (&t.designation, 2.0 * radius);
        let cut = format!("it would cut {:.2} mm of the thread's {basic:.2} mm depth", depth.max(0.0));
        let shown = show_mm(dia);
        return Err(DocError::Invalid(if t.internal {
            let alt = alt(dia).map(|a| format!(", or thread this hole {a}")).unwrap_or_default();
            format!("this {shown} mm hole is too wide for a modelled {m} thread: {cut}; drill {drill} mm for {m}{alt}")
        } else {
            let alt = alt(dia).map(|a| format!(", or thread this shaft {a}")).unwrap_or_default();
            format!("this {shown} mm shaft is too thin for a modelled {m} thread: {cut}; make the shaft {} mm for {m}{alt}", show_mm(d))
        }));
    }
    // The section: from the root out past the surface (external), or from inside the hole wall
    // out to the root (internal), so the cut leaves no skin, its flanks the groove's.
    // A taper thread's crests are cut by its major cone (external) or minor cone (internal),
    // as the die or tap cuts them ([`taper_envelope`]): its groove reaches past the crest by as
    // much as narrows the tooth's crest flat by a quarter (NPT's flat is 0.038 P). The cone
    // changes 1 in 32 along the axis (`major_at`) across the section's own width, so its edge
    // there follows the cone at that margin rather than being cut square: square, its deeper
    // corner stops short of the cone and leaves a sliver of each flank.
    let cone = taper.map_or(0.0, |_| (major_at(t.start + 1.0) - major_at(t.start)) / 2.0);
    let section_of = |g: &crate::thread_forms::GrooveProfile| -> (Vec<Vec2>, f64) {
        let (tl, th) = (g.flank_lo.tan(), g.flank_hi.tan());
        if taper.is_none() {
            let (r0, r1) = if t.internal { (radius.min(g.crest_r) - 0.1 * p, g.root_r) } else { (g.root_r, radius.max(d / 2.0) + 0.05 * p) };
            let ((lo0, hi0), (lo1, hi1)) = (g.extents(p, r0), g.extents(p, r1));
            let wide = g.width(p, if t.internal { r0 } else { r1 });
            return (vec![Vec2::new(r0, -lo0), Vec2::new(r1, -lo1), Vec2::new(r1, hi1), Vec2::new(r0, hi0)], wide);
        }
        // Depth from the root, out towards the crest: d(z) = dc + q·z along the edge, which
        // meets the flanks z = −(h + d·tl) and z = h + d·th.
        let o = if t.internal { -1.0 } else { 1.0 };
        let q = o * cone;
        let h = g.width(p, g.root_r) / 2.0;
        let dc = o * (g.crest_r - g.root_r) + 0.25 * g.crest_flat / (tl + th);
        let (z_lo, z_hi) = (-(h + dc * tl) / (1.0 + q * tl), (h + dc * th) / (1.0 - q * th));
        let r = |z: f64| g.root_r + o * (dc + q * z);
        let (root, edge) = ([Vec2::new(g.root_r, -h), Vec2::new(g.root_r, h)], [Vec2::new(r(z_lo), z_lo), Vec2::new(r(z_hi), z_hi)]);
        // In the order the square section has: the inner radius's corners first.
        let pts = if t.internal { vec![edge[0], root[0], root[1], edge[1]] } else { vec![root[0], edge[0], edge[1], root[1]] };
        (pts, z_hi - z_lo)
    };
    let (mut section, wide) = section_of(&g);
    let radii = |s: &[Vec2]| s.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v.x), b.max(v.x)));
    let (r0, r1) = radii(&section);
    if wide >= p * 0.98 {
        // The groove would be wider than a pitch: the face is far inside the thread (a hole
        // narrower than its minor diameter, a shaft thicker than its major one, #2688).
        let (m, dia) = (&t.designation, 2.0 * radius);
        let shown = show_mm(dia);
        return Err(DocError::Invalid(if t.internal {
            let alt = alt(dia).map(|a| format!(", or thread this hole {a}")).unwrap_or_default();
            format!("this {shown} mm hole is too narrow for a modelled {m} thread (its teeth would fill the hole); drill {drill} mm for {m}{alt}")
        } else {
            let alt = alt(dia).map(|a| format!(", or thread this shaft {a}")).unwrap_or_default();
            format!("this {shown} mm shaft is too thick for a modelled {m} thread; make the shaft {} mm for {m}{alt}", show_mm(d))
        }));
    }
    if !(r0.is_finite() && r1.is_finite() && r0 > 0.0) {
        return Err(DocError::Invalid("thread dimensions are out of range".into()));
    }
    // Past a free end of the face the groove runs out into the air, a pitch beyond it. Before
    // a closed one it stops with its section clear of the face's end (its centre as far inside
    // as the section reaches that way), so it never cuts into the shoulder there; inside the
    // face it stops square at the threaded length's end.
    //
    // A runout `e` at an end (ISO 3508) fades the groove out over `e`: its section moves out of
    // the material (an external thread's away from the axis, past the surface; an internal
    // one's towards it, into the hole) as it turns. It lies past the full-form length, up to the
    // shoulder's clearance; at a free end, inside the face, the groove reaching nothing there.
    // How far the section reaches each way along the axis.
    let (w_lo, w_hi) = section.iter().fold((0.0_f64, 0.0_f64), |(a, b), v| (a.max(-v.y), b.max(v.y)));
    let s0 = match (ends[0], ease[0] > 0.0) {
        (End::Free, false) => t.start - p,
        (End::Free, true) => t.start,
        (End::Closed, _) => (t.start - ease[0]).max(face.0 + w_lo),
        (End::Inside, _) => t.start - ease[0],
    };
    let s1 = match (ends[1], ease[1] > 0.0) {
        (End::Free, false) => t.end + p,
        (End::Free, true) => t.end,
        (End::Closed, _) => (t.end + ease[1]).min(face.1 - w_hi),
        (End::Inside, _) => t.end + ease[1],
    };
    if !(s1 - s0 > p * 1e-3) {
        return Err(DocError::Invalid(format!(
            "the thread is too short to model between the shoulders: its groove needs {} mm clear of each",
            show_mm(w_lo.max(w_hi))
        )));
    }
    let turns = (s1 - s0) / lead;
    // A taper thread's section is the profile where the sweep begins, moved off the axis as it
    // turns by the taper's 1/32 of the axial travel.
    let mut r0 = r0;
    let taper_rate = match taper {
        Some(_) => {
            section = section_of(&profile(major_at(s0))).0;
            r0 = radii(&section).0;
            (major_at(s0 + lead) - major_at(s0)) / 2.0
        }
        None => 0.0,
    };
    // How far the section moves to leave the material, a little past it.
    let out = if t.internal { -(r1 - radius + 0.05 * p) } else { radius - r0 + 0.05 * p };
    if t.internal && r0 + out < 0.1 * p {
        return Err(DocError::Invalid("the hole is too small for this thread's runout".into()));
    }
    let (r_in, r_out) = (ease[0] / lead, ease[1] / lead);
    let mut law = Vec::new();
    if r_in > 0.0 {
        law.extend([(0.0, out), (r_in, 0.0)]);
    }
    if r_out > 0.0 {
        law.extend([(turns - r_out, 0.0), (turns, out)]);
    }
    if r_in > 0.0 && r_out > 0.0 && !(turns - r_out > r_in + 1e-9) || r_in.max(r_out) > turns {
        return Err(DocError::Invalid(format!("the runout ({} mm) is longer than the thread", show_mm(ease[0].max(ease[1])))));
    }
    if taper_rate != 0.0 {
        law = with_taper(&law, turns, taper_rate);
        if t.internal && law.iter().any(|(_, o)| r0 + o < 0.1 * p) {
            return Err(DocError::Invalid("the hole is too small for this taper thread".into()));
        }
    }
    let k = *t.axis;
    let u = k.cross(if k.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) });
    let u = u.normalized().ok_or_else(|| DocError::Invalid("thread axis".into()))?;
    let base = t.axis_point + k * s0;
    let hands: &[f64] = match t.hand {
        ThreadHand::Right => &[1.0],
        ThreadHand::Left => &[-1.0],
        ThreadHand::Both => &[1.0, -1.0],
    };
    let n = t.starts.max(1);
    let mut grooves = Vec::new();
    for &h in hands {
        for j in 0..n {
            // Start j is the first turned a j-th of a turn on: the same helix a j-th of the
            // lead along.
            let a = std::f64::consts::TAU * f64::from(j) / f64::from(n);
            let x = u * a.cos() + k.cross(u) * a.sin();
            // A left-handed groove: turning the same way about the reversed axis while rising
            // along it (the screw motion about −k rising −lead per turn).
            let (axis, pitch) = if h > 0.0 { (k, lead) } else { (-k, -lead) };
            grooves.push(GrooveShape {
                plane: Plane { origin: base, x, y: k },
                section: section.clone(),
                base,
                axis,
                pitch,
                turns,
                law: law.clone(),
            });
        }
    }
    Ok(grooves)
}

/// A radial law (turn, offset) with a taper's `rate` mm per turn added: the law's own points and
/// both ends, each offset the law's (held past its first and last points) plus the taper's.
fn with_taper(law: &[(f64, f64)], turns: f64, rate: f64) -> Vec<(f64, f64)> {
    let at = |t: f64| -> f64 {
        match (law.first(), law.last()) {
            (Some(a), _) if t <= a.0 => a.1,
            (_, Some(b)) if t >= b.0 => b.1,
            _ => law
                .windows(2)
                .find_map(|w| match w {
                    [a, b] if t >= a.0 && t <= b.0 && b.0 > a.0 => Some(a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0)),
                    _ => None,
                })
                .unwrap_or(0.0),
        }
    };
    let mut ts: Vec<f64> = law.iter().map(|(t, _)| *t).chain([0.0, turns]).collect();
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    ts.into_iter().map(|t| (t, at(t) + rate * t)).collect()
}

/// A taper thread's crests, as its die or tap cuts them: the ring of material outside its major
/// cone (external) or inside its minor cone (internal) along the thread, from a pitch past the
/// end it starts at (`from`, a free end; `d0` its major diameter there) to its other end (a pitch
/// past that too when it is free). None when the cone misses the material.
fn taper_envelope(t: &ThreadInfo, radius: f64, ends: [End; 2], (d0, from): (f64, usize)) -> Result<Option<Body>> {
    let p = t.pitch;
    let k = *t.axis;
    let u = k.cross(if k.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) });
    let u = u.normalized().ok_or_else(|| DocError::Invalid("thread axis".into()))?;
    let (at, into) = if from == 0 { (t.start, k) } else { (t.end, -k) };
    let far = 1 - from.min(1);
    let len = t.end - t.start + if ends.get(far) == Some(&End::Free) { p } else { 0.0 };
    let e = p;
    // In the plane: x radial, y along the axis into the material from the start.
    let plane = Plane { origin: t.axis_point + k * at, x: u, y: into };
    let pts = if t.internal {
        // The minor cone shrinks into the hole: what lies inside it, down to well inside the hole.
        let minor0 = t.form.groove(d0, p, true).crest_r;
        let rm = |y: f64| minor0 - y / 32.0;
        if !(rm(-e) > radius) {
            return Ok(None);
        }
        let r_in = 0.5 * radius.min(rm(len));
        if !(r_in > 0.0) {
            return Err(DocError::Invalid("the hole is too small for this taper thread".into()));
        }
        vec![Vec2::new(r_in, -e), Vec2::new(rm(-e), -e), Vec2::new(rm(len), len), Vec2::new(r_in, len)]
    } else {
        // The major cone grows into the shaft: what lies outside it, out past the surface.
        let rc = |y: f64| d0 / 2.0 + y / 32.0;
        let r_out = radius + 0.1 * p;
        if !(rc(-e) < r_out) {
            return Ok(None);
        }
        let hit = (r_out - d0 / 2.0) * 32.0;
        if hit < len {
            vec![Vec2::new(rc(-e), -e), Vec2::new(r_out, hit), Vec2::new(r_out, -e)]
        } else {
            vec![Vec2::new(rc(-e), -e), Vec2::new(rc(len), len), Vec2::new(r_out, len), Vec2::new(r_out, -e)]
        }
    };
    let region = Region2 { outer: solidcraft_geom::Loop2::polygon(&pts), holes: Vec::new() };
    let mut ring = kernel::revolve(&plane, &[region], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), std::f64::consts::TAU)?;
    Ok(ring.pop())
}

/// Evaluate a thread feature: a cosmetic thread is listed in the model's threads, a modelled
/// one cut into its body.
pub(super) fn eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::Thread {
        face,
        designation,
        length,
        modeled,
        from,
        up_to,
        form,
        hand,
        starts,
        ease_out,
        tip,
        chamfer_angle,
        chamfer_depth,
        steep_flank,
    } = &f.kind
    else {
        return Err(DocError::Invalid("not a thread".into()));
    };
    if *tip != ThreadTip::None && !*modeled {
        return Err(DocError::Invalid("a chamfered tip is cut only on a modelled thread".into()));
    }
    let extent = match (length, up_to) {
        (Some(e), _) => Extent::Length(val(vals, e, Kind::Length)?),
        (None, Some(crate::ThreadLimit::Face(q))) => Extent::UpToPoint(q.0),
        (None, Some(crate::ThreadLimit::Plane(pl))) => Extent::UpTo(doc.resolve_plane_in(vals, pl, 0, Some(st))?),
        (None, None) => Extent::Full,
    };
    // A form left at its default is the one the designation names (`Tr20x4`).
    let form = (*form != ThreadForm::Metric).then_some(*form);
    let ask = ThreadAsk {
        form,
        designation: designation.as_deref(),
        hand: *hand,
        starts: *starts,
        steep: *steep_flank,
        extent,
        from: from.map(|q| q.0),
        modeled: *modeled,
        ease_out: (0.0, 0.0),
        tapped: false,
    };
    if ease_out.is_some() && !*modeled {
        return Err(DocError::Invalid("a runout is cut only on a modelled thread".into()));
    }
    let ask = ThreadAsk { ease_out: runout(vals, ease_out)?, ..ask };
    let on = thread_on(st, f.id, face.0, &ask)?;
    if !*modeled {
        st.threads.push(on.info);
        return Ok(());
    }
    let angle = chamfer_angle.as_deref().map(|e| val(vals, e, Kind::Angle)).transpose()?;
    let depth = chamfer_depth.as_deref().map(|e| val(vals, e, Kind::Length)).transpose()?;
    cut(st, on, (*tip, angle, depth))
}

/// A runout as (mm, pitches): ISO 3508's standard 2.5 P and short 1.25 P, or a length.
pub(super) fn runout(vals: &BTreeMap<String, Value>, e: &Option<crate::ThreadRunout>) -> Result<(f64, f64)> {
    Ok(match e {
        None => (0.0, 0.0),
        Some(crate::ThreadRunout::Standard) => (0.0, 2.5),
        Some(crate::ThreadRunout::Short) => (0.0, 1.25),
        Some(crate::ThreadRunout::Length { length }) => (val(vals, length, Kind::Length)?, 0.0),
    })
}

/// Cut a modelled thread resolved on its face into its body: the grooves (SMLib's exact screw
/// sweeps, a taper's crests by its envelope) by one B-rep boolean, then the chamfered tip
/// (`tip`, with its angle and depth when given).
pub(super) fn cut(st: &mut ModelState, on: ThreadOn, (tip, angle, depth): (ThreadTip, Option<f64>, Option<f64>)) -> Result<()> {
    let ThreadOn { info: t, cyl, ends, steep, ease, from: from_side } = on;
    let i = st.bodies.iter().position(|b| b.name == t.body).ok_or_else(|| DocError::Invalid("threaded body".into()))?;
    let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
    let (input, radius) = (mb.traced(), cyl.radius);
    // The exact grooves: SMLib's screw sweeps, cut by one B-rep boolean.
    // A taper thread starts at its free end at the major diameter its standard puts there: an
    // internal one's gauge (reference) plane at its face, an external one's that far in.
    let taper = match t.form.is_taper() {
        true => {
            let z = t.form.pipe_size(t.major_diameter).ok_or_else(|| DocError::Invalid(format!("no pipe size {}", t.designation)))?;
            Some((if t.internal { t.major_diameter } else { t.major_diameter - z.gauge / 16.0 }, from_side))
        }
        false => None,
    };
    let mut grooves = Vec::new();
    for g in thread_grooves(&t, radius, (cyl.start, cyl.end), ends, steep, ease, taper)? {
        let section = Region2 { outer: solidcraft_geom::Loop2::polygon(&g.section), holes: Vec::new() };
        grooves.push(kernel::sweep_helix_radial(&g.plane, &section, g.base, g.axis, g.pitch, g.turns, &g.law)?);
    }
    if let Some(taper) = taper
        && let Some(envelope) = taper_envelope(&t, radius, ends, taper)?
    {
        grooves.push(envelope);
    }
    let tool = match grooves.as_slice() {
        [one] => one.clone(),
        many => kernel::union_all(&many.iter().collect::<Vec<_>>())?,
    };
    let (nb, groove_faces) = (kernel::boolean(&input.body, &tool, BoolOp::Cut)?, tool.face_count());
    let nb = nb.ok_or_else(|| DocError::Invalid("the thread removed everything".into()))?;
    // The groove's faces have no role to name them by.
    let groove = crate::lineage::Lineage::unnamed_faces(groove_faces);
    let lineage = input.lineage.and_then(|l| crate::lineage::Lineage::step(&nb, &[&l, &groove]));
    let mut nb = Traced { body: nb, lineage };
    if tip != ThreadTip::None {
        for cone in tip_cones(&t, radius, ends, (tip, from_side), angle, depth, taper)? {
            let faces = cone.face_count();
            let cut = kernel::boolean(&nb.body, &cone, BoolOp::Cut)?.ok_or_else(|| DocError::Invalid("the chamfer removed everything".into()))?;
            let unnamed = crate::lineage::Lineage::unnamed_faces(faces);
            let lineage = nb.lineage.and_then(|l| crate::lineage::Lineage::step(&cut, &[&l, &unnamed]));
            nb = Traced { body: cut, lineage };
        }
    }
    if let Some(slot) = st.bodies.get_mut(i) {
        *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
    }
    Ok(())
}

/// The chamfered tip of an external thread (ISO 4753, chamfered end CH) at the free ends `tip`
/// names (`Start`: the end of the axis `from`, which must be free; `Both`: each free end):
/// the material outside a cone `angle` to the axis (default 45°) that reaches the diameter
/// d_f = d - 2 `depth` at the face's end, as a ring to cut after the groove, so the last teeth
/// taper into it. ISO 4753 bounds d_f by the external thread's minor diameter; the default is
/// that bound, d3 = d - 1.226869 P (ISO 724, the rounded root of the design profile). It lies
/// below the modelled basic root d1, so the cone crosses the root cylinder and leaves a whole
/// ring on the end; one ending on the root itself would pinch to nothing wherever the groove
/// reaches the end. ISO 4753 is for ISO metric threads: other forms give `depth`. A taper
/// thread's `taper` (its major diameter where it starts, the end it starts from) puts each end's
/// major diameter where the taper has it there, and `depth` is measured from that.
fn tip_cones(
    t: &ThreadInfo,
    radius: f64,
    ends: [End; 2],
    (tip, from): (ThreadTip, usize),
    angle: Option<f64>,
    depth: Option<f64>,
    taper: Option<(f64, usize)>,
) -> Result<Vec<Body>> {
    if t.internal {
        return Err(DocError::Invalid("a chamfered tip is for an external thread (ISO 4753); countersink the hole's entry instead".into()));
    }
    let g = t.form.groove(t.major_diameter, t.pitch, false);
    let a = angle.unwrap_or(std::f64::consts::FRAC_PI_4);
    if !(a > 1f64.to_radians() && a < 89f64.to_radians()) {
        return Err(DocError::Invalid("the chamfer's angle to the axis must be between 1° and 89°".into()));
    }
    // The major diameter at each end of the axis: a taper's grows 1 in 16 from where it starts.
    let major = |side: usize| match taper {
        Some((d0, start)) if side == start => d0,
        Some((d0, _)) => d0 + (t.end - t.start) / 16.0,
        None => t.major_diameter,
    };
    let depth = match (depth, t.form) {
        (Some(d), _) => d,
        // ISO 724: d3 = d1 - H/6 = d - 2 (17/24) H.
        (None, ThreadForm::Metric) => 17.0 / 24.0 * t.pitch * 3f64.sqrt() / 2.0,
        (None, form) => {
            return Err(DocError::Invalid(format!(
                "ISO 4753's chamfered end is for ISO metric threads: give a {} thread its chamfer depth (its root is {} mm deep)",
                form.name(),
                show_mm(t.major_diameter / 2.0 - g.root_r)
            )));
        }
    };
    let smallest = major(0).min(major(1)) / 2.0;
    if !(depth > 0.0 && smallest - depth > 1e-3) {
        return Err(DocError::Invalid(format!("the chamfer's depth must be between 0 and {} mm", show_mm(smallest))));
    }
    let k = *t.axis;
    let u = k.cross(if k.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) });
    let u = u.normalized().ok_or_else(|| DocError::Invalid("thread axis".into()))?;
    let slope = a.tan();
    // Past the end only the radii the groove's overrun already reaches (r >= d_f / 2), so
    // nothing past a free end beyond the thread is touched.
    let e = 0.5 * t.pitch;
    let mut out = Vec::new();
    let free = |side: usize| ends.get(side) == Some(&End::Free);
    if tip == ThreadTip::Start && !free(from) {
        return Err(DocError::Invalid("the end the thread starts from is not a free end (material goes on past it, or the thread stops inside the face): chamfer it with `tip: both`, or start the thread from its free end".into()));
    }
    for (side, at, out_dir) in [(0usize, t.start, -k), (1, t.end, k)] {
        if !free(side) || (tip == ThreadTip::Start && side != from) {
            continue;
        }
        let r_f = major(side) / 2.0 - depth;
        let r_out = radius.max(major(side) / 2.0) + 0.1 * t.pitch;
        // In the plane: x radial, y outward along the axis from the end.
        let plane = Plane { origin: t.axis_point + k * at, x: u, y: out_dir };
        let pts = [Vec2::new(r_f, 0.0), Vec2::new(r_f, e), Vec2::new(r_out, e), Vec2::new(r_out, -(r_out - r_f) / slope)];
        let region = Region2 { outer: solidcraft_geom::Loop2::polygon(&pts), holes: Vec::new() };
        let mut ring = kernel::revolve(&plane, &[region], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), std::f64::consts::TAU)?;
        out.push(ring.pop().ok_or_else(|| DocError::Invalid("chamfer".into()))?);
    }
    if out.is_empty() {
        return Err(DocError::Invalid("a chamfered tip needs a free end of the thread (one with no material past it)".into()));
    }
    Ok(out)
}
