use parasolid_core::brep::*;

type R<T> = Result<T, String>;
type V = [f64; 3];
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
fn mul(a: V, k: f64) -> V {
    a.map(|v| v * k)
}
fn norm(a: V) -> f64 {
    a.iter().map(|v| v * v).sum::<f64>().sqrt()
}
fn knots(k: &[f64], m: &[u16]) -> R<Vec<f64>> {
    if k.len() != m.len() || k.iter().any(|v| !v.is_finite()) || k.windows(2).any(|p| p.first() > p.get(1)) {
        return Err("invalid knot vector".into());
    }
    let count = m.iter().try_fold(0usize, |n, v| n.checked_add(usize::from(*v)).filter(|n| *n <= 1_000_000)).ok_or("knot budget")?;
    let mut out = Vec::with_capacity(count);
    for (k, m) in k.iter().zip(m) {
        out.extend(std::iter::repeat_n(*k, usize::from(*m)));
    }
    Ok(out)
}
fn spline(k: &[f64], n: usize, p: &[Vec<f64>], t: f64) -> R<Vec<f64>> {
    if n == 0 || n > 32 || p.is_empty() || p.len() <= n || p.len() > 500_000 || p.len().checked_add(n + 1) != Some(k.len()) || !t.is_finite() {
        return Err("spline dimensions".into());
    }
    let at = |i| k.get(i).copied().ok_or_else(|| "invalid knot index".to_owned());
    let lo = at(n)?;
    let hi = at(p.len())?;
    if !(lo.is_finite() && hi.is_finite() && hi > lo) || t < lo - 0.01 * (hi - lo) || t > hi + 0.01 * (hi - lo) {
        return Err("spline parameter outside admitted interval".into());
    }
    let span = if t >= hi {
        p.len() - 1
    } else if t <= lo {
        n
    } else {
        (n..p.len()).find(|i| at(*i).ok().is_some_and(|a| a <= t) && at(*i + 1).ok().is_some_and(|b| t < b)).ok_or("knot span")?
    };
    let mut d = p.get(span - n..=span).ok_or("spline controls")?.to_vec();
    for r in 1..=n {
        for j in (r..=n).rev() {
            let i = span - n + j;
            let den = at(i + n - r + 1)? - at(i)?;
            let alpha = if den.abs() < 1e-30 { 0. } else { (t - at(i)?) / den };
            let left = d.get(j - 1).ok_or("spline control")?.clone();
            let right = d.get(j).ok_or("spline control")?.clone();
            if left.len() != right.len() || !(2..=4).contains(&left.len()) {
                return Err("spline control dimension".into());
            }
            let value = left.iter().zip(&right).map(|(a, b)| (1. - alpha) * a + alpha * b).collect::<Vec<_>>();
            *d.get_mut(j).ok_or("spline control")? = value;
        }
    }
    d.get(n).cloned().ok_or_else(|| "spline evaluation".into())
}
fn vec3(v: &[f64], rational: bool) -> R<V> {
    let w = if rational { *v.last().ok_or("homogeneous weight")? } else { 1. };
    if !w.is_finite() || w <= 0. {
        return Err("weight".into());
    }
    let [x, y, ..] = v else { return Err("curve point dimension".into()) };
    let z = if v.len() >= 3 + usize::from(rational) { v.get(2).copied().ok_or("point z")? / w } else { 0. };
    let p = [x / w, y / w, z];
    if !p.iter().all(|v| v.is_finite()) {
        return Err("nonfinite curve point".into());
    }
    Ok(p)
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
// Local tensor-product basis evaluation avoids cloning every control row for
// every projection sample. Only (degree+1)^2 homogeneous controls contribute.
fn basis(k: &[f64], degree: usize, count: usize, t: f64) -> R<(usize, [f64; 33])> {
    if degree == 0 || degree > 32 || count <= degree || count.checked_add(degree + 1) != Some(k.len()) || !t.is_finite() {
        return Err("surface basis dimensions".into());
    }
    let lo = *k.get(degree).ok_or("basis domain")?;
    let hi = *k.get(count).ok_or("basis domain")?;
    if !hi.is_finite() || !lo.is_finite() || hi <= lo || t < lo - (hi - lo) * 0.01 || t > hi + (hi - lo) * 0.01 {
        return Err("surface basis parameter".into());
    }
    let span = if t <= lo {
        degree
    } else if t >= hi {
        count - 1
    } else {
        k.partition_point(|v| *v <= t).checked_sub(1).filter(|v| *v >= degree && *v < count).ok_or("surface basis span")?
    };
    let mut weights = [0.; 33];
    let mut left = [0.; 33];
    let mut right = [0.; 33];
    weights[0] = 1.;
    let at = |v: &[f64; 33], i| v.get(i).copied().ok_or("surface basis index");
    for j in 1..=degree {
        *left.get_mut(j).ok_or("basis index")? = t - *k.get(span + 1 - j).ok_or("basis knot")?;
        *right.get_mut(j).ok_or("basis index")? = *k.get(span + j).ok_or("basis knot")? - t;
        let mut saved = 0.;
        for r in 0..j {
            let den = at(&right, r + 1)? + at(&left, j - r)?;
            let value = if den.abs() < 1e-30 { 0. } else { at(&weights, r)? / den };
            *weights.get_mut(r).ok_or("basis index")? = saved + at(&right, r + 1)? * value;
            saved = at(&left, j - r)? * value;
        }
        *weights.get_mut(j).ok_or("basis index")? = saved;
    }
    Ok((span - degree, weights))
}
fn nurbs_surface(n: &NurbsSurface, uv: V) -> R<V> {
    let nu = n.u_control_vertex_count as usize;
    let nv = n.v_control_vertex_count as usize;
    if nu.checked_mul(nv) != Some(n.control_vertices.len()) || n.control_vertices.len() > 500_000 {
        return Err("surface grid dimensions".into());
    }
    let u = knots(&n.u_knots, &n.u_knot_multiplicities)?;
    let v = knots(&n.v_knots, &n.v_knot_multiplicities)?;
    let (a, us) = basis(&u, usize::from(n.u_degree), nu, uv[0])?;
    let (b, vs) = basis(&v, usize::from(n.v_degree), nv, uv[1])?;
    let mut value = [0.; 4];
    for (i, wu) in us.iter().enumerate().take(usize::from(n.u_degree) + 1) {
        for (j, wv) in vs.iter().enumerate().take(usize::from(n.v_degree) + 1) {
            let at = (a + i).checked_mul(nv).and_then(|v| v.checked_add(b + j)).ok_or("surface control index")?;
            let pole = n.control_vertices.get(at).ok_or("surface control")?;
            let [x, y, z, ..] = pole.as_slice() else { return Err("surface control dimension".into()) };
            let w = wu * wv;
            value[0] += w * x;
            value[1] += w * y;
            value[2] += w * z;
            if n.rational {
                value[3] += w * *pole.get(3).ok_or("surface control weight")?;
            }
        }
    }
    if !n.rational {
        value[3] = 1.;
    }
    if !value.iter().all(|v| v.is_finite()) || value[3] <= 0. {
        return Err("invalid surface projection".into());
    }
    Ok([value[0] / value[3], value[1] / value[3], value[2] / value[3]])
}
pub(super) fn surface(m: &BrepModel, id: u32, uv: V) -> R<V> {
    let s = m.surfaces.iter().find(|s| s.id == id).ok_or("pcurve surface")?;
    Ok(match &s.kind {
        SurfaceKind::Nurbs(n) => nurbs_surface(n, uv)?,

        SurfaceKind::Cylinder { point, axis, radius, x_axis } => {
            let radial = add(mul(x_axis.to_array(), uv[0].cos()), mul(cross(axis.to_array(), x_axis.to_array()), uv[0].sin()));
            add(point.to_array(), add(mul(radial, *radius), mul(axis.to_array(), uv[1])))
        }
        SurfaceKind::Sphere { center, radius, axis, x_axis } => {
            let radial = add(mul(x_axis.to_array(), uv[0].cos()), mul(cross(axis.to_array(), x_axis.to_array()), uv[0].sin()));
            add(center.to_array(), add(mul(radial, *radius * uv[1].cos()), mul(axis.to_array(), *radius * uv[1].sin())))
        }
        SurfaceKind::Torus { center, axis, major_radius, minor_radius, x_axis } => {
            let radial = add(mul(x_axis.to_array(), uv[0].cos()), mul(cross(axis.to_array(), x_axis.to_array()), uv[0].sin()));
            add(center.to_array(), add(mul(radial, *major_radius + *minor_radius * uv[1].cos()), mul(axis.to_array(), *minor_radius * uv[1].sin())))
        }
        SurfaceKind::Plane { point, normal, x_axis } => {
            add(point.to_array(), add(mul(x_axis.to_array(), uv[0]), mul(cross(normal.to_array(), x_axis.to_array()), uv[1])))
        }
        _ => return Err(format!("pcurve surface {}", s.kind.as_str())),
    })
}
pub(super) fn curve(m: &BrepModel, id: u32, t: f64, depth: usize) -> R<V> {
    if depth > 16 {
        return Err("pcurve nesting".into());
    }
    let c = m.curves.iter().find(|c| c.id == id).ok_or("pcurve")?;
    match &c.kind {
        CurveKind::Nurbs(n) => vec3(&spline(&knots(&n.knots, &n.knot_multiplicities)?, n.degree as usize, &n.control_vertices, t)?, n.rational),
        CurveKind::Intersection { surfaces, chart_points, .. } => super::intersection::evaluate(m, *surfaces, chart_points, t),
        CurveKind::Line { point, direction } => Ok(add(point.to_array(), mul(direction.to_array(), t))),
        CurveKind::Trimmed { basis_curve, .. } => curve(m, *basis_curve, t, depth + 1),
        CurveKind::SurfaceParametric { surface: si, parameter_curve, .. } => surface(m, *si, curve(m, *parameter_curve, t, depth + 1)?),
        _ => Err(format!("pcurve basis {}", c.kind.as_str())),
    }
}
fn point(m: &BrepModel, v: u32) -> R<V> {
    let v = m.vertices.iter().find(|v0| v0.id == v).ok_or("tolerant vertex")?;
    Ok(m.points.iter().find(|p| p.id == v.point).ok_or("tolerant point")?.position.to_array())
}
struct Piece {
    a: f64,
    b: f64,
    p: [V; 4],
}
fn fit(m: &BrepModel, id: u32, lo: f64, hi: f64, tol: f64) -> R<Vec<Piece>> {
    let eval = |t| curve(m, id, t, 0);
    let mut todo = vec![(lo, hi, 0usize)];
    let mut out = Vec::new();
    while let Some((a, b, depth)) = todo.pop() {
        if out.len() + todo.len() > 4096 || depth > 20 {
            return Err(format!("pcurve fitting budget at {a}..{b}, depth {depth}"));
        }
        let p0 = eval(a)?;
        let p3 = eval(b)?;
        let q1 = eval(a + (b - a) / 3.)?;
        let q2 = eval(a + 2. * (b - a) / 3.)?;
        let aa = sub(sub(mul(q1, 27.), mul(p0, 8.)), p3);
        let bb = sub(sub(mul(q2, 27.), p0), mul(p3, 8.));
        let p1 = mul(sub(mul(aa, 2.), bb), 1. / 18.);
        let p2 = mul(sub(mul(bb, 2.), aa), 1. / 18.);
        let mut error = 0_f64;
        for i in 1..16 {
            let t = i as f64 / 16.;
            let q = add(add(mul(p0, (1. - t).powi(3)), mul(p1, 3. * t * (1. - t).powi(2))), add(mul(p2, 3. * t * t * (1. - t)), mul(p3, t.powi(3))));
            error = error.max(norm(sub(q, eval(a + (b - a) * t)?)));
        }
        if error > tol {
            let mid = (a + b) / 2.;
            todo.push((mid, b, depth + 1));
            todo.push((a, mid, depth + 1));
        } else {
            out.push(Piece { a, b, p: [p0, p1, p2, p3] });
        }
    }
    Ok(out)
}
pub fn recover(m: &mut BrepModel) -> R<()> {
    let mut pending = Vec::new();
    for e in m.edges.iter().filter(|e| e.curve.is_none()) {
        let tol = e.tolerance.ok_or("missing tolerant edge tolerance")?;
        // Sheets can retain the source's loose joins without asserting a solid.
        // This does not relax the fitting error or the closure gate for solids.
        let sheet = m.bodies.iter().any(|b| b.source.node_index == e.owner.node_index && b.kind == BodyKind::Sheet);
        let max_tolerance = if sheet { 1e-3 } else { 1e-5 };
        if !tol.is_finite() || tol <= 0. || tol > max_tolerance {
            return Err("edge tolerance exceeds import budget".into());
        }
        let mut errors = Vec::new();
        let mut chosen = None;
        for hi in &e.half_edges {
            let h = m.half_edges.iter().find(|h| h.id == *hi).ok_or("tolerant fin")?;
            let Some(ci) = h.curve else { continue };
            let c = m.curves.iter().find(|c| c.id == ci).ok_or("fin curve")?;
            let CurveKind::Trimmed { start_parameter, end_parameter, .. } = c.kind else { continue };
            match fit(m, ci, start_parameter, end_parameter, (tol * 0.01).min(1e-8)) {
                Ok(fit) => {
                    chosen = Some((c, h.sense, fit));
                    break;
                }
                Err(err) => errors.push(err.to_string()),
            }
        }
        let (source, fin_sense, pieces) = chosen.ok_or_else(|| format!("no supported pcurve carrier: {errors:?}"))?;
        let start = pieces.first().ok_or("empty fitted curve")?.p[0];
        let end = pieces.last().ok_or("empty fitted curve")?.p[3];
        let ring = e.start_vertex.is_none() && e.end_vertex.is_none();
        let sense = if ring {
            let CurveKind::Trimmed { start_point, end_point, .. } = &source.kind else { return Err("ring trim".into()) };
            let boundary_tol = (tol * 0.01).min(1e-8);
            if fin_sense == Sense::Unknown
                || norm(sub(start, end)) > boundary_tol
                || norm(sub(start_point.to_array(), end_point.to_array())) > boundary_tol
                || norm(sub(start, start_point.to_array())) > tol
                || norm(sub(end, end_point.to_array())) > tol
            {
                return Err("tolerant ring is not closed within source tolerance".into());
            }
            fin_sense
        } else {
            let (a, b) = (point(m, e.start_vertex.ok_or("missing start vertex")?)?, point(m, e.end_vertex.ok_or("missing end vertex")?)?);
            let forward = norm(sub(start, a)).max(norm(sub(end, b)));
            let reversed = norm(sub(start, b)).max(norm(sub(end, a)));
            let sense = if forward < reversed { Sense::Positive } else { Sense::Negative };
            let vertex_tol = |id: u32| m.vertices.iter().find(|v| v.id == id).and_then(|v| v.tolerance).unwrap_or(5e-9);
            let ta = vertex_tol(e.start_vertex.ok_or("start")?);
            let tb = vertex_tol(e.end_vertex.ok_or("end")?);
            let fits = if sense == Sense::Positive {
                norm(sub(start, a)) <= ta && norm(sub(end, b)) <= tb
            } else {
                norm(sub(end, a)) <= ta && norm(sub(start, b)) <= tb
            };
            if !fits {
                return Err(format!(
                    "tolerant pcurve endpoints exceed source tolerance: edge={} tol={tol} forward={forward} reversed={reversed}",
                    e.id
                ));
            }
            sense
        };
        let first = pieces.first().ok_or("empty fitted curve")?;
        let mut poles = vec![first.p[0].to_vec()];
        let mut ks = vec![first.a];
        let mut mult = vec![4];
        for (i, p) in pieces.iter().enumerate() {
            poles.extend(p.p[1..].iter().map(|p| p.to_vec()));
            ks.push(p.b);
            mult.push(if i + 1 == pieces.len() { 4 } else { 3 });
        }
        // A source trim may run backwards. Keep its control points in their
        // evaluated travel order, but give the fitted spline an ascending parameter.
        if first.a > pieces.last().ok_or("empty fitted curve")?.b {
            ks.iter_mut().for_each(|k| *k = -*k);
        }
        let c = CurveGeometry {
            id: 0,
            sense,
            owner: None,
            kind: CurveKind::Nurbs(NurbsCurve {
                degree: 3,
                control_vertex_count: u32::try_from(poles.len()).map_err(|e| e.to_string())?,
                vertex_dimension: 3,
                knot_type: 0,
                periodic: false,
                closed: ring,
                rational: false,
                curve_form: 0,
                control_vertices: poles,
                knots: ks,
                knot_multiplicities: mult,
                sources: vec![source.source.clone()],
            }),
            source: source.source.clone(),
        };
        pending.push((e.id, c));
    }
    let mut next = m.curves.iter().map(|c| c.id).max().unwrap_or(0).checked_add(1).ok_or("curve id overflow")?;
    for (ei, mut c) in pending {
        c.id = next;
        next = next.checked_add(1).ok_or("curve id overflow")?;
        m.edges.iter_mut().find(|e| e.id == ei).ok_or("edge")?.curve = Some(c.id);
        m.curves.push(c);
    }
    Ok(())
}

// Edge vertices trim the implicit basis. Its chart may extend well beyond the
// bounded NURBS support, and those unused portions must not be projected or fitted.
fn used_intersection_range(m: &BrepModel, id: u32, chart: &[Vector3]) -> R<(f64, f64)> {
    let end = (chart.len() - 1) as f64;
    let parameter = |q: V| -> R<f64> {
        let mut best = (f64::INFINITY, 0.);
        for (i, pair) in chart.windows(2).enumerate() {
            let a = pair.first().ok_or("chart point")?.to_array();
            let b = pair.get(1).ok_or("chart point")?.to_array();
            let d = sub(b, a);
            let den = d.iter().map(|v| v * v).sum::<f64>();
            if den <= 1e-30 {
                continue;
            }
            let delta = sub(q, a);
            let t = (delta.iter().zip(d).map(|(a, b)| a * b).sum::<f64>() / den).clamp(0., 1.);
            let distance = norm(sub(q, add(a, mul(d, t))));
            if distance < best.0 {
                best = (distance, i as f64 + t);
            }
        }
        if !best.0.is_finite() {
            return Err("degenerate intersection chart".into());
        }
        Ok(best.1)
    };
    let (mut lo, mut hi) = (end, 0_f64);
    for edge in m.edges.iter().filter(|e| e.curve == Some(id)) {
        let (Some(a), Some(b)) = (edge.start_vertex, edge.end_vertex) else { return Ok((0., end)) };
        if a == b {
            return Ok((0., end));
        }
        for v in [a, b] {
            let t = parameter(point(m, v)?)?;
            lo = lo.min(t);
            hi = hi.max(t);
        }
    }
    if hi - lo < 1e-12 {
        return Err("intersection has no bounded edge interval".into());
    }
    Ok(((lo - 0.001).max(0.), (hi + 0.001).min(end)))
}

// Retain the source supports as surfaces and restate their intersection to a small
// fraction of the source modelling resolution. No chart chord is used as geometry.
pub(super) fn recover_intersections(m: &mut BrepModel) -> R<()> {
    let resolution = m.bodies.iter().map(|b| b.linear_resolution).filter(|v| v.is_finite() && *v > 0.).fold(1e-8_f64, f64::min);
    let tolerance = (resolution * 0.05).clamp(1e-12, 5e-10);
    let mut pending = Vec::new();
    let mut total = 0usize;
    for c in &m.curves {
        let CurveKind::Intersection { chart_points, .. } = &c.kind else { continue };
        if !m.edges.iter().any(|e| e.curve == Some(c.id)) {
            continue;
        }
        if chart_points.len() < 2 || chart_points.len() > 4096 {
            return Err("intersection chart budget".into());
        }
        let (lo, hi) = used_intersection_range(m, c.id, chart_points)?;
        let mut pieces = Vec::new();
        for i in lo.floor() as usize..(hi.ceil() as usize).min(chart_points.len() - 1) {
            let a = lo.max(i as f64);
            let b = hi.min((i + 1) as f64);
            if b <= a {
                continue;
            }
            pieces.extend(fit(m, c.id, a, b, tolerance).map_err(|e| format!("intersection {} segment {i}: {e}", c.id))?);
            if pieces.len() > 4096 {
                return Err("intersection fit budget".into());
            }
        }
        let first = pieces.first().ok_or("empty intersection")?;
        let mut poles = vec![first.p[0].to_vec()];
        let mut knots = vec![first.a];
        let mut mult = vec![4];
        for (i, p) in pieces.iter().enumerate() {
            poles.extend(p.p[1..].iter().map(|p| p.to_vec()));
            knots.push(p.b);
            mult.push(if i + 1 == pieces.len() { 4 } else { 3 });
        }
        total = total.checked_add(poles.len()).filter(|v| *v <= 500_000).ok_or("intersection control budget")?;
        let last = pieces.last().ok_or("empty intersection")?;
        let closed = norm(sub(first.p[0], last.p[3])) <= resolution;
        pending.push((
            c.id,
            CurveKind::Nurbs(NurbsCurve {
                degree: 3,
                control_vertex_count: u32::try_from(poles.len()).map_err(|e| e.to_string())?,
                vertex_dimension: 3,
                knot_type: 0,
                periodic: false,
                closed,
                rational: false,
                curve_form: 0,
                control_vertices: poles,
                knots,
                knot_multiplicities: mult,
                sources: vec![c.source.clone()],
            }),
        ));
    }
    for (id, kind) in pending {
        m.curves.iter_mut().find(|c| c.id == id).ok_or("source intersection")?.kind = kind;
    }
    Ok(())
}

pub(super) fn surface_domain(n: &NurbsSurface) -> R<[[f64; 2]; 2]> {
    let u = knots(&n.u_knots, &n.u_knot_multiplicities)?;
    let v = knots(&n.v_knots, &n.v_knot_multiplicities)?;
    Ok([
        [*u.get(usize::from(n.u_degree)).ok_or("surface U domain")?, *u.get(n.u_control_vertex_count as usize).ok_or("surface U domain")?],
        [*v.get(usize::from(n.v_degree)).ok_or("surface V domain")?, *v.get(n.v_control_vertex_count as usize).ok_or("surface V domain")?],
    ])
}
