//! Threads: the ISO metric sizes, the cosmetic thread on a cylindrical face, and the modelled
//! thread's groove cut along a helix.

use super::*;

/// A cosmetic thread: on which cylinder, its size, and the threaded part of the axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub feature: u64,
    pub body: String,
    pub designation: String,
    pub major_diameter: f64,
    /// Radius of the actual cylindrical face when the cosmetic thread is evaluated.
    pub radius: f64,
    pub pitch: f64,
    pub internal: bool,
    pub axis_point: Vec3,
    pub axis: UnitVec3,
    /// Threaded part along the axis from `axis_point`.
    pub start: f64,
    pub end: f64,
}

/// ISO metric coarse threads: (nominal diameter, pitch).
const ISO_COARSE: [(f64, f64); 22] = [
    (1.6, 0.35),
    (2.0, 0.4),
    (2.5, 0.45),
    (3.0, 0.5),
    (4.0, 0.7),
    (5.0, 0.8),
    (6.0, 1.0),
    (8.0, 1.25),
    (10.0, 1.5),
    (12.0, 1.75),
    (14.0, 2.0),
    (16.0, 2.0),
    (18.0, 2.5),
    (20.0, 2.5),
    (22.0, 2.5),
    (24.0, 3.0),
    (27.0, 3.0),
    (30.0, 3.5),
    (36.0, 4.0),
    (42.0, 4.5),
    (48.0, 5.0),
    (56.0, 5.5),
];

/// Nominal diameters and coarse pitches (mm) used for automatic ISO metric thread sizing.
/// Other diameter/pitch pairs can be supplied as explicit designations.
pub fn metric_thread_sizes() -> &'static [(f64, f64)] {
    &ISO_COARSE
}

/// "M8" or "M8x1" → (nominal diameter, pitch).
pub fn parse_metric_thread(s: &str) -> Option<(f64, f64)> {
    let t = s.trim().to_ascii_uppercase();
    let body = t.strip_prefix('M')?;
    let (d, p) = match body.split_once(['X', '×']) {
        Some((d, p)) => (d.trim().parse::<f64>().ok()?, Some(p.trim().parse::<f64>().ok()?)),
        None => (body.trim().parse::<f64>().ok()?, None),
    };
    let pitch = match p {
        Some(p) => p,
        None => ISO_COARSE.iter().find(|(n, _)| (n - d).abs() < 1e-9)?.1,
    };
    (d.is_finite() && pitch.is_finite() && d > 0.0 && pitch > 0.0 && pitch < d).then_some((d, pitch))
}

/// The metric thread a cylinder of this diameter takes: a shaft is the major diameter, a hole
/// the tap drill (major − pitch).
fn thread_for(diameter: f64, internal: bool) -> Option<(f64, f64)> {
    ISO_COARSE
        .iter()
        .map(|&(n, p)| (n, p, if internal { n - p } else { n }))
        .min_by(|a, b| (a.2 - diameter).abs().total_cmp(&(b.2 - diameter).abs()))
        .filter(|(_, _, want)| (want - diameter).abs() <= 0.15 * diameter)
        .map(|(n, p, _)| (n, p))
}

pub(crate) fn format_thread(d: f64, p: f64) -> String {
    let coarse = ISO_COARSE.iter().any(|(n, q)| (n - d).abs() < 1e-9 && (q - p).abs() < 1e-9);
    let num = |x: f64| if (x - x.round()).abs() < 1e-9 { format!("{}", x.round()) } else { format!("{x}") };
    if coarse { format!("M{}", num(d)) } else { format!("M{}x{}", num(d), num(p)) }
}

