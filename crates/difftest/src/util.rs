//! Small conversions shared by the unit runners in [`crate::units`].

#[cfg(feature = "cpp")]
pub(crate) mod exact_curve;
pub mod primitive;

use solidcore::geom::object::ObjectRef;
use solidcore::math::{Point2, Vec3};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::{Brep, EdgeId, FaceId};

/// A JSON `[x, y, z]` as a [`Vec3`].
#[must_use]
pub fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A JSON `[u, v]` as a [`Point2`].
#[must_use]
pub fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

/// A boolean as an output value: `1.0` or `0.0`.
#[must_use]
pub fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

// ---- exact analytic face surfaces of the shared B-rep exports ---------------------------

/// The exact head of an analytic face surface in the shared B-rep graph exports
/// (`harness/cpp/shim/sc_analytic_export.h`), read after its kind code `2`:
/// `type version surf_oob gen_oob nbytes byte[nbytes]`, the surface's upstream binary
/// database record (`SmSurfOfRevolution`, `SmCone`, `SmCylinder`, `SmSphere`, `SmTorus` or
/// `SmSurfOfExtrusion`)
/// and the out-of-bounds flags the
/// record does not hold. The regular surface block (the B-spline part) follows it; the
/// caller reads that too and checks it with [`check_exact_base`].
///
/// Returns the surface rebuilt by the ported `ReadFromDB`, or `None` when that reader
/// is not ported (`SolidError::Unported`): the caller then keeps the B-spline part, as
/// for any kind the export does not carry exactly.
///
/// # Errors
///
/// A [`crate::HarnessError`] for a malformed head, a type the export never sends, a
/// reader error other than `Unported`, or a record the reader does not consume whole.
pub fn read_exact_surface(
    num: &mut dyn FnMut() -> Result<f64, crate::HarnessError>,
) -> Result<Option<solidcore::surface::surface::Surface>, crate::HarnessError> {
    use solidcore::kernel::Kernel;
    use solidcore::surface::surf_of_revolution::SurfOfRevolution;
    use solidcore::surface::surface::p2::{
        CONE_TYPE, CYLINDER_TYPE, SPHERE_TYPE, SURF_OF_EXTRUSION_TYPE, SURF_OF_REVOLUTION_TYPE,
        TORUS_TYPE,
    };
    use solidcore::surface::surface::{Surface, SurfaceKind};
    use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
    use solidcore::SolidError;
    let harness = |m: String| crate::HarnessError::new(m);
    let ty = num()? as i64;
    let version = num()?;
    let surf_oob = num()? != 0.0;
    let gen_oob = num()? != 0.0;
    let n = num()?;
    if !(0.0..=1e8).contains(&n) || n.fract() != 0.0 || !(0.0..=1e4).contains(&version) {
        return Err(harness(format!(
            "export: bad exact surface head ({version}, {n})"
        )));
    }
    let mut bytes = Vec::new();
    for _ in 0..n as usize {
        let b = num()?;
        if !(0.0..=255.0).contains(&b) || b.fract() != 0.0 {
            return Err(harness(format!("export: bad record byte {b}")));
        }
        bytes.push(b as u8);
    }
    if ty != SURF_OF_REVOLUTION_TYPE
        && ty != CONE_TYPE
        && ty != CYLINDER_TYPE
        && ty != SPHERE_TYPE
        && ty != TORUS_TYPE
        && ty != SURF_OF_EXTRUSION_TYPE
    {
        return Err(harness(format!("export: exact surface of type {ty}")));
    }
    let k = Kernel::default();
    let version = version as u64;
    let mut db = DatabaseReader::with_stream(FileType::Binary, InputStream::new(bytes));
    db.set_version(version);
    let mut s = match Surface::read_from_db(&k, ty, &mut db, version) {
        Ok(s) => s,
        Err(SolidError::Unported { .. }) => return Ok(None),
        Err(e) => return Err(harness(format!("export: surface record of type {ty}: {e}"))),
    };
    if db.in_stream().is_none_or(|st| st.remaining() != 0) {
        return Err(harness(format!(
            "export: surface record of type {ty} not read whole"
        )));
    }
    // the flags beside the record: SetOutOfBoundsEnabled sets the surface's and the
    // generator's, then the surface's own is put back
    if let SurfaceKind::SurfOfExtrusion(e) = s.kind_mut() {
        e.set_out_of_bounds_enabled(gen_oob);
        *e.base_mut().out_of_bounds_enabled_mut() = surf_oob;
        return Ok(Some(s));
    }
    if let SurfaceKind::Cone(c) = s.kind_mut() {
        c.set_out_of_bounds_enabled(gen_oob);
        *c.base_mut().out_of_bounds_enabled_mut() = surf_oob;
        return Ok(Some(s));
    }
    let rev: &mut SurfOfRevolution = match s.kind_mut() {
        SurfaceKind::SurfOfRevolution(r) => r,
        SurfaceKind::Sphere(r) => r.base_mut(),
        SurfaceKind::Torus(r) => r.base_mut(),
        _ => {
            return Err(harness(format!(
                "export: exact surface of type {ty} read as another kind"
            )))
        }
    };
    rev.set_out_of_bounds_enabled(gen_oob);
    *rev.base_mut().out_of_bounds_enabled_mut() = surf_oob;
    Ok(Some(s))
}

