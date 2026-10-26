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
    thread_on(st, 0, p, None, None, false).map(|t| t.designation)
}

/// A cosmetic thread on the cylindrical face at `p`. A `turned` (modelled) thread also fits a
/// shaft thicker than it, which it turns down to its major diameter first.
pub(super) fn thread_on(st: &ModelState, feature: u64, p: Vec3, designation: Option<&str>, length: Option<f64>, turned: bool) -> Result<ThreadInfo> {
    thread_on_impl(st, feature, p, designation, length, false, turned)
}

// Tapped holes admit verified Unified and parallel pipe sizes; the separate Thread feature remains metric,
// including when an untrusted design requests modelled geometry.
pub(super) fn thread_on_impl(
    st: &ModelState,
    feature: u64,
    p: Vec3,
    designation: Option<&str>,
    length: Option<f64>,
    tapped: bool,
    turned: bool,
) -> Result<ThreadInfo> {
    let (body, cyl) = st
        .bodies
        .iter()
        .filter_map(|b| kernel::cylinder_face_at(&b.body, p).map(|c| (b.name.clone(), c)))
        .min_by(|a, b| {
            let dist = |c: &kernel::CylinderFace| ((p - c.axis_point - c.axis * (p - c.axis_point).dot(c.axis)).len() - c.radius).abs();
            dist(&a.1).total_cmp(&dist(&b.1))
        })
        .ok_or_else(|| DocError::Invalid("no cylindrical face there to thread".into()))?;
    let dia = cyl.radius * 2.0;
    let tapped_size = if tapped {
        let name = designation.ok_or_else(|| DocError::Invalid("a tapped hole needs a designation".into()))?;
        Some(
            crate::parse_tapped_thread(name)
                .ok_or_else(|| DocError::Invalid(format!("unknown tapped thread `{name}` (ISO metric or a catalogued UNC/UNF, G or Rp size)")))?,
        )
    } else {
        None
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
    // From the end of the face nearest the point.
    let s = (p - cyl.axis_point).dot(cyl.axis);
    let (mut a, mut b) = (cyl.start, cyl.end);
    if let Some(l) = length {
        if !(l > 0.0) {
            return Err(DocError::Invalid("thread length must be positive".into()));
        }
        if (s - a).abs() <= (b - s).abs() {
            b = (a + l).min(b);
        } else {
            a = (b - l).max(a);
        }
    }
    Ok(ThreadInfo {
        feature,
        body,
        designation: name,
        major_diameter: major,
        radius: cyl.radius,
        pitch,
        internal: cyl.internal,
        axis_point: cyl.axis_point,
        axis: cyl.axis,
        start: a,
        end: b,
    })
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
/// `start` to `end` along the axis. `radius` is the face's own radius; `open` says which ends
/// of the threaded length are free ends of the face.
fn thread_groove_shape(t: &ThreadInfo, radius: f64, open: (bool, bool)) -> Result<GrooveShape> {
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
    // Past a free end of the face the groove runs out into the air; elsewhere it stops square
    // at the threaded length's end.
    let (s0, s1) = (if open.0 { t.start - p } else { t.start }, if open.1 { t.end + p } else { t.end });
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
/// What a modelled external thread turns off a shaft thicker than its major diameter before it
/// cuts, as a lathe would: the ring between the major diameter and the shaft over the threaded
/// length, run on past an end of the face that nothing of the part (`mb`) lies beyond. `None`
/// for a shaft no thicker than the thread. Its inner wall is the turned face, named `turned`;
/// an end that stops inside the face leaves a shoulder, `shoulder:start` or `shoulder:end`.
fn thread_turn(t: &ThreadInfo, cyl: &kernel::CylinderFace, mb: &ModelBody) -> Result<Option<Traced>> {
    let (p, r) = (t.pitch, t.major_diameter / 2.0);
    if t.internal || !(cyl.radius > r * (1.0 + 1e-6)) {
        return Ok(None);
    }
    let k = t.axis;
    let u = k.cross(if k.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) });
    let u = u.normalized().ok_or_else(|| DocError::Invalid("thread axis".into()))?;
    let v = k.cross(u);
    let mesh = mb.mesh();
    let solid_round = |s: f64, radius: f64| {
        (0..12).any(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 12.0;
            mesh.contains(t.axis_point + *k * s + (u * a.cos() + v * a.sin()) * radius)
        })
    };
    // The ring reaches a little past the shaft; the part must not go on there (a shaft standing
    // in a corner, a rounded edge), or the turning would cut into it.
    let out = cyl.radius + 0.1 * p;
    if [0.25, 0.5, 0.75].iter().any(|f| solid_round(t.start + (t.end - t.start) * f, cyl.radius + 0.05 * p)) {
        return Err(DocError::Invalid(format!(
            "this {} mm shaft is too thick for a modelled {} thread, and the part around it keeps it from being turned down; make the shaft {} mm",
            show_mm(2.0 * cyl.radius),
            t.designation,
            show_mm(t.major_diameter)
        )));
    }
    // Past a free end of the face out into the air; elsewhere square at the threaded length's end.
    let tol = p * 1e-3;
    let mid = (r + cyl.radius) / 2.0;
    let free = |at: f64, s: f64, past: f64| (at - s).abs() < tol && !solid_round(s + past * 0.5 * p, mid);
    let (open0, open1) = (free(cyl.start, t.start, -1.0), free(cyl.end, t.end, 1.0));
    let s0 = if open0 { t.start - p } else { t.start };
    let s1 = if open1 { t.end + p } else { t.end };
    let plane = Plane { origin: t.axis_point, x: u, y: *k };
    let ring = [Vec2::new(r, s0), Vec2::new(out, s0), Vec2::new(out, s1), Vec2::new(r, s1)];
    let region = Region2 { outer: solidcraft_geom::Loop2::polygon(&ring), holes: vec![] };
    let ring = kernel::revolve(&plane, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?
        .into_iter()
        .next()
        .ok_or_else(|| DocError::Invalid("thread: the shaft turned down to the thread".into()))?;
    // The ring's sides by profile segment: its start end, outside, end end, inner wall.
    let role = |segment: usize| -> Vec<String> {
        match segment {
            0 if !open0 => vec!["shoulder:start".into()],
            2 if !open1 => vec!["shoulder:end".into()],
            3 => vec!["turned".into()],
            _ => Vec::new(),
        }
    };
    let roles = ring.history().filter(|h| h.operands == 0).map(|h| {
        h.faces
            .iter()
            .map(|f| match f.origins.as_slice() {
                [kernel::history::FaceOrigin::Profile { segment }] => role(*segment),
                _ => Vec::new(),
            })
            .collect()
    });
    Ok(Some(Traced::made(ring, roles)))
}

pub(super) fn eval(vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::Thread { face, designation, length, modeled } = &f.kind else {
        return Err(DocError::Invalid("not a thread".into()));
    };
    let l = match length {
        Some(e) => Some(val(vals, e, Kind::Length)?),
        None => None,
    };
    let t = thread_on(st, f.id, face.0, designation.as_deref(), l, *modeled)?;
    if !*modeled {
        st.threads.push(t);
        return Ok(());
    }
    let i = st.bodies.iter().position(|b| b.name == t.body).ok_or_else(|| DocError::Invalid("threaded body".into()))?;
    let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
    let cyl = kernel::cylinder_face_at(&mb.body, face.0).ok_or_else(|| DocError::Invalid("no cylindrical face there to thread".into()))?;
    let tol = t.pitch * 1e-3;
    // A shaft thicker than the thread is turned down to its major diameter first.
    let input = mb.traced();
    let (input, radius) = match thread_turn(&t, &cyl, &mb)? {
        Some(ring) => {
            let turned = kernel::boolean(&input.body, &ring.body, BoolOp::Cut)?
                .ok_or_else(|| DocError::Invalid("turning the shaft down removed everything".into()))?;
            (Traced::step(turned, &[&input, &ring]), t.major_diameter / 2.0)
        }
        None => (input, cyl.radius),
    };
    let open = ((t.start - cyl.start).abs() < tol, (t.end - cyl.end).abs() < tol);
    // The exact groove: SMLib's screw sweep, cut by a B-rep boolean.
    let g = thread_groove_shape(&t, radius, open)?;
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