/// The automatic ISO metric designation for a cylindrical face, using the same fit check as
/// a thread feature without an explicit size. This query does not change the model.
pub fn suggested_thread(st: &ModelState, p: Vec3) -> Result<String> {
    thread_on(st, 0, p, &ThreadAsk::default()).map(|t| t.info.designation)
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
    pub designation: Option<&'a str>,
    pub extent: Extent,
    /// A point near the end of the face the thread starts at.
    pub from: Option<Vec3>,
    /// A modelled thread needs to know how each of its ends meets the face's ends.
    pub modeled: bool,
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
    let (major, pitch) = match ask.designation {
        Some(s) => parse_metric_thread(s).ok_or_else(|| DocError::Invalid(format!("unknown thread `{s}` (ISO metric, like M8 or M8x1)")))?,
        None => thread_for(dia, cyl.internal).ok_or_else(|| DocError::Invalid(format!("no metric thread fits a {dia:.2} mm cylinder")))?,
    };
    let (major, pitch) = match &tapped_size {
        Some(size) => (size.major_diameter, size.pitch),
        None => match designation {
            Some(s) => parse_metric_thread(s).ok_or_else(|| DocError::Invalid(format!("unknown thread `{s}` (ISO metric, like M8 or M8x1)")))?,
            None => thread_for(dia, cyl.internal).ok_or_else(|| DocError::Invalid(format!("no metric thread fits a {dia:.2} mm cylinder")))?,
        },
    };
    let name = tapped_size.as_ref().map(|s| s.designation.clone()).unwrap_or_else(|| format_thread(major, pitch));
    let fits = if let Some(size) = tapped_size.as_ref().filter(|s| s.family != crate::TappedThreadFamily::Metric) {
        // Fit the recommended physical drill, without borrowing an ISO minor-diameter rule.
        cyl.internal && (size.tap_drill - dia).abs() < 0.15 * major
    } else if cyl.internal {
        (major - pitch * 1.0825 - dia).abs() < 0.2 * major || (major - pitch - dia).abs() < 0.15 * major
    } else {
        (major - dia).abs() < 0.15 * major || (turned && dia > major)
    };
    if !fits {
        return Err(DocError::Invalid(format!("an {} thread does not fit a {dia:.2} mm {}", name, if cyl.internal { "hole" } else { "shaft" })));
    }
    // The radii the groove cuts: from the minor diameter (ISO 68-1: 5H/8 under the major) to
    // the major, and to the face itself where it lies outside that.
    let minor_r = major / 2.0 - 5.0 * pitch * 3f64.sqrt() / 16.0;
    let band = (cyl.radius.min(minor_r), cyl.radius.max(major / 2.0));
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
    let (a, b) = match ask.extent {
        Extent::Full => (lo, hi),
        Extent::Length(l) => {
            if !(l > 0.0) {
                return Err(DocError::Invalid("thread length must be positive".into()));
            }
            if l >= hi - lo - pitch * 1e-3 {
                // As long as the face (or longer): it runs the whole face from either end.
                (lo, hi)
            } else {
                let side = start_side(&mut ends, ask.from.map(along), along(p))?;
                span(side, if side == 0 { lo + l } else { hi - l })?
            }
        }
        Extent::UpTo(pl) => {
            let n = pl.normal();
            let k = n.dot(*cyl.axis);
            if k.abs() < 1e-9 {
                return Err(DocError::Invalid("the plane the thread runs up to is parallel to its axis".into()));
            }
            let to = n.dot(pl.origin - cyl.axis_point) / k;
            span(start_side(&mut ends, ask.from.map(along), along(p))?, to)?
        }
        Extent::UpToPoint(q) => span(start_side(&mut ends, ask.from.map(along), along(p))?, along(q))?,
    };
    // How each end of the thread meets the face: at a face end, or within half a pitch of one
    // (where the groove would reach past it), that end's free-end test decides.
    let near = |at: f64, side: usize| if side == 0 { at - lo } else { hi - at } < pitch / 2.0;
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
        designation: format_thread(major, pitch),
        major_diameter: major,
        radius: cyl.radius,
        pitch,
        internal: cyl.internal,
        axis_point: cyl.axis_point,
        axis: cyl.axis,
        start: a,
        end: b,
    };
    Ok(ThreadOn { info, cyl, ends })
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

/// A thread's groove as a screw sweep: its trapezoid `section` in `plane` (an axial plane
/// through `base`), carried `turns` turns about `axis` through `base`, rising `pitch` per turn.
struct GrooveShape {
    plane: Plane,
    section: [Vec2; 4],
    base: Vec3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
}

