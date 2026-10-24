use parasolid_core::brep::*;
use std::collections::BTreeMap;
type R<T> = Result<T, String>;
#[derive(Default)]
struct Writer {
    lines: Vec<String>,
    base: usize,
}
impl Writer {
    fn add(&mut self, s: String) -> usize {
        self.lines.push(s);
        self.base + self.lines.len()
    }
    fn p(&mut self, v: Vector3) -> usize {
        self.add(format!("CARTESIAN_POINT('',({},{},{}))", v.x * 1000., v.y * 1000., v.z * 1000.))
    }
    fn d(&mut self, v: Vector3) -> usize {
        self.add(format!("DIRECTION('',({},{},{}))", v.x, v.y, v.z))
    }
    fn axis(&mut self, p: Vector3, z: Vector3, x: Vector3) -> usize {
        let p = self.p(p);
        let z = self.d(z);
        let x = self.d(x);
        self.add(format!("AXIS2_PLACEMENT_3D('',#{p},#{z},#{x})"))
    }
}
fn refs(ids: impl IntoIterator<Item = usize>) -> String {
    ids.into_iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",")
}
fn flag(v: bool) -> &'static str {
    if v { ".T." } else { ".F." }
}
fn neg(v: Vector3) -> Vector3 {
    Vector3 { x: -v.x, y: -v.y, z: -v.z }
}
fn shift(p: Vector3, x: Vector3, r: f64) -> Vector3 {
    Vector3 { x: p.x + x.x * r, y: p.y + x.y * r, z: p.z + x.z * r }
}
pub(super) fn write_colored(original: &BrepModel, colors: &BTreeMap<u32, [f64; 3]>, base: usize) -> R<(String, usize)> {
    let mut prepared = original.clone();
    super::pcurves::recover(&mut prepared)?;
    for _ in 0..16 {
        let snapshot = prepared.curves.clone();
        let mut changed = false;
        for c in &mut prepared.curves {
            if let CurveKind::Trimmed { basis_curve, .. } = c.kind {
                let basis = snapshot.iter().find(|v| v.id == basis_curve).ok_or("trim basis")?;
                c.kind = basis.kind.clone();
                if basis.sense == Sense::Negative {
                    c.sense = if c.sense == Sense::Positive { Sense::Negative } else { Sense::Positive };
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    recover_intersections(&mut prepared)?;
    super::pcurves::recover_intersections(&mut prepared)?;
    let m = &prepared;
    if !m.complete {
        return Err(format!("incomplete {:?}", m.diagnostics));
    }
    let mut w = Writer { base, ..Default::default() };
    let mut surfaces = BTreeMap::new();
    let mut curves = BTreeMap::new();
    let mut vertices = BTreeMap::new();
    let mut edges = BTreeMap::new();
    for s in &m.surfaces {
        let text = match s.kind {
            SurfaceKind::Nurbs(ref n) => {
                let mut grid = Vec::new();
                let mut weights = Vec::new();
                if n.v_control_vertex_count == 0 || n.control_vertices.len() > 500_000 {
                    return Err("surface grid budget".into());
                }
                for row in n.control_vertices.chunks(n.v_control_vertex_count as usize) {
                    let mut ids = Vec::new();
                    let mut ws = Vec::new();
                    for v in row {
                        let weight = if n.rational { *v.get(3).ok_or("surface weight")? } else { 1. };
                        let [x, y, z, ..] = v.as_slice() else { return Err("surface pole dimension".into()) };
                        ids.push(w.p(Vector3 { x: x / weight, y: y / weight, z: z / weight }));
                        ws.push(weight.to_string());
                    }
                    grid.push(format!("({})", refs(ids)));
                    weights.push(format!("({})", ws.join(",")));
                }
                let fmt = |v: &[f64]| v.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                let mfmt = |v: &[u16]| v.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                let poles = grid.join(",");
                let um = mfmt(&n.u_knot_multiplicities);
                let vm = mfmt(&n.v_knot_multiplicities);
                let uk = fmt(&n.u_knots);
                let vk = fmt(&n.v_knots);
                if n.rational {
                    format!(
                        "(BOUNDED_SURFACE() B_SPLINE_SURFACE({},{},({poles}),.UNSPECIFIED.,{},{},.F.) B_SPLINE_SURFACE_WITH_KNOTS(({um}),({vm}),({uk}),({vk}),.UNSPECIFIED.) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(({})) REPRESENTATION_ITEM('') SURFACE())",
                        n.u_degree,
                        n.v_degree,
                        flag(n.u_closed),
                        flag(n.v_closed),
                        weights.join(",")
                    )
                } else {
                    format!(
                        "B_SPLINE_SURFACE_WITH_KNOTS('',{},{},({poles}),.UNSPECIFIED.,{},{},.F.,({um}),({vm}),({uk}),({vk}),.UNSPECIFIED.)",
                        n.u_degree,
                        n.v_degree,
                        flag(n.u_closed),
                        flag(n.v_closed)
                    )
                }
            }
            SurfaceKind::Plane { point, normal, x_axis } => {
                let a = w.axis(point, normal, x_axis);
                format!("PLANE('',#{a})")
            }
            SurfaceKind::Cylinder { point, axis, radius, x_axis } => {
                let a = w.axis(point, axis, x_axis);
                format!("CYLINDRICAL_SURFACE('',#{a},{})", radius * 1000.)
            }
            SurfaceKind::Cone { point, axis, radius, sin_half_angle, cos_half_angle, x_axis } => {
                let a = w.axis(point, if sin_half_angle < 0. { neg(axis) } else { axis }, x_axis);
                format!("CONICAL_SURFACE('',#{a},{},{})", radius * 1000., sin_half_angle.atan2(cos_half_angle).abs())
            }
            SurfaceKind::Sphere { center, radius, axis, x_axis } => {
                let a = w.axis(center, axis, x_axis);
                format!("SPHERICAL_SURFACE('',#{a},{})", radius * 1000.)
            }
            SurfaceKind::Torus { center, axis, major_radius, minor_radius, x_axis } => {
                let a = w.axis(center, axis, x_axis);
                format!("TOROIDAL_SURFACE('',#{a},{},{})", major_radius * 1000., minor_radius * 1000.)
            }
            _ => return Err(format!("surface {} {:?}", s.id, s.kind)),
        };
        surfaces.insert(s.id, w.add(text));
    }
    for c in &m.curves {
        if !m.edges.iter().any(|e| e.curve == Some(c.id)) {
            continue;
        }

        let text = match c.kind {
            CurveKind::Intersection { .. } => return Err("source intersection was not resolved to source precision".into()),
            CurveKind::Line { point, direction } => {
                let p = w.p(point);
                let d = w.d(direction);
                let v = w.add(format!("VECTOR('',#{d},1000.)"));
                format!("LINE('',#{p},#{v})")
            }
            CurveKind::Circle { center, normal, x_axis, radius } => {
                let a = w.axis(center, normal, x_axis);
                format!("CIRCLE('',#{a},{})", radius * 1000.)
            }
            CurveKind::Nurbs(ref n) => {
                let points = n
                    .control_vertices
                    .iter()
                    .map(|v| {
                        let weight = if n.rational { *v.get(3).ok_or("weight")? } else { 1. };
                        let [x, y, z, ..] = v.as_slice() else { return Err("curve pole dimension".into()) };
                        Ok(w.p(Vector3 { x: x / weight, y: y / weight, z: z / weight }))
                    })
                    .collect::<R<Vec<_>>>()?;
                let knots = n.knots.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                let mult = n.knot_multiplicities.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                if n.rational {
                    let weights = n
                        .control_vertices
                        .iter()
                        .map(|v| v.get(3).map(ToString::to_string).ok_or_else(|| "curve weight".to_owned()))
                        .collect::<R<Vec<_>>>()?
                        .join(",");
                    format!(
                        "(BOUNDED_CURVE() B_SPLINE_CURVE({},({}),.UNSPECIFIED.,{},.F.) B_SPLINE_CURVE_WITH_KNOTS(({mult}),({knots}),.UNSPECIFIED.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(({weights})) REPRESENTATION_ITEM(''))",
                        n.degree,
                        refs(points),
                        flag(n.closed)
                    )
                } else {
                    format!(
                        "B_SPLINE_CURVE_WITH_KNOTS('',{},({}),.UNSPECIFIED.,{},.F.,({mult}),({knots}),.UNSPECIFIED.)",
                        n.degree,
                        refs(points),
                        flag(n.closed)
                    )
                }
            }
            CurveKind::Parabola { origin, normal, x_axis, focal_length } => {
                let a = w.axis(origin, normal, x_axis);
                format!("PARABOLA('',#{a},{})", focal_length * 1000.)
            }
            CurveKind::Hyperbola { origin, normal, x_axis, transverse_radius, conjugate_radius } => {
                let a = w.axis(origin, normal, x_axis);
                format!("HYPERBOLA('',#{a},{},{})", transverse_radius * 1000., conjugate_radius * 1000.)
            }
            CurveKind::Ellipse { center, normal, x_axis, major_radius, minor_radius } => {
                let a = w.axis(center, normal, x_axis);
                format!("ELLIPSE('',#{a},{},{})", major_radius * 1000., minor_radius * 1000.)
            }
            _ => return Err(format!("curve {} {:?}", c.id, c.kind)),
        };
        curves.insert(c.id, w.add(text));
    }
    for v in &m.vertices {
        let p = m.points.iter().find(|p| p.id == v.point).ok_or("point")?;
        let p = w.p(p.position);
        vertices.insert(v.id, w.add(format!("VERTEX_POINT('',#{p})")));
    }
    for e in &m.edges {
        let ci = e.curve.ok_or("tolerant edge")?;
        let c = m.curves.iter().find(|c| c.id == ci).ok_or("curve")?;
        if c.sense == Sense::Unknown {
            return Err("edge curve orientation is unknown".into());
        }
        let (start, end) = if let (Some(a), Some(b)) = (e.start_vertex, e.end_vertex) {
            (*vertices.get(&a).ok_or("start")?, *vertices.get(&b).ok_or("end")?)
        } else {
            let p = match c.kind {
                CurveKind::Circle { center, x_axis, radius, .. } => shift(center, x_axis, radius),
                CurveKind::Ellipse { center, x_axis, major_radius, .. } => shift(center, x_axis, major_radius),
                CurveKind::Intersection { ref chart_points, .. } => *chart_points.first().ok_or("ring intersection point")?,
                CurveKind::Nurbs(ref n) if n.closed => {
                    let p = n.control_vertices.first().ok_or("ring pole")?;
                    let [x, y, z, ..] = p.as_slice() else { return Err("ring pole dimensions".into()) };
                    let w = if n.rational { *p.get(3).ok_or("ring weight")? } else { 1. };
                    Vector3 { x: x / w, y: y / w, z: z / w }
                }
                _ => return Err("unsupported ring curve".into()),
            };
            let p = w.p(p);
            let v = w.add(format!("VERTEX_POINT('',#{p})"));
            (v, v)
        };
        let curve = curves.get(&ci).ok_or("edge curve")?;
        let s = flag(c.sense == Sense::Positive);
        edges.insert(e.id, w.add(format!("EDGE_CURVE('',#{start},#{end},#{curve},{s})")));
    }
    let mut loops = BTreeMap::new();
    for l in &m.loops {
        let mut ring = Vec::new();
        for hi in &l.half_edges {
            let h = m.half_edges.iter().find(|h| h.id == *hi).ok_or("fin")?;
            if let Some(e) = h.edge {
                let ei = edges.get(&e).ok_or("fin edge")?;
                let s = flag(h.sense == Sense::Positive);
                ring.push(w.add(format!("ORIENTED_EDGE('',*,*,#{ei},{s})")));
            } else {
                let v = vertices.get(&h.vertex.ok_or("pole vertex")?).ok_or("pole vertex")?;
                let v = w.add(format!("VERTEX_LOOP('',#{v})"));
                loops.insert(l.id, v);
                continue;
            }
        }
        if ring.is_empty() {
            continue;
        }
        let e = w.add(format!("EDGE_LOOP('',({}))", refs(ring)));
        loops.insert(l.id, e);
    }
    let mut faces = BTreeMap::new();
    for f in &m.faces {
        let si = f.surface.ok_or("surface")?;
        let s = m.surfaces.iter().find(|s| s.id == si).ok_or("surface")?;
        let mut bounds = Vec::new();
        for li in &f.loops {
            let l = loops.get(li).ok_or("loop")?;
            bounds.push(w.add(format!("FACE_BOUND('',#{l},.T.)")));
        }
        let surf = surfaces.get(&si).ok_or("face surface")?;
        if f.sense == Sense::Unknown || s.sense == Sense::Unknown {
            return Err("face orientation is unknown".into());
        }
        let orientation = f.sense == s.sense;
        faces.insert(f.id, w.add(format!("ADVANCED_FACE('',({}),#{surf},{})", refs(bounds), flag(orientation))));
    }
    let mut solids = Vec::new();
    let mut solid_owners = BTreeMap::new();
    let mut sheets = Vec::new();
    for body in &m.bodies {
        if body.kind == BodyKind::Sheet {
            // A sheet's two sides bound the same void. Emit each face once,
            // without creating a material region or claiming a closed volume.
            let mut shell_ids = Vec::new();
            let mut seen = std::collections::BTreeSet::new();
            for ri in &body.regions {
                let region = m.regions.iter().find(|r| r.id == *ri).ok_or("sheet region")?;
                if region.kind != RegionKind::Void {
                    return Err("sheet has a material region".into());
                }
                for si in &region.shells {
                    let shell = m.shells.iter().find(|s| s.id == *si).ok_or("sheet shell")?;
                    if !shell.wire_edges.is_empty() || shell.isolated_vertex.is_some() {
                        return Err("sheet includes unsupported wire geometry".into());
                    }
                    let ids = shell.back_faces.iter().chain(&shell.front_faces).copied().collect::<std::collections::BTreeSet<_>>();
                    if ids.is_empty() || ids.iter().any(|id| !seen.insert(*id)) {
                        return Err("empty or repeated sheet shell".into());
                    }
                    let fs = ids.iter().map(|id| faces.get(id).copied().ok_or("sheet face")).collect::<Result<Vec<_>, _>>()?;
                    shell_ids.push(w.add(format!("OPEN_SHELL('',({}))", refs(fs))));
                }
            }
            if shell_ids.is_empty() {
                return Err("sheet has no surface shells".into());
            }
            let item = w.add(format!("SHELL_BASED_SURFACE_MODEL('source-sheet-{}',({}))", body.id, refs(shell_ids)));
            sheets.push(item);
            solid_owners.insert(item, body.source.node_index);
            continue;
        }
        if body.kind != BodyKind::Solid {
            continue;
        }
        let exterior = *body.regions.first().ok_or("body exterior region")?;
        if m.regions.iter().find(|r| r.id == exterior).ok_or("exterior")?.kind != RegionKind::Void {
            return Err("body exterior must be void".into());
        }
        for ri in &body.regions {
            let region = m.regions.iter().find(|r| r.id == *ri).ok_or("region")?;
            if region.kind != RegionKind::Solid {
                continue;
            }
            let mut outer = None;
            let mut inner = Vec::new();
            for si in &region.shells {
                let shell = m.shells.iter().find(|sh| sh.id == *si).ok_or("shell")?;
                let fs = shell.back_faces.iter().chain(&shell.front_faces).copied().collect::<Vec<_>>();
                let adjacent = fs
                    .iter()
                    .map(|fi| {
                        let f = m.faces.iter().find(|f| f.id == *fi).ok_or("face")?;
                        let other = if f.back_shell == *si {
                            f.front_shell
                        } else if f.front_shell == *si {
                            f.back_shell
                        } else {
                            return Err("face shell membership");
                        };
                        Ok(m.shells.iter().find(|s| s.id == other).ok_or("opposite shell")?.region)
                    })
                    .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
                if adjacent.len() != 1 {
                    return Err("mixed opposite regions on material shell".into());
                }
                let face_ids = fs.iter().map(|i| faces.get(i).copied().ok_or("shell face")).collect::<Result<Vec<_>, _>>()?;
                let sh = w.add(format!("CLOSED_SHELL('',({}))", refs(face_ids)));
                if adjacent.contains(&exterior) {
                    if outer.replace(sh).is_some() {
                        return Err("material region has multiple outer shells".into());
                    }
                } else {
                    inner.push(w.add(format!("ORIENTED_CLOSED_SHELL('',*,#{sh},.F.)")));
                }
            }
            let sh = outer.ok_or("material region has no exterior boundary")?;
            let solid = if inner.is_empty() {
                w.add(format!("MANIFOLD_SOLID_BREP('source-{}',#{sh})", body.id))
            } else {
                w.add(format!("BREP_WITH_VOIDS('source-{}',#{sh},({}))", body.id, refs(inner)))
            };
            solids.push(solid);
            solid_owners.insert(solid, body.source.node_index);
        }
    }
    let mut targets = BTreeMap::new();
    for f in &m.faces {
        if let Some(c) = colors.get(&f.source.node_index) {
            targets.insert(*faces.get(&f.id).ok_or("colored face")?, *c);
        }
    }
    for (solid, owner) in &solid_owners {
        if let Some(c) = colors.get(owner) {
            targets.insert(*solid, *c);
        }
    }
    for (target, c) in targets {
        let rgb = w.add(format!("COLOUR_RGB('',{},{},{})", c[0], c[1], c[2]));
        let fill = w.add(format!("FILL_AREA_STYLE_COLOUR('',#{rgb})"));
        let a = w.add(format!("FILL_AREA_STYLE('',(#{fill}))"));
        let b = w.add(format!("SURFACE_STYLE_FILL_AREA(#{a})"));
        let side = w.add(format!("SURFACE_SIDE_STYLE('',(#{b}))"));
        let usage = w.add(format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
        let style = w.add(format!("PRESENTATION_STYLE_ASSIGNMENT((#{usage}))"));
        w.add(format!("STYLED_ITEM('',(#{style}),#{target})"));
    }
    let mm = w.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))".into());
    let rad = w.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))".into());
    let sr = w.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())".into());
    let accuracy = m
        .bodies
        .iter()
        .map(|b| b.linear_resolution)
        .chain(m.vertices.iter().filter_map(|v| v.tolerance))
        .chain(m.edges.iter().filter_map(|e| e.tolerance))
        .fold(1e-10_f64, f64::max)
        * 1000.;
    let un = w.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({accuracy}),#{mm},'','')"));
    let ctx=w.add(format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{un})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{rad},#{sr})) REPRESENTATION_CONTEXT('',''))"));
    if !solids.is_empty() {
        w.add(format!("ADVANCED_BREP_SHAPE_REPRESENTATION('',({}),#{ctx})", refs(solids)));
    }
    if !sheets.is_empty() {
        w.add(format!("MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',({}),#{ctx})", refs(sheets)));
    }
    let mut out = String::from(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('SolidCraft native source geometry'),'2;1');\nFILE_NAME('','',(''),(''),'SolidCraft','','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n",
    );
    for (i, l) in w.lines.iter().enumerate() {
        out.push_str(&format!("#{}={};\n", i + 1 + base, l));
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok((out, w.lines.len()))
}

fn support(s: &SurfaceKind) -> R<crate::native_cad::sldprt::analytic::Support> {
    use crate::native_cad::sldprt::analytic::Support as Q;
    let mm = |v: Vector3| v.to_array().map(|x| x * 1000.);
    Ok(match *s {
        SurfaceKind::Plane { point, normal, x_axis } => Q::Plane { p: mm(point), n: normal.to_array(), u: x_axis.to_array() },
        SurfaceKind::Cylinder { point, axis, radius, .. } => Q::Rotation { p: mm(point), z: axis.to_array(), r: radius * 1000., angle: 0. },
        SurfaceKind::Cone { point, axis, radius, sin_half_angle, cos_half_angle, .. } => {
            Q::Rotation { p: mm(point), z: axis.to_array(), r: radius * 1000., angle: sin_half_angle.atan2(cos_half_angle) }
        }
        SurfaceKind::Sphere { center, radius, .. } => Q::Sphere { p: mm(center), radius: radius * 1000. },
        SurfaceKind::Torus { center, axis, major_radius, minor_radius, .. } => {
            Q::Torus { p: mm(center), z: axis.to_array(), major: major_radius * 1000., minor: minor_radius * 1000. }
        }
        _ => return Err("intersection support".into()),
    })
}
fn recover_intersections(m: &mut BrepModel) -> R<()> {
    use cadmpeg_ir::geometry::SolvedCurveGeometry as C;
    let v = |p: cadmpeg_ir::math::Vector3| Vector3 { x: p.x, y: p.y, z: p.z };
    let p = |p: cadmpeg_ir::math::Point3| Vector3 { x: p.x / 1000., y: p.y / 1000., z: p.z / 1000. };
    for c in &mut m.curves {
        let CurveKind::Intersection { surfaces, chart_points, .. } = &c.kind else { continue };
        if !m.edges.iter().any(|e| e.curve == Some(c.id)) {
            continue;
        }
        let a = chart_points.first().ok_or("intersection branch")?.to_array().map(|v| v * 1000.);
        let b = chart_points.last().ok_or("intersection branch")?.to_array().map(|v| v * 1000.);
        let aa = support(&m.surfaces.iter().find(|s| s.id == surfaces[0]).ok_or("support")?.kind);
        let bb = support(&m.surfaces.iter().find(|s| s.id == surfaces[1]).ok_or("support")?.kind);
        let (Ok(aa), Ok(bb)) = (aa, bb) else { continue };
        let Some(solved) =
            crate::native_cad::sldprt::analytic::conic(&aa, &bb, a, b)?.or(crate::native_cad::sldprt::analytic::conic(&bb, &aa, a, b)?)
        else {
            continue;
        };
        let parameter = |q: [f64; 3]| -> R<f64> {
            let dot = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
            let read =
                |center: cadmpeg_ir::math::Point3, z: cadmpeg_ir::math::Vector3, x: cadmpeg_ir::math::Vector3, ra: f64, rb: f64, hyper: bool| {
                    let x = [x.x, x.y, x.z];
                    let z = [z.x, z.y, z.z];
                    let y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]];
                    let d = [q[0] - center.x, q[1] - center.y, q[2] - center.z];
                    if hyper { (dot(d, y) / rb).asinh() } else { (dot(d, y) / rb).atan2(dot(d, x) / ra) }
                };
            Ok(match &solved {
                C::Circle(g) => {
                    read(g.center().get(), *g.frame().axis().as_raw(), *g.frame().reference().as_raw(), g.radius().get(), g.radius().get(), false)
                }
                C::Ellipse(g) => read(
                    g.center().get(),
                    *g.frame().axis().as_raw(),
                    *g.frame().reference().as_raw(),
                    g.major_radius().get(),
                    g.minor_radius().get(),
                    false,
                ),
                C::Parabola(g) => {
                    let o = g.vertex().get();
                    let z = *g.frame().axis().as_raw();
                    let x = *g.frame().reference().as_raw();
                    let y = [z.y * x.z - z.z * x.y, z.z * x.x - z.x * x.z, z.x * x.y - z.y * x.x];
                    dot([q[0] - o.x, q[1] - o.y, q[2] - o.z], y) / (2. * g.focal_distance().get())
                }
                C::Hyperbola(g) => read(
                    g.center().get(),
                    *g.frame().axis().as_raw(),
                    *g.frame().reference().as_raw(),
                    g.major_radius().get(),
                    g.minor_radius().get(),
                    true,
                ),
                _ => return Err("intersection carrier".into()),
            })
        };
        let params = chart_points.iter().map(|q| parameter(q.to_array().map(|v| v * 1000.))).collect::<R<Vec<_>>>()?;
        let hyper = matches!(&solved, C::Hyperbola(_) | C::Parabola(_));
        let diffs = params
            .windows(2)
            .map(|v| {
                let d = v[1] - v[0];
                if hyper { d } else { (d + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI }
            })
            .collect::<Vec<_>>();
        let sign = diffs.iter().sum::<f64>();
        if sign.abs() < 1e-12 || diffs.iter().any(|d| d * sign < -1e-10) {
            return Err("ambiguous exact intersection branch orientation".into());
        }
        if sign < 0. {
            c.sense = if c.sense == Sense::Positive { Sense::Negative } else { Sense::Positive };
        }
        c.kind = match solved {
            C::Circle(g) => CurveKind::Circle {
                center: p(g.center().get()),
                normal: v(*g.frame().axis().as_raw()),
                x_axis: v(*g.frame().reference().as_raw()),
                radius: g.radius().get() / 1000.,
            },
            C::Ellipse(g) => CurveKind::Ellipse {
                center: p(g.center().get()),
                normal: v(*g.frame().axis().as_raw()),
                x_axis: v(*g.frame().reference().as_raw()),
                major_radius: g.major_radius().get() / 1000.,
                minor_radius: g.minor_radius().get() / 1000.,
            },
            C::Parabola(g) => CurveKind::Parabola {
                origin: p(g.vertex().get()),
                normal: v(*g.frame().axis().as_raw()),
                x_axis: v(*g.frame().reference().as_raw()),
                focal_length: g.focal_distance().get() / 1000.,
            },
            C::Hyperbola(g) => CurveKind::Hyperbola {
                origin: p(g.center().get()),
                normal: v(*g.frame().axis().as_raw()),
                x_axis: v(*g.frame().reference().as_raw()),
                transverse_radius: g.major_radius().get() / 1000.,
                conjugate_radius: g.minor_radius().get() / 1000.,
            },
            _ => return Err("intersection result".into()),
        };
    }
    Ok(())
}
