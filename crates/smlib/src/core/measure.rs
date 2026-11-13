//! The Measure and Check families on solidcore: what `capi/src/smlib_measure.cpp`,
//! `smlib_brep_solid_count` (`smlib_ops.cpp`), `smlib_brep_is_manifold_solid`
//! (`smlib_flat_api.cpp`) and `smlib_brep_check` (`smlib_check.cpp`) do on the C++ kernel, here
//! composed from solidcore's ports of the same SMLib routines.
//!
//! [`check`] fails with "not supported yet" while a test it reaches is unported in solidcore.

use solidcore::geom::assert_array::{AssertArray, AssertReport, obj_type_string};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::tol::{SmallTopoType, context_xsect_tol_3d};
use solidcore::heal::gap::GapFunction;
use solidcore::mass::integrator::properties::{MassPropertyBehavior, PropertyAttributes, brep_properties, face_properties};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::{brep_bounding_box, brep_is_manifold_solid, brep_material_census, face_bounding_box};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::validation::{CheckLevel as SolidcoreCheckLevel, check_brep};
use solidcore::topology::face::p2::{PPF_AREA, PPF_AREA_FIRST_MOMENTS};
use solidcore::topology::{FaceId, FaceuseId, NamingScheme};
use solidcore::{Kernel, Point3, SolidError, Vec3};

use solidcore_ops::measure::{LocateFailure, Located, locate_points};

use super::blend::status;
use super::{Body, with_body};
use crate::check::{CheckLevel, CheckReport};
use crate::measure::{MassProperties, PointClass};
use crate::{Error, Result};

const OP: &str = "smlib_measure";

/// The accuracy the C API passes on: finite, clamped to [1e-8, 1e-1].
pub(super) fn accuracy(relative_accuracy: f64) -> Result<f64> {
    if !relative_accuracy.is_finite() {
        return Err(Error::Argument);
    }
    Ok(relative_accuracy.clamp(1e-8, 1e-1))
}