/// The helical space between the teeth of a modelled ISO metric thread (ISO 68-1 basic
/// profile, 60° flanks): cut from the shaft (external) or the hole wall (internal), running from
/// `start` to `end` along the axis. `radius` is the face's own radius, `face` its extent along
/// the axis and `ends` how the thread's two ends meet it.
fn thread_groove_shape(t: &ThreadInfo, radius: f64, face: (f64, f64), ends: [End; 2]) -> Result<GrooveShape> {
    let (p, d) = (t.pitch, t.major_diameter);
    let h = p * 3f64.sqrt() / 2.0;
    let slope = (std::f64::consts::PI / 6.0).tan();
    let len = t.end - t.start;
    if !(p > 1e-3 && len > 1e-6) {
        return Err(DocError::Invalid("thread too short".into()));
    }
    if len / p > 190.0 {
        return Err(DocError::Invalid(format!("a modelled thread is limited to 190 turns ({:.0} mm at this pitch); use a cosmetic one", 190.0 * p)));
    }
    // How deep the groove cuts into the material: from the hole wall out to the major diameter,
    // or from the shaft's surface in to the root. Next to nothing (a hole as wide as the thread,
    // #995) is refused: the faceted cut would leave the face faceted and unthreaded.
    let basic = 5.0 * h / 8.0;
    let depth = if t.internal { d / 2.0 - radius } else { radius - (d / 2.0 - basic) };
    if !(depth >= basic / 4.0) {
        let (m, dia) = (&t.designation, 2.0 * radius);
        let cut = format!("it would cut {:.2} mm of the thread's {basic:.2} mm depth", depth.max(0.0));
        let alt = thread_for(dia, t.internal).map(|(n, q)| format_thread(n, q)).filter(|a| a != m);
        let shown = show_mm(dia);
        return Err(DocError::Invalid(if t.internal {
            let alt = alt.map(|a| format!(", or thread this hole {a}")).unwrap_or_default();
            format!("this {shown} mm hole is too wide for a modelled {m} thread: {cut}; drill {} mm for {m}{alt}", show_mm(d - p))
        } else {
            let alt = alt.map(|a| format!(", or thread this shaft {a}")).unwrap_or_default();
            format!("this {shown} mm shaft is too thin for a modelled {m} thread: {cut}; make the shaft {} mm for {m}{alt}", show_mm(d))
        }));
    }
    // (radius, half width of the groove) at its two ends: a trapezoid in the axial plane.
    let (r0, w0, r1, w1) = if t.internal {
        // From inside the hole wall out to the major diameter, where the bolt's crest sits
        // (the root flat is P/8 wide).
        let r_in = radius.min(d / 2.0 - 5.0 * h / 8.0) - 0.1 * p;
        (r_in, (p / 8.0 + 2.0 * (d / 2.0 - r_in) * slope) / 2.0, d / 2.0, p / 16.0)
    } else {
        // From the root (5H/8 under the major diameter, flat P/4 wide) out past the surface.
        let r_root = d / 2.0 - 5.0 * h / 8.0;
        let r_out = radius.max(d / 2.0) + 0.05 * p;
        (r_root, p / 8.0, r_out, (p / 4.0 + 2.0 * (r_out - r_root) * slope) / 2.0)
    };
    if w0.max(w1) * 2.0 >= p * 0.98 {
        // The groove would be wider than a pitch: the face is far inside the thread (a hole
        // narrower than its minor diameter, a shaft thicker than its major one, #2688).
        let (m, dia) = (&t.designation, 2.0 * radius);
        let alt = thread_for(dia, t.internal).map(|(n, q)| format_thread(n, q)).filter(|a| a != m);
        let shown = show_mm(dia);
        return Err(DocError::Invalid(if t.internal {
            let alt = alt.map(|a| format!(", or thread this hole {a}")).unwrap_or_default();
            format!(
                "this {shown} mm hole is too narrow for a modelled {m} thread (its teeth would fill the hole); drill {} mm for {m}{alt}",
                show_mm(d - p)
            )
        } else {
            let alt = alt.map(|a| format!(", or thread this shaft {a}")).unwrap_or_default();
            format!("this {shown} mm shaft is too thick for a modelled {m} thread; make the shaft {} mm for {m}{alt}", show_mm(d))
        }));
    }
    let k = t.axis;
    let u = k.cross(if k.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) });
    let u = u.normalized().ok_or_else(|| DocError::Invalid("thread axis".into()))?;
    // Past a free end of the face the groove runs out into the air, a pitch beyond it. Before
    // a closed one it stops with its widest part (half `w` either side of its centre) clear of
    // the face's end, so it never cuts into the shoulder there; inside the face it stops square
    // at the threaded length's end.
    let w = w0.max(w1);
    let s0 = match ends[0] {
        End::Free => t.start - p,
        End::Closed => t.start.max(face.0 + w),
        End::Inside => t.start,
    };
    let s1 = match ends[1] {
        End::Free => t.end + p,
        End::Closed => t.end.min(face.1 - w),
        End::Inside => t.end,
    };
    if !(s1 - s0 > p * 1e-3) {
        return Err(DocError::Invalid(format!(
            "the thread is too short to model between the shoulders: its groove needs {} mm clear of each",
            show_mm(w)
        )));
    }
    let base = t.axis_point + k * s0;
    let plane = Plane { origin: base, x: u, y: *k };
    let section = [Vec2::new(r0, -w0), Vec2::new(r1, -w1), Vec2::new(r1, w1), Vec2::new(r0, w0)];
    if !(r0.is_finite() && r1.is_finite()) {
        return Err(DocError::Invalid("thread dimensions are too large".into()));
    }
    Ok(GrooveShape { plane, section, base, axis: *k, pitch: p, turns: (s1 - s0) / p })
}