/// Checks that an exactly imported surface's B-spline part is bit for bit the regular
/// block exported after it (`fallback`, a B-spline surface): its NURB, form and
/// out-of-bounds flag.
///
/// # Errors
///
/// A [`crate::HarnessError`] naming the first difference.
pub fn check_exact_base(
    exact: &solidcore::surface::surface::Surface,
    fallback: Option<&solidcore::surface::surface::Surface>,
) -> Result<(), crate::HarnessError> {
    use solidcore::surface::surface::SurfaceKind;
    let base = match exact.kind() {
        SurfaceKind::SurfOfRevolution(s) => s.base(),
        SurfaceKind::Sphere(s) => s.base().base(),
        SurfaceKind::Torus(s) => s.base().base(),
        SurfaceKind::SurfOfExtrusion(s) => s.base(),
        SurfaceKind::Cone(s) => s.rev().base(),
        _ => {
            return Err(crate::HarnessError::new(
                "export: unexpected exact surface kind",
            ))
        }
    };
    let Some(SurfaceKind::BSpline(fb)) = fallback.map(solidcore::surface::surface::Surface::kind)
    else {
        return Err(crate::HarnessError::new(
            "export: exact surface without its B-spline part",
        ));
    };
    if nurb_bits(base) != nurb_bits(fb)
        || base.form() != fb.form()
        || base.is_out_of_bounds_enabled() != fb.is_out_of_bounds_enabled()
    {
        return Err(crate::HarnessError::new(
            "export: the exact surface's B-spline part differs from the exported one",
        ));
    }
    Ok(())
}

/// A B-spline surface's NURB as bits, in the export's block order (`None`: no NURB).
fn nurb_bits(b: &solidcore::nurbs::b_spline_surface::BSplineSurface) -> Option<Vec<u64>> {
    let nurb = b.gw_nurb()?;
    let (p, q) = nurb.degrees();
    let mut v = vec![nurb.n() as u64, nurb.m() as u64, p as u64, q as u64];
    for row in nurb.cpts() {
        for pt in row {
            v.extend([pt.x, pt.y, pt.z, pt.w].map(f64::to_bits));
        }
    }
    v.extend(nurb.u_knots().iter().map(|x| x.to_bits()));
    v.extend(nurb.v_knots().iter().map(|x| x.to_bits()));
    Some(v)
}

/// Gives every edge curve and face surface of an imported B-rep the owner a native
/// B-rep's geometry has (its edge or face, by [`arena_identity`]; curves also the
/// B-rep), which the B-rep cache solves read (the import leaves geometry unowned).
pub fn own_arena_geometry(b: &mut Brep) {
    own_edge_curves(
        b,
        &|id| arena_identity(CacheObject::Edge(id)),
        Some(arena_identity(CacheObject::Brep)),
    );
    own_face_surfaces(b, &|id| arena_identity(CacheObject::Face(id)));
}

/// Gives every edge curve of an imported B-rep its edge as owner, under `edge_key`:
/// the name the runner hands the library for that edge (its `key_of` or cache keys).
/// The topology solver's B-rep trees and edge lookups compare owners with those names;
/// `brep` is the owner edge's B-rep as `Curve::set_owner` takes it.
pub fn own_edge_curves(
    b: &mut Brep,
    edge_key: &dyn Fn(EdgeId) -> ObjectRef,
    brep: Option<ObjectRef>,
) {
    let edges: Vec<_> = b.edges().ids().collect();
    for id in edges {
        if let Some(curve) = b.edge_mut(id).ok().and_then(|e| e.curve.as_mut()) {
            let _ = curve.set_owner(Some(edge_key(id)), brep);
        }
    }
}

/// Gives every face surface of an imported B-rep its face as owner, under `face_key`
/// (as [`own_edge_curves`] for edges).
pub fn own_face_surfaces(b: &mut Brep, face_key: &dyn Fn(FaceId) -> ObjectRef) {
    let faces: Vec<_> = b.faces().ids().collect();
    for id in faces {
        if let Some(surface) = b.face_mut(id).ok().and_then(|f| f.surface.as_mut()) {
            let _ = surface.set_owner(Some(face_key(id)));
        }
        let _ = b.sync_face_surface_tolerance(id);
    }
}