fn invalid(why: &'static str) -> SolidError {
    SolidError::invalid_input(OP, why)
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// `faces_area` of `smlib_measure.cpp`: every face counted once (SmApiBrepComputeArea, with the
/// [1e-8, 1e-1] envelope instead of its [1e-4, 1e-1]), the total area and, when `first` is set,
/// the area's first moments about `o`. With `only_sheets`, only [`sheet_face`]s (the capi's
/// `counted`).
fn faces_area(k: &Kernel, b: &mut solidcore::Brep, accuracy: f64, o: Point3, first: bool, only_sheets: bool) -> solidcore::Result<(f64, Vec3)> {
    let mut area = 0.0;
    let mut sum = Vec3::new(0.0, 0.0, 0.0);
    let flags = if first { PPF_AREA | PPF_AREA_FIRST_MOMENTS } else { PPF_AREA };
    for f in bl::faces(b, None)? {
        if only_sheets && !sheet_face(b, f)? {
            continue;
        }
        let size = face_bounding_box(k, b, f, false)?.size();
        let edge = size.z.max(size.x.max(size.y));
        let p = face_properties(k, b, f, OrientType::Same, accuracy, o, edge * edge / 100.0, 0.0, flags)?;
        // The area and its moments share one integrand, so a trim boundary integrated the other
        // way round flips both.
        let sign = if p.area < 0.0 { -1.0 } else { 1.0 };
        area += sign * p.area;
        if first {
            sum += p.moments[0] * sign;
        }
    }
    if !area.is_finite() {
        return Err(invalid("non-finite area"));
    }
    Ok((area, sum))
}

/// `sheet_face`: a face with material on neither side, standing free or hung on a solid (a blend
/// sheet stitched between two boxes). `SmBrep::ComputePreciseProperties` takes its area only from
/// faces between a solid region and a void, so such a face is counted on its own.
fn sheet_face(b: &solidcore::Brep, f: FaceId) -> solidcore::Result<bool> {
    let void_side = |u: Option<FaceuseId>| -> solidcore::Result<bool> {
        let Some(u) = u else { return Ok(true) };
        let Some(shell) = b.faceuse(u)?.shell else { return Ok(true) };
        let Some(region) = b.shell(shell)?.region else { return Ok(true) };
        // IsVoid's UNSURE (2) is true in C++: only FALSE is material.
        Ok(b.region(region)?.is_void() != 0)
    };
    let (x, y) = solidcore::topology::face::p2::faceuses(b, f)?;
    Ok(void_side(x)? && void_side(y)?)
}

/// `smlib_brep_mass_properties`: `SmBrep::ComputePreciseProperties` directly, as the C API calls
/// it (not `SmApiBrepComputeMassProperties`: the [1e-8, 1e-1] accuracy envelope, no manifold
/// refusal, the sum over material regions), the face-by-face area and its centroid for a body
/// with no material, and moments about the middle of the box moved to the centroid. A body's area
/// is every face's: the solid's boundary and its sheet faces (fork 9767916).
pub(crate) fn mass_properties(body: &Body, relative_accuracy: f64) -> Result<MassProperties> {
    let accuracy = accuracy(relative_accuracy)?;
    with_body("mass_properties", body, |k, b| {
        let mut material = false;
        for r in bl::regions(b, None)? {
            // IsVoid's UNSURE (2) is true in C++: only FALSE is material.
            material |= b.region(r)?.is_void() == 0;
        }
        // The C API refuses a body with per-entity mass attributes (they would override the unit
        // density). Bodies here carry none: the attribute store passed in is empty.
        let store = AttributeStore::default();
        let attrs = PropertyAttributes { store: &store, key_of: &|_| ObjectKey(0) };
        // Moments about the middle of the box, moved to the centroid below: about the world
        // origin, a body far from it would lose its inertia to cancellation.
        let o = brep_bounding_box(k, b, false)?.mid();
        if !finite(&[o.x, o.y, o.z]) {
            return Err(invalid("non-finite box"));
        }
        if !material {
            let (area, first) = faces_area(k, b, accuracy, o, true, false)?;
            if !(area > 0.0) {
                return Err(invalid("no area"));
            }
            let c = [o.x + first.x / area, o.y + first.y / area, o.z + first.z / area];
            if !finite(&[area, c[0], c[1], c[2]]) {
                return Err(invalid("non-finite result"));
            }
            return Ok(MassProperties { area, volume: 0.0, centroid: c, moments: [0.0; 3], products: [0.0; 3] });
        }
        let p = brep_properties(k, b, accuracy, o, MassPropertyBehavior { density: 1.0, ..MassPropertyBehavior::default() }, &attrs)?;
        let (mass, volume) = (p.mass, p.volume);
        if !mass.is_finite() || !(mass > 0.0) || !(volume > 0.0) {
            return Err(invalid("no mass"));
        }
        // m[4]: first moments about o; m[7]: (y²+z², z²+x², x²+y²) about o; m[6]: (yz, zx, xy)
        // about o. Parallel axes from o to the centroid (unit density: mass = volume).
        let [_, _, _, _, m4, _, m6, m7] = p.moments;
        let d = [m4.x / mass, m4.y / mass, m4.z / mass];
        let (sheets, _) = faces_area(k, b, accuracy, o, false, true)?;
        let r = MassProperties {
            area: p.area + sheets,
            volume,
            centroid: [o.x + d[0], o.y + d[1], o.z + d[2]],
            moments: [
                m7.x - mass * (d[1] * d[1] + d[2] * d[2]),
                m7.y - mass * (d[0] * d[0] + d[2] * d[2]),
                m7.z - mass * (d[0] * d[0] + d[1] * d[1]),
            ],
            products: [m6.x - mass * d[1] * d[2], m6.y - mass * d[2] * d[0], m6.z - mass * d[0] * d[1]],
        };
        if !finite(&[r.area, r.volume]) || !finite(&r.centroid) || !finite(&r.moments) || !finite(&r.products) {
            return Err(invalid("non-finite result"));
        }
        Ok(r)
    })
}

/// `smlib_brep_area`: every face counted once.
pub(crate) fn area(body: &Body, relative_accuracy: f64) -> Result<f64> {
    let accuracy = accuracy(relative_accuracy)?;
    with_body("area", body, |k, b| faces_area(k, b, accuracy, Point3::new(0.0, 0.0, 0.0), false, false).map(|(a, _)| a))
}

/// `smlib_brep_bounds`: `SmBrep::CalculateTightBoundingBox`.
pub(crate) fn bounds(body: &Body) -> Result<([f64; 3], [f64; 3])> {
    with_body("bounds", body, |k, b| {
        let x = brep_bounding_box(k, b, true)?;
        let (lo, hi) = (x.min(), x.max());
        let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
        if !finite(&lo) || !finite(&hi) || lo.iter().zip(&hi).any(|(a, b)| a > b) {
            return Err(invalid("no box"));
        }
        Ok((lo, hi))
    })
}

/// `smlib_brep_edge_face_distance`: SMLib's own edge/face gap (`SmEdgeuse::CalcMaxEdgeFaceGap`'s
/// `SmCrvSrfGapFunction` with 20 samples, its largest sample). Indices in the body's edge and
/// face order.
pub(crate) fn edge_face_distance(body: &Body, edge: usize, face: usize) -> Result<f64> {
    let found = with_body("edge_face_distance", body, |k, b| {
        let (Some(&e), Some(&f)) = (bl::edges(b, None)?.get(edge), bl::faces(b, None)?.get(face)) else {
            return Ok(None);
        };
        let interval = b.edge(e)?.interval;
        let (Some(curve), Some(surface)) = (b.edge(e)?.curve.as_ref(), b.face(f)?.surface.as_ref()) else {
            return Err(invalid("edge without curve or face without surface"));
        };
        if !(interval.max() > interval.min()) {
            return Err(invalid("empty edge interval"));
        }
        // SmTol::GetXSectTol3d(brep's context): the process context. The tolerance only
        // classifies the samples; it does not change the distances.
        let tol = context_xsect_tol_3d(k, None, SmallTopoType::NormalSize);
        let mut gaps = GapFunction::crv_srf(tol, curve, &interval, surface, 20);
        let d = gaps.max_gap_sample(k, false)?.length();
        if !d.is_finite() {
            return Err(invalid("non-finite gap"));
        }
        Ok(Some(d))
    })?;
    found.ok_or(Error::Argument)
}

/// `smlib_brep_classify_point`: `SmBrep::Point3DClassify` at the body's own zone tolerance
/// (`SmTol::GetZoneTol3d` of the body), with boundary intersections, so a point within that
/// tolerance of a vertex, an edge or a face is on the boundary and never decided by a ray; else
/// the region it lies in, outside when that region is void (`IsVoid() == TRUE`; `UNSURE` is
/// material, as the capi compares with `TRUE`). The classification is the modelling layer's
/// (`solidcore_ops::measure::locate_points`), which runs on a private copy when a solve needs one:
/// upstream's `const` method may add UV trim curves to the body.
pub(crate) fn classify_point(body: &Body, p: [f64; 3]) -> Result<PointClass> {
    if !finite(&p) {
        return Err(Error::Argument);
    }
    with_body("classify_point", body, |k, b| Ok(point_class(locate_points(k, b, &[p]).pop().unwrap_or(Err(LocateFailure::Unclassified)))))?
}

/// [`classify_point`] for each of `points`, through one classification of the body
/// (`locate_points`): the body's private copy and its search structures are made once for all of
/// them, not once a point (on a large body they cost far more than a point's own solve). Each
/// point's class, or its own error.
pub(crate) fn classify_points(body: &Body, points: &[[f64; 3]]) -> Result<Vec<Result<PointClass>>> {
    if points.iter().all(|p| !finite(p)) {
        return Ok(points.iter().map(|_| Err(Error::Argument)).collect());
    }
    with_body("classify_points", body, |k, b| Ok(locate_points(k, b, points).into_iter().map(point_class).collect()))
}

/// [`classify_point`]'s class of a point the modelling layer placed. A region the kernel is unsure
/// of is material (the capi's `IsVoid() == TRUE` test), where `solidcore_ops`' own
/// `Body::classify_point` refuses it.
fn point_class(l: std::result::Result<Located, LocateFailure>) -> Result<PointClass> {
    match l {
        Ok(Located::OnBoundary) => Ok(PointClass::OnBoundary),
        Ok(Located::InRegion { material: Some(false), .. }) => Ok(PointClass::Outside),
        Ok(Located::InRegion { material: Some(true) | None, .. }) => Ok(PointClass::Inside),
        Err(LocateFailure::NotFinite) => Err(Error::Argument),
        // Its status, as the capi passes it on (no ray finding a region is SM_ERR).
        Err(LocateFailure::Classify(e)) => Err(status(e)),
        Err(LocateFailure::Region(e)) => Err(Error::Solidcore(format!("classify_point: {e}"))),
        // Point3DClassify reports only the four classes on success: SM_ERR.
        Err(LocateFailure::Unclassified) => Err(Error::Status(SM_ERR)),
    }
}

/// `SM_ERR`.
const SM_ERR: i32 = 1001;

/// `smlib_brep_is_manifold_solid`: `SmApiBrepIsManifoldSolid`.
pub(crate) fn is_manifold_solid(body: &Body) -> Result<bool> {
    with_body("is_manifold_solid", body, |_, b| brep_is_manifold_solid(b))
}

/// `smlib_brep_solid_count`: the solids of `SmApiBrepMaterialCensus` (its voids are dropped).
pub(crate) fn solid_count(body: &Body) -> Result<usize> {
    with_body("solid_count", body, |_, b| brep_material_census(b).map(|(solids, _voids)| solids))
}

/// SMLib's `SM_SUCCESS`, the status `SmBrep::ValidatePointers` returns when it finds nothing.
const SM_SUCCESS: i32 = 1000;

/// The capi's `text()`: an empty string reads as `?`.
fn text(s: &str) -> &str {
    if s.is_empty() { "?" } else { s }
}

/// The capi's `line()`: `<reporting type>::Test #<n> <name>: <message> [owner <type>]`, the owner
/// tag only when the report names the owner's type.
fn report_line(r: &AssertReport) -> String {
    let msg = r.format_log_message();
    let mut s = format!("{}::Test #{} {}: {}", text(obj_type_string(r.reporting_type)), r.test_index, text(r.name), text(&msg));
    if let Some(owner) = r.owner_type_string.filter(|o| !o.is_empty()) {
        s.push_str(" [owner ");
        s.push_str(owner);
        s.push(']');
    }
    s
}

/// The capi's `collect()`: appends the failed reports of `list`, one line each, and counts them;
/// SMLib's warnings ([`AssertReport::is_warning`]: `SmBrep` test 21, "Suspicious Shell Count", a
/// body of several faces with one shell each, is one) are neither.
fn collect(list: &AssertArray, out: &mut String, failures: &mut usize) {
    for r in list.reports().iter().filter(|r| !r.ok && !r.is_warning()) {
        *failures += 1;
        out.push_str(&report_line(r));
        out.push('\n');
    }
}

/// Why [`check`] could not run.
enum Refusal {
    /// A test solidcore has not ported (its [`SolidError::Unported`]).
    Unported(&'static str, &'static str),
    /// A body whose geometry owners' identities the check cannot know: a
    /// [`NamingScheme::Unknown`] body.
    Naming(NamingScheme),
}

impl<T> super::dump::Answer for std::result::Result<T, Refusal> {
    fn refusal(&self) -> Option<String> {
        self.is_err().then(|| "the check could not run".to_string())
    }
}

/// `smlib_brep_check` (`smlib_check.cpp`): `SmBrep::ValidatePointers` at `level`, then, when it
/// failed nothing, `SmBrep::AssertValid` at `level` over the whole topology graph, with every
/// failed test as one line of the report and the capi's own lines for a failed status without a
/// report. solidcore's [`check_brep`] runs the same two calls.
///
/// The geometry owners are named as the body's naming scheme names them: `check_brep` reads a
/// [`NamingScheme::Caller`] body's keys from where its maker recorded them (solidcore#1585: an
/// array import through `make_topology_from_data`, `api_brep_copy`); where it recorded none, the
/// arena identities the binders name such an entity by. A [`NamingScheme::Unknown`] body would be
/// checked under arena identities no maker uses: every edge would fail `SmBrep` test 61 on the
/// owner of its curve.
///
/// A test solidcore has not ported yet (its [`SolidError::Unported`]), or a body whose geometry
/// owners' identities the check cannot know, fails the check with "not supported yet": the check
/// never reports a body valid on fewer tests than the C++ kernel runs.
pub(crate) fn check(body: &Body, level: CheckLevel) -> Result<CheckReport> {
    check_with(body, level, false)
}

/// [`check`], stopped by the token over this thread's work when `stoppable` (a body the caller
/// consumes: [`super::with_own_body`]).
pub(crate) fn check_with(body: &Body, level: CheckLevel, stoppable: bool) -> Result<CheckReport> {
    let lv = match level {
        CheckLevel::Basic => SolidcoreCheckLevel::Level0,
        CheckLevel::Geometry => SolidcoreCheckLevel::Level1,
        CheckLevel::All => SolidcoreCheckLevel::Level2,
    };
    let checked = |k: &mut Kernel, b: &mut solidcore::topology::Brep| {
        let naming = b.naming();
        if naming == NamingScheme::Unknown {
            return Ok(Err(Refusal::Naming(naming)));
        }
        match check_brep(k, b, lv) {
            Err(SolidError::Unported { upstream, unit }) => Ok(Err(Refusal::Unported(upstream, unit))),
            r => r.map(Ok),
        }
    };
    let ran = if stoppable { super::with_own_body("check", body, checked) } else { with_body("check", body, checked) };
    let c = match ran? {
        Ok(c) => c,
        Err(Refusal::Unported(upstream, unit)) => {
            return Err(Error::Solidcore(format!("smlib_brep_check: {upstream}: not supported yet: solidcore has not ported it (unit {unit})")));
        }
        Err(Refusal::Naming(n)) => {
            return Err(Error::Solidcore(format!(
                "smlib_brep_check: not supported yet: the body names its geometry owners by its maker's keys (NamingScheme::{n:?}), which the check cannot know"
            )));
        }
    };
    let mut text = String::new();
    let mut failures = 0usize;
    collect(&c.pointer_reports, &mut text, &mut failures);
    if c.pointers_status != SM_SUCCESS && failures == 0 {
        failures += 1;
        text.push_str(&format!("SmBrep::ValidatePointers failed with status {}\n", c.pointers_status));
    }
    if failures > 0 {
        // The capi does not run AssertValid after a failed pointer check. check_brep runs it when
        // the status failed without a report; that result is dropped here, as the capi never
        // computes it.
        text.push_str("AssertValid skipped: ValidatePointers failed\n");
    } else {
        let Some(ok) = c.assert_valid else {
            return Err(Error::Solidcore("smlib_brep_check: AssertValid did not run after a clean pointer check".into()));
        };
        collect(&c.reports, &mut text, &mut failures);
        if !ok && failures == 0 {
            failures += 1;
            text.push_str("SmBrep::AssertValid failed without a report\n");
        }
    }
    Ok(CheckReport { failures, text })
}