/// Evaluate a thread feature: a cosmetic thread is listed in the model's threads, a modelled
/// one cut into its body.
pub(super) fn eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::Thread { face, designation, length, modeled, from, up_to } = &f.kind else {
        return Err(DocError::Invalid("not a thread".into()));
    };
    let extent = match (length, up_to) {
        (Some(e), _) => Extent::Length(val(vals, e, Kind::Length)?),
        (None, Some(crate::ThreadLimit::Face(q))) => Extent::UpToPoint(q.0),
        (None, Some(crate::ThreadLimit::Plane(pl))) => Extent::UpTo(doc.resolve_plane_in(vals, pl, 0, Some(st))?),
        (None, None) => Extent::Full,
    };
    let ask = ThreadAsk { designation: designation.as_deref(), extent, from: from.map(|q| q.0), modeled: *modeled };
    let ThreadOn { info: t, cyl, ends } = thread_on(st, f.id, face.0, &ask)?;
    if !*modeled {
        st.threads.push(t);
        return Ok(());
    }
    let i = st.bodies.iter().position(|b| b.name == t.body).ok_or_else(|| DocError::Invalid("threaded body".into()))?;
    let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
    // The exact groove: SMLib's screw sweep, cut by a B-rep boolean.
    let g = thread_groove_shape(&t, cyl.radius, (cyl.start, cyl.end), ends)?;
    let section = Region2 { outer: solidcraft_geom::Loop2::polygon(&g.section), holes: Vec::new() };
    let groove = kernel::sweep_helix(&g.plane, &section, g.base, g.axis, g.pitch, g.turns)?;
    let (nb, groove_faces) = (kernel::boolean(&input.body, &groove, BoolOp::Cut)?, groove.face_count());
    let nb = nb.ok_or_else(|| DocError::Invalid("the thread removed everything".into()))?;
    // The groove's faces have no role to name them by.
    let groove = crate::lineage::Lineage::unnamed_faces(groove_faces);
    let lineage = input.lineage.and_then(|l| crate::lineage::Lineage::step(&nb, &[&l, &groove]));
    let nb = Traced { body: nb, lineage };
    if let Some(slot) = st.bodies.get_mut(i) {
        *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
    }
    Ok(())
}
