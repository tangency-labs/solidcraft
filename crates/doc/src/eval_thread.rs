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
    &crate::thread_forms::ISO_COARSE
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
    /// Runouts at the end the thread starts from and at the other, each as (mm, pitches): its
    /// length is mm + pitches x the pitch.
    pub ease: [(f64, f64); 2],
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
}

/// The free-end test of a face's two ends (`side` 0 at its start, 1 at its end), each made
/// once: whether any material lies past the end, over the axial span the groove's one-pitch
/// overrun reaches beyond its first pitch, within the radii the groove cuts.
struct FaceEnds<'a> {
    body: &'a Body,
    cyl: &'a kernel::CylinderFace,
    band: (f64, f64),
    pitch: f64,
    free: [Option<bool>; 2],
}

impl FaceEnds<'_> {
    fn free(&mut self, side: usize) -> Result<bool> {
        if let Some(Some(f)) = self.free.get(side) {
            return Ok(*f);
        }
        let p = self.pitch;
        let along = if side == 0 { (self.cyl.start - 1.5 * p, self.cyl.start - p) } else { (self.cyl.end + p, self.cyl.end + 1.5 * p) };
        // Material that only touches the band's rims (a counterbore as wide as the thread) is
        // not in the groove's way.
        let e = p * 1e-3;
        let free = !kernel::material_in_ring(self.body, self.cyl, self.band.0 + e, self.band.1 - e, along)?;
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
    let size = match ask.designation {
        Some(s) => form.parse(s, starts).map_err(DocError::Invalid)?,
        None => {
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
    let designation = form.format(major, pitch, size.lead);
    let minor = 2.0 * form.groove(major, pitch, true).crest_r;
    let fits = if cyl.internal {
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
    let mut ends = FaceEnds { body: &mb.body, cyl: &cyl, band, pitch, free: [None; 2] };
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
    let (a, b, side) = match ask.extent {
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
    // The runout at each end of the axis: ease in where the thread starts from.
    let len = |(mm, pitches): (f64, f64)| mm + pitches * pitch;
    let (e_in, e_out) = (len(ask.ease[0]), len(ask.ease[1]));
    let ease = if side == 0 { [e_in, e_out] } else { [e_out, e_in] };
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
    let info = ThreadInfo {
        feature,
        body: mb.name.clone(),
        designation,
        major_diameter: major,
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
    Ok(ThreadOn { info, cyl, ends, steep: ask.steep, ease })
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
/// along the axis and `ends` how the thread's two ends meet it.
fn thread_grooves(t: &ThreadInfo, radius: f64, face: (f64, f64), ends: [End; 2], steep: SteepFlank, ease: [f64; 2]) -> Result<Vec<GrooveShape>> {
    let (p, d, form) = (t.pitch, t.major_diameter, t.form);
    let lead = p * f64::from(t.starts.max(1));
    let len = t.end - t.start;
    if !(p > 1e-3 && len > 1e-6) {
        return Err(DocError::Invalid("thread too short".into()));
    }
    if len / lead > 190.0 {
        return Err(DocError::Invalid(format!("a modelled thread is limited to 190 turns ({:.0} mm at this lead); use a cosmetic one", 190.0 * lead)));
    }
    let mut g = form.groove(d, p, t.internal);
    if steep == SteepFlank::Start {
        std::mem::swap(&mut g.flank_lo, &mut g.flank_hi);
    }
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
    // The section's radial span: from the root out past the surface (external), or from inside
    // the hole wall out to the root (internal), so the cut leaves no skin.
    let (r0, r1) = if t.internal { (radius.min(g.crest_r) - 0.1 * p, g.root_r) } else { (g.root_r, radius.max(d / 2.0) + 0.05 * p) };
    let wide = if t.internal { r0 } else { r1 };
    if g.width(p, wide) >= p * 0.98 {
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
    let (lo0, hi0) = g.extents(p, r0);
    let (lo1, hi1) = g.extents(p, r1);
    let section = vec![Vec2::new(r0, -lo0), Vec2::new(r1, -lo1), Vec2::new(r1, hi1), Vec2::new(r0, hi0)];
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
    let (w_lo, w_hi) = (lo0.max(lo1), hi0.max(hi1));
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
        return Err(DocError::Invalid(format!("the runouts ({} and {} mm) are longer than the thread", show_mm(ease[0]), show_mm(ease[1]))));
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
            grooves.push(GrooveShape { plane: Plane { origin: base, x, y: k }, section: section.clone(), base, axis, pitch, turns, law: law.clone() });
        }
    }
    Ok(grooves)
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
        ease_in,
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
        ease: [(0.0, 0.0); 2],
    };
    // ISO 3508's runouts: standard 2.5 P, short 1.25 P.
    let runout = |e: &Option<crate::ThreadRunout>| -> Result<(f64, f64)> {
        Ok(match e {
            None => (0.0, 0.0),
            Some(crate::ThreadRunout::Standard) => (0.0, 2.5),
            Some(crate::ThreadRunout::Short) => (0.0, 1.25),
            Some(crate::ThreadRunout::Length { length }) => (val(vals, length, Kind::Length)?, 0.0),
        })
    };
    if (ease_in.is_some() || ease_out.is_some()) && !*modeled {
        return Err(DocError::Invalid("a runout is cut only on a modelled thread".into()));
    }
    let ask = ThreadAsk { ease: [runout(ease_in)?, runout(ease_out)?], ..ask };
    let ThreadOn { info: t, cyl, ends, steep, ease } = thread_on(st, f.id, face.0, &ask)?;
    if !*modeled {
        st.threads.push(t);
        return Ok(());
    }
    let i = st.bodies.iter().position(|b| b.name == t.body).ok_or_else(|| DocError::Invalid("threaded body".into()))?;
    let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
    // The exact grooves: SMLib's screw sweeps, cut by one B-rep boolean.
    let mut grooves = Vec::new();
    for g in thread_grooves(&t, cyl.radius, (cyl.start, cyl.end), ends, steep, ease)? {
        let section = Region2 { outer: solidcraft_geom::Loop2::polygon(&g.section), holes: Vec::new() };
        grooves.push(kernel::sweep_helix_radial(&g.plane, &section, g.base, g.axis, g.pitch, g.turns, &g.law)?);
    }
    let tool = match grooves.as_slice() {
        [one] => one.clone(),
        many => kernel::union_all(&many.iter().collect::<Vec<_>>())?,
    };
    let (nb, groove_faces) = (kernel::boolean(&mb.body, &tool, BoolOp::Cut)?, tool.face_count());
    let nb = nb.ok_or_else(|| DocError::Invalid("the thread removed everything".into()))?;
    // The groove's faces have no role to name them by.
    let groove = crate::lineage::Lineage::unnamed_faces(groove_faces);
    let lineage = mb.traced().lineage.and_then(|l| crate::lineage::Lineage::step(&nb, &[&l, &groove]));
    let mut nb = Traced { body: nb, lineage };
    if *tip == ThreadTip::Chamfer {
        let angle = chamfer_angle.as_deref().map(|e| val(vals, e, Kind::Angle)).transpose()?;
        let depth = chamfer_depth.as_deref().map(|e| val(vals, e, Kind::Length)).transpose()?;
        for cone in tip_cones(&t, cyl.radius, ends, angle, depth)? {
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

/// The chamfered tip of an external thread (ISO 4753, chamfered end CH) at each of its free ends:
/// the material outside a cone `angle` to the axis (default 45°) that reaches the diameter
/// d_f = d - 2 `depth` at the face's end (default: the external thread's minor diameter, the
/// largest ISO 4753 allows), as a ring to cut after the groove, so the last teeth taper into it.
fn tip_cones(t: &ThreadInfo, radius: f64, ends: [End; 2], angle: Option<f64>, depth: Option<f64>) -> Result<Vec<Body>> {
    if t.internal {
        return Err(DocError::Invalid("a chamfered tip is for an external thread (ISO 4753); countersink the hole's entry instead".into()));
    }
    let g = t.form.groove(t.major_diameter, t.pitch, false);
    let a = angle.unwrap_or(std::f64::consts::FRAC_PI_4);
    if !(a > 1f64.to_radians() && a < 89f64.to_radians()) {
        return Err(DocError::Invalid("the chamfer's angle to the axis must be between 1° and 89°".into()));
    }
    let r_top = radius.max(t.major_diameter / 2.0);
    let depth = depth.unwrap_or(t.major_diameter / 2.0 - g.root_r);
    let r_f = t.major_diameter / 2.0 - depth;
    if !(depth > 0.0 && r_f > 1e-3) {
        return Err(DocError::Invalid(format!("the chamfer's depth must be between 0 and {} mm", show_mm(t.major_diameter / 2.0))));
    }
    let k = *t.axis;
    let u = k.cross(if k.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) });
    let u = u.normalized().ok_or_else(|| DocError::Invalid("thread axis".into()))?;
    let (r_out, slope) = (r_top + 0.1 * t.pitch, a.tan());
    // Past the end only the radii the groove's overrun already reaches (r >= d_f / 2), so
    // nothing past a free end beyond the thread is touched.
    let e = 0.5 * t.pitch;
    let mut out = Vec::new();
    for (side, at, out_dir) in [(0usize, t.start, -k), (1, t.end, k)] {
        if ends.get(side) != Some(&End::Free) {
            continue;
        }
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
