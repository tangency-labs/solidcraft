//! Runner for unit `topology.brep_p8`. C++: `solidcore_ffi::units::topology_brep_p8`.
//! Rust: `solidcore::topology::brep::p8`.
//!
//! **Ops.** Each case runs one face builder on a new B-rep (`new SmBrep()`, its void
//! infinite region): `in_region` (`CreateFaceInRegionFromSurface`), `from_surface`
//! (`CreateFaceFromSurface`), `faces_from_surface` (`CreateFacesFromSurface`),
//! `planar_face` / `planar_faces` (`CreatePlanarFace(s)With3DCurves`),
//! `faces_with_curves` (`CreateFacesWithCurves`) and `make_faces`
//! (`MakeFacesWithCurves`). Surfaces are real `SmPlane`s or `SmBSplineSurface`s
//! (including ones with `C0` knots, which the split paths take, and closed or pinched
//! ones); 3D curves are `SmLine`s or `SmBSplineCurve`s, UV curves `SmBSplineCurve`s.
//!
//! **Outputs.** The status, the op's results (new objects as indices in a walk of the
//! B-rep after the op), then the whole B-rep after the op, also after a failure: the
//! counts and shell digest, the full topology graph of the `topology.brep_p3` runner
//! (every owner and adjacency link) and the geometry of every vertex (point,
//! tolerance), edge (tolerance, interval, curve), face (tolerance, UV domain, rectangular
//! trim flag, surface) and edge use (UV trim curve), curves and surfaces as their type,
//! natural domain and sampled points (layout in the ffi module).
//!
//! **Guards.** The FFI wrapper rejects what upstream does not handle safely (see its
//! docs); generated cases avoid those inputs. A plane fit whose loop tessellation has
//! zero area (curves far from the origin relative to their size) makes upstream
//! `FindOuterLoops` read past its arrays (`plan/upstream-issues.md` UB-119); the port
//! refuses it (`InvalidInput` from `topology::trimming_tools`) and the case defers as
//! `cpp-guard:ub-119`: the shim cannot tell it before the call.
//!
//! **Deferred.** Calls into units that are not ported return `Unported` on the Rust side
//! (`DeleteEdge` / `DeleteVertex` of `topology.brep_p4`, and further down the Euler
//! operators' own deferrals).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p7::FaceCurvesInput;
use solidcore::topology::brep::p8::{self, FaceBuildEnv};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, RegionId, ShellId, VertexId,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_brep_p8::{self as ffi, Op};

use super::topology_brep_p3::{put_counts, walk, Fail};
use super::topology_vertexuse::{push_curve, push_surface, BssIn, CrvIn, SrfIn};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

const UNIT: &str = "topology.brep_p8";

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn hfail(e: impl core::fmt::Display) -> Fail {
    Fail::Harness(harness(e.to_string()))
}

// ---- inputs -----------------------------------------------------------------------------

/// A 3D curve: a B-spline or an `SmLine(a, b, 3)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum C3In {
    Bspline(CrvIn),
    Line { a: [f64; 3], b: [f64; 3] },
}

/// A curve of `faces_with_curves`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OrientedCurve {
    c: C3In,
    orient: u8,
}

/// A curve of `make_faces`: its 3D and / or UV shape and orientation.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoopCurve {
    #[serde(default)]
    c3: Option<C3In>,
    #[serde(default)]
    uv: Option<CrvIn>,
    orient: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    InRegion {
        surface: Option<SrfIn>,
        #[serde(default)]
        region_null: bool,
        dom: [f64; 4],
    },
    FromSurface {
        surface: Option<SrfIn>,
        dom: [f64; 4],
    },
    FacesFromSurface {
        surface: Option<SrfIn>,
        continuity: u8,
    },
    PlanarFace {
        #[serde(default)]
        region_null: bool,
        tol: f64,
        curves: Vec<C3In>,
    },
    PlanarFaces {
        #[serde(default)]
        region_null: bool,
        tol: f64,
        curves: Vec<C3In>,
    },
    FacesWithCurves {
        surface: SrfIn,
        #[serde(default)]
        region_null: bool,
        loops: Vec<usize>,
        #[serde(default = "yes")]
        has3d: bool,
        curves: Vec<OrientedCurve>,
    },
    MakeFaces {
        surface: SrfIn,
        dom: [f64; 4],
        orient: u8,
        #[serde(default)]
        region_null: bool,
        loops: Vec<usize>,
        has3d: bool,
        has_uv: bool,
        curves: Vec<LoopCurve>,
        #[serde(default)]
        points: Vec<[f64; 3]>,
    },
}

const fn yes() -> bool {
    true
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

// ---- encoding ---------------------------------------------------------------------------

fn push_surface_opt(v: &mut Vec<f64>, s: Option<&SrfIn>) -> Result<(), HarnessError> {
    match s {
        None => {
            v.push(-1.0);
            Ok(())
        }
        Some(s) => push_surface(v, s),
    }
}

fn push_c3(v: &mut Vec<f64>, c: &C3In) -> Result<(), HarnessError> {
    match c {
        C3In::Bspline(c) => {
            v.push(0.0);
            push_curve(v, c)
        }
        C3In::Line { a, b } => {
            v.push(1.0);
            v.extend(a);
            v.extend(b);
            Ok(())
        }
    }
}

fn encode(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    let op = match i {
        Inputs::InRegion {
            surface,
            region_null,
            dom,
        } => {
            push_surface_opt(&mut v, surface.as_ref())?;
            v.push(flag(*region_null));
            v.extend(dom);
            Op::InRegion
        }
        Inputs::FromSurface { surface, dom } => {
            push_surface_opt(&mut v, surface.as_ref())?;
            v.extend(dom);
            Op::FromSurface
        }
        Inputs::FacesFromSurface {
            surface,
            continuity,
        } => {
            push_surface_opt(&mut v, surface.as_ref())?;
            v.push(f64::from(*continuity));
            Op::FacesFromSurface
        }
        Inputs::PlanarFace {
            region_null,
            tol,
            curves,
        }
        | Inputs::PlanarFaces {
            region_null,
            tol,
            curves,
        } => {
            v.extend([flag(*region_null), *tol, curves.len() as f64]);
            for c in curves {
                push_c3(&mut v, c)?;
            }
            if matches!(i, Inputs::PlanarFace { .. }) {
                Op::PlanarFace
            } else {
                Op::PlanarFaces
            }
        }
        Inputs::FacesWithCurves {
            surface,
            region_null,
            loops,
            has3d,
            curves,
        } => {
            push_surface(&mut v, surface)?;
            v.push(flag(*region_null));
            v.push(loops.len() as f64);
            v.extend(loops.iter().map(|&n| n as f64));
            v.push(flag(*has3d));
            v.push(curves.len() as f64);
            for c in curves {
                push_c3(&mut v, &c.c)?;
                v.push(f64::from(c.orient));
            }
            Op::FacesWithCurves
        }
        Inputs::MakeFaces {
            surface,
            dom,
            orient,
            region_null,
            loops,
            has3d,
            has_uv,
            curves,
            points,
        } => {
            push_surface(&mut v, surface)?;
            v.extend(dom);
            v.push(f64::from(*orient));
            v.push(flag(*region_null));
            v.push(loops.len() as f64);
            v.extend(loops.iter().map(|&n| n as f64));
            v.extend([flag(*has3d), flag(*has_uv), curves.len() as f64]);
            for c in curves {
                if *has3d {
                    let c3 = c.c3.as_ref().ok_or_else(|| harness("missing 3d curve"))?;
                    push_c3(&mut v, c3)?;
                }
                if *has_uv {
                    let uv = c.uv.as_ref().ok_or_else(|| harness("missing uv curve"))?;
                    push_curve(&mut v, uv)?;
                }
                v.push(f64::from(c.orient));
            }
            v.push(points.len() as f64);
            for p in points {
                v.extend(p);
            }
            Op::MakeFaces
        }
    };
    Ok((op, v))
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = encode(i)?;
    let out = ffi::eval(op, &input).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

// ---- Rust side --------------------------------------------------------------------------

fn rust_curve(c: &CrvIn) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let nl = NlCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    Ok(BSplineCurve::from_nurb(nl, c.dim, false))
}

fn rust_c3(c: &C3In) -> Result<SmCurve, HarnessError> {
    Ok(match c {
        C3In::Bspline(c) => SmCurve::from(rust_curve(c)?),
        C3In::Line { a, b } => SmCurve::from(Line::from_points(
            Vec3::new(a[0], a[1], a[2]),
            Vec3::new(b[0], b[1], b[2]),
            3,
        )),
    })
}

fn rust_surface(s: &SrfIn) -> Result<Surface, HarnessError> {
    match s {
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(
            Vec3::new(o[0], o[1], o[2]),
            Vec3::new(n[0], n[1], n[2]),
        ))),
        SrfIn::Bspline(b) => {
            let net = b
                .net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
                        .collect()
                })
                .collect();
            let nl = NlSurface::new(net, b.p, b.q, b.u.clone(), b.v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            *s.out_of_bounds_enabled_mut() = b.oob;
            let form = BSplineSurfaceForm::from_code(u64::from(b.form))
                .ok_or_else(|| harness("bad surface form"))?;
            s.set_b_spline_surface_form(form);
            Ok(Surface::from(s))
        }
    }
}

fn orient_of(code: u8) -> Result<OrientType, HarnessError> {
    match code {
        0 => Ok(OrientType::Unknown),
        1 => Ok(OrientType::Same),
        2 => Ok(OrientType::Opposite),
        _ => Err(harness("SmOrientType code")),
    }
}

fn continuity_of(code: u8) -> Result<ContinuityType, HarnessError> {
    use ContinuityType as C;
    Ok(match code {
        0 => C::Undefined,
        1 => C::Discontinuous,
        2 => C::C0,
        3 => C::G1,
        4 => C::G1R,
        5 => C::G1G2,
        6 => C::G1G2G3,
        7 => C::C1,
        8 => C::C1G2,
        9 => C::C1G2G3,
        10 => C::C1C2,
        11 => C::C1C2G3,
        12 => C::C1C2C3,
        13 => C::CInfinity,
        _ => return Err(harness("SmContinuityType code")),
    })
}

fn dom_of(d: &[f64; 4]) -> Extent2d {
    Extent2d::from_points(Point2::new(d[0], d[1]), Point2::new(d[2], d[3]))
}

/// The notification identity of a topology object (distinct per kind and index).
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 4, x.index()),
        EntityRef::Region(x) => (16012, 5, x.index()),
        EntityRef::Shell(x) => (16013, 6, x.index()),
        EntityRef::Faceuse(x) => (16022, 7, x.index()),
        EntityRef::Face(x) => (solidcore::topology::face::p2::TYPE, 0, x.index()),
        EntityRef::Loopuse(x) => (16004, 8, x.index()),
        EntityRef::Loop(x) => (16003, 9, x.index()),
        EntityRef::Edgeuse(x) => (16005, 2, x.index()),
        EntityRef::Edge(x) => (solidcore::topology::edge::TYPE, 1, x.index()),
        EntityRef::Vertexuse(x) => (16006, 10, x.index()),
        EntityRef::Vertex(x) => (16021, 11, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(kind << 40 | (1 + u64::from(i))),
    }
}

fn this_face(f: FaceId) -> ObjectRef {
    key_of(EntityRef::Face(f))
}

/// The `SmStatus` of a Rust result (`0` success, `SM_ERR_INVALID_INPUT` for
/// `InvalidInput`, the code of an upstream status); a deferral or another error ends the
/// case.
fn status<T>(r: &Result<T, SolidError>) -> Result<f64, Fail> {
    match r {
        Ok(_) => Ok(0.0),
        // the port's refusal of FindOuterLoops' zero-area loop: upstream reads past its
        // arrays there (plan/upstream-issues.md UB-119) and goes on with garbage counts,
        // so the case is not comparable (the shim cannot tell it beforehand)
        Err(SolidError::InvalidInput {
            operation: "topology::trimming_tools",
            ..
        }) => Err(Fail::Deferred(Box::new(Output::deferred(
            "cpp-guard:ub-119",
            "SmTrimmingTools::FindOuterLoops",
        )))),
        Err(SolidError::Upstream { code, .. }) => Ok(f64::from(*code)),
        Err(SolidError::InvalidInput { .. }) => Ok(1007.0),
        Err(e) => Err(match Output::unported(e) {
            Some(o) => Fail::Deferred(Box::new(o)),
            None => hfail(format!("rust: {e}")),
        }),
    }
}

/// The shim's curve block of the geometry digest.
fn put_curve(k: &Kernel, o: &mut Vec<f64>, c: Option<&SmCurve>) {
    let Some(c) = c else {
        o.push(-1.0);
        return;
    };
    let d = c.natural_interval();
    o.extend([c.type_code() as f64, d.min(), d.max()]);
    for i in 0..=4 {
        let t = d.min() + (d.max() - d.min()) * (f64::from(i) / 4.0);
        match c.evaluate_point(k, t) {
            Ok(p) => o.extend([1.0, p.x, p.y, p.z]),
            Err(_) => o.extend([0.0, 0.0, 0.0, 0.0]),
        }
    }
}

/// The shim's surface block of the geometry digest (samples over `dom`).
fn put_surface(k: &Kernel, o: &mut Vec<f64>, s: Option<&Surface>, dom: &Extent2d) {
    let Some(s) = s else {
        o.push(-1.0);
        return;
    };
    let n = s.natural_uv_domain();
    o.extend([
        s.type_code() as f64,
        n.min().x,
        n.min().y,
        n.max().x,
        n.max().y,
    ]);
    for i in 0..=2 {
        for j in 0..=2 {
            let uv = Point2::new(
                dom.min().x + (dom.max().x - dom.min().x) * (f64::from(i) / 2.0),
                dom.min().y + (dom.max().y - dom.min().y) * (f64::from(j) / 2.0),
            );
            match s.evaluate_point(k, uv) {
                Ok(p) => o.extend([1.0, p.x, p.y, p.z]),
                Err(_) => o.extend([0.0, 0.0, 0.0, 0.0]),
            }
        }
    }
}

fn must<T>(r: Result<T, SolidError>) -> Result<T, Fail> {
    r.map_err(|e| hfail(format!("rust: {e}")))
}

/// The B-rep after the op: the `topology.brep_p3` runner's counts and graph, then the
/// geometry in walk order.
fn put_brep(k: &Kernel, o: &mut Vec<f64>, b: &Brep) -> Result<(), Fail> {
    put_counts(o, b)?;
    let g = walk(b)?;
    for i in 0..g.len(9) {
        let v = must(b.vertex(VertexId::from_index(g.at(9, i)?)))?;
        o.extend([v.point.x, v.point.y, v.point.z, v.tolerance]);
    }
    for i in 0..g.len(7) {
        let e = must(b.edge(EdgeId::from_index(g.at(7, i)?)))?;
        o.extend([e.tolerance, e.interval.min(), e.interval.max()]);
        put_curve(k, o, e.curve.as_ref());
    }
    for i in 0..g.len(3) {
        let f = must(b.face(FaceId::from_index(g.at(3, i)?)))?;
        let d = f.uv_domain;
        o.extend([
            f.tolerance,
            d.min().x,
            d.min().y,
            d.max().x,
            d.max().y,
            f64::from(f.rectangular_trim),
        ]);
        put_surface(k, o, f.surface.as_ref(), &d);
    }
    for i in 0..g.len(6) {
        let eu = must(b.edgeuse(EdgeuseId::from_index(g.at(6, i)?)))?;
        put_curve(k, o, eu.uv_trim_curve.as_ref());
    }
    Ok(())
}

/// What an op returned: its new objects.
#[derive(Default)]
struct Made {
    region: Option<RegionId>,
    shell: Option<ShellId>,
    face: Option<FaceId>,
    faces: Vec<FaceId>,
}

fn rust_op(k: &mut Kernel, b: &mut Brep, i: &Inputs, fenv: &mut FaceBuildEnv<'_, '_>) -> Result<Result<Made, SolidError>, HarnessError> {
    let infinite = b.infinite_region();
    let region = |null: bool| if null { None } else { infinite };
    Ok(match i {
        Inputs::InRegion {
            surface,
            region_null,
            dom,
        } => {
            let s = surface.as_ref().map(rust_surface).transpose()?;
            p8::create_face_in_region_from_surface(k, b, region(*region_null), s, &dom_of(dom), fenv)
                .map(|m| Made {
                    shell: m.shell,
                    face: Some(m.face),
                    ..Made::default()
                })
        }
        Inputs::FromSurface { surface, dom } => {
            let s = surface.as_ref().map(rust_surface).transpose()?;
            p8::create_face_from_surface(k, b, s, &dom_of(dom), fenv).map(|f| Made {
                face: Some(f),
                ..Made::default()
            })
        }
        Inputs::FacesFromSurface {
            surface,
            continuity,
        } => {
            let s = surface.as_ref().map(rust_surface).transpose()?;
            let dom = s.as_ref().map_or_else(
                || Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)),
                Surface::natural_uv_domain,
            );
            p8::create_faces_from_surface(k, b, s, &dom, continuity_of(*continuity)?, fenv).map(
                |faces| Made {
                    faces,
                    ..Made::default()
                },
            )
        }
        Inputs::PlanarFace {
            region_null,
            tol,
            curves,
        } => {
            let cs = curves.iter().map(rust_c3).collect::<Result<Vec<_>, _>>()?;
            p8::create_planar_face_with_3d_curves(k, b, region(*region_null), &cs, *tol, fenv).map(
                |face| Made {
                    face,
                    ..Made::default()
                },
            )
        }
        Inputs::PlanarFaces {
            region_null,
            tol,
            curves,
        } => {
            let cs = curves.iter().map(rust_c3).collect::<Result<Vec<_>, _>>()?;
            p8::create_planar_faces_with_3d_curves(k, b, region(*region_null), &cs, *tol, fenv)
                .map(|faces| Made {
                    faces,
                    ..Made::default()
                })
        }
        Inputs::FacesWithCurves {
            surface,
            region_null,
            loops,
            has3d,
            curves,
        } => {
            let s = rust_surface(surface)?;
            let cs = curves
                .iter()
                .map(|c| rust_c3(&c.c).map(Some))
                .collect::<Result<Vec<_>, _>>()?;
            let orients = curves
                .iter()
                .map(|c| orient_of(c.orient))
                .collect::<Result<Vec<_>, _>>()?;
            p8::create_faces_with_curves(
                k,
                b,
                region(*region_null),
                loops,
                has3d.then_some(cs),
                &orients,
                Some(s),
                fenv,
            )
            .map(|m| Made {
                shell: m.shell,
                faces: m.faces,
                ..Made::default()
            })
        }
        Inputs::MakeFaces {
            surface,
            dom,
            orient,
            region_null,
            loops,
            has3d,
            has_uv,
            curves,
            points,
        } => {
            let s = rust_surface(surface)?;
            let mut c3 = Vec::new();
            let mut uv = Vec::new();
            for c in curves {
                if *has3d {
                    let x = c.c3.as_ref().ok_or_else(|| harness("missing 3d curve"))?;
                    c3.push(Some(rust_c3(x)?));
                }
                if *has_uv {
                    let x = c.uv.as_ref().ok_or_else(|| harness("missing uv curve"))?;
                    uv.push(SmCurve::from(rust_curve(x)?));
                }
            }
            let input = FaceCurvesInput {
                loops: loops.clone(),
                curves_3d: has3d.then_some(c3),
                uv_curves: has_uv.then_some(uv),
                orientations: curves
                    .iter()
                    .map(|c| orient_of(c.orient))
                    .collect::<Result<_, _>>()?,
                loop_points: points.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect(),
            };
            p8::make_faces_with_curves(
                k,
                b,
                region(*region_null),
                input,
                s,
                &dom_of(dom),
                orient_of(*orient)?,
                fenv,
            )
            .map(|m| Made {
                region: m.region,
                shell: m.shell,
                faces: m.faces,
                ..Made::default()
            })
        }
    })
}

fn rust_record(i: &Inputs) -> Result<Vec<f64>, Fail> {
    let mut k = Kernel::default();
    // `new SmBrep()` in the kernel's context
    let mut b = solidcore::topology::brep::p3::new_brep(
        &mut k,
        solidcore::geom::tol::USE_DEFAULT,
        solidcore::geom::tol::USE_DEFAULT,
    )
    .map_err(hfail)?;
    let mut records = BTreeMap::new();
    let mut attrs = AttributeStore::default();
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attrs,
        context: None,
        key_of: &key_of,
    };
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let r = rust_op(&mut k, &mut b, i, &mut fenv)?;
    let mut o = vec![status(&r)?];
    let ok = r.is_ok();
    let made = r.unwrap_or_default();
    let g = walk(&b)?;
    let idx = |k: usize, id: Option<u32>| g.find(k, id);
    let put_faces = |o: &mut Vec<f64>, faces: &[FaceId]| {
        o.push(faces.len() as f64);
        for f in faces {
            o.push(idx(3, Some(f.index())));
        }
    };
    match i {
        // (the out-parameters only after a success)
        _ if !ok => {}
        Inputs::InRegion { .. } => {
            o.push(idx(1, made.shell.map(solidcore::topology::ArenaId::index)));
            o.push(idx(3, made.face.map(solidcore::topology::ArenaId::index)));
        }
        Inputs::FromSurface { .. } | Inputs::PlanarFace { .. } => {
            o.push(idx(3, made.face.map(solidcore::topology::ArenaId::index)));
        }
        Inputs::FacesWithCurves { .. } => {
            o.push(idx(1, made.shell.map(solidcore::topology::ArenaId::index)));
            put_faces(&mut o, &made.faces);
        }
        Inputs::MakeFaces { .. } => {
            o.push(idx(0, made.region.map(solidcore::topology::ArenaId::index)));
            o.push(idx(1, made.shell.map(solidcore::topology::ArenaId::index)));
            put_faces(&mut o, &made.faces);
        }
        Inputs::FacesFromSurface { .. } | Inputs::PlanarFaces { .. } => {
            put_faces(&mut o, &made.faces);
        }
    }
    put_brep(&k, &mut o, &b)?;
    Ok(o)
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_record(i) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- cases ------------------------------------------------------------------------------

fn line3(a: [f64; 3], b: [f64; 3]) -> C3In {
    C3In::Line { a, b }
}

/// A degree-1 B-spline line of dimension `dim` from `a` to `b` (`z = NOZ` in 2D).
fn bline(dim: u64, a: [f64; 3], b: [f64; 3]) -> CrvIn {
    let (za, zb) = if dim == 2 { (NOZ, NOZ) } else { (a[2], b[2]) };
    CrvIn {
        dim,
        cpts: vec![[a[0], a[1], za, NOW], [b[0], b[1], zb, NOW]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// A quadratic B-spline arc from `a` through near `m` to `b` (3D).
fn arc(a: [f64; 3], m: [f64; 3], b: [f64; 3]) -> CrvIn {
    CrvIn {
        dim: 3,
        cpts: vec![[a[0], a[1], a[2], NOW], [m[0], m[1], m[2], NOW], [b[0], b[1], b[2], NOW]],
        p: 2,
        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    }
}

/// The closed polygon through `pts` (in order) as lines, or degree-1 B-splines.
fn polygon(pts: &[[f64; 3]], bspline: bool) -> Vec<C3In> {
    let n = pts.len();
    (0..n)
        .filter_map(|i| {
            let a = *pts.get(i)?;
            let b = *pts.get((i + 1) % n)?;
            Some(if bspline {
                C3In::Bspline(bline(3, a, b))
            } else {
                line3(a, b)
            })
        })
        .collect()
}

/// An axis-aligned rectangle `[x0, x1] x [y0, y1]` at `z`, counter-clockwise.
fn rect(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<[f64; 3]> {
    vec![[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]]
}

/// A plane `z = 0` (normal +z, or -z with `flip`).
fn plane_z(flip: bool) -> SrfIn {
    SrfIn::Plane {
        o: [0.0, 0.0, 0.0],
        n: [0.0, 0.0, if flip { -1.0 } else { 1.0 }],
    }
}

/// A bilinear-or-better B-spline patch over `[0, 1]^2` (`p`, `q` degrees, `nu`, `nv`
/// spans), heights from `h(i, j)`; `u_c0` / `v_c0` repeat the middle knot to multiplicity
/// `p` / `q` (a `C0` crease), `closed_u` makes the first and last rows equal.
#[allow(clippy::too_many_arguments)]
fn patch(
    p: usize,
    q: usize,
    nu: usize,
    nv: usize,
    scale: f64,
    u_c0: bool,
    v_c0: bool,
    h: &dyn Fn(usize, usize) -> f64,
) -> SrfIn {
    let knots = |deg: usize, spans: usize, crease: bool| {
        let mut u = vec![0.0; deg + 1];
        for s in 1..spans {
            let t = s as f64 / spans as f64;
            let mult = if crease && s == spans / 2 { deg } else { 1 };
            u.extend(std::iter::repeat_n(t, mult));
        }
        u.extend(vec![1.0; deg + 1]);
        u
    };
    let u = knots(p, nu, u_c0);
    let v = knots(q, nv, v_c0);
    let n = u.len() - p - 1;
    let m = v.len() - q - 1;
    let net = (0..n)
        .map(|i| {
            (0..m)
                .map(|j| {
                    let x = i as f64 / (n - 1) as f64;
                    let y = j as f64 / (m - 1) as f64;
                    [x * scale, y * scale, h(i, j) * scale, NOW]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net,
        p,
        q,
        u,
        v,
    })
}

fn bump(i: usize, j: usize) -> f64 {
    0.1 * ((i * 7 + j * 3) % 5) as f64 - 0.2
}

fn flat(_: usize, _: usize) -> f64 {
    0.0
}

/// A ridge: the middle row lifted (with a crease there the surface is `C0`).
fn ridge(i: usize, _: usize) -> f64 {
    if i == 2 {
        0.4
    } else {
        0.0
    }
}

/// A half-cylinder-like closed B-spline (first and last rows equal: closed in u).
fn closed_u() -> SrfIn {
    let pts = [
        [1.0, 0.0],
        [1.0, 1.0],
        [-1.0, 1.0],
        [-1.0, -1.0],
        [1.0, -1.0],
        [1.0, 0.0],
    ];
    let net = pts
        .iter()
        .map(|&[x, y]| (0..3usize).map(|j| [x, y, j as f64 * 0.5, NOW]).collect())
        .collect();
    SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net,
        p: 2,
        q: 1,
        u: vec![0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 0.5, 1.0, 1.0],
    })
}

/// A patch pinched at its `u = 0` side (a degenerate boundary).
fn pinched() -> SrfIn {
    let net = (0..3usize)
        .map(|i| {
            (0..3usize)
                .map(|j| {
                    if i == 0 {
                        [0.0, 0.0, 0.0, NOW]
                    } else {
                        [i as f64, j as f64 - 1.0, 0.1 * (i * j) as f64, NOW]
                    }
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net,
        p: 2,
        q: 2,
        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    })
}

fn natural(s: &SrfIn) -> [f64; 4] {
    match s {
        SrfIn::Plane { .. } => [-1.0, -1.0, 1.0, 1.0],
        SrfIn::Bspline(b) => [
            b.u.first().copied().unwrap_or(0.0),
            b.v.first().copied().unwrap_or(0.0),
            b.u.last().copied().unwrap_or(1.0),
            b.v.last().copied().unwrap_or(1.0),
        ],
    }
}

/// A UV loop (degree-1 B-spline lines, counter-clockwise) over `[u0, u1] x [v0, v1]`.
fn uv_rect(u0: f64, v0: f64, u1: f64, v1: f64) -> Vec<CrvIn> {
    let p = [[u0, v0, 0.0], [u1, v0, 0.0], [u1, v1, 0.0], [u0, v1, 0.0]];
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .map(|(&a, &b)| bline(2, a, b))
        .collect()
}

fn same(c: C3In) -> OrientedCurve {
    OrientedCurve { c, orient: 1 }
}

#[allow(clippy::too_many_lines)]
fn builtin() -> Vec<(String, Inputs)> {
    let mut v: Vec<(String, Inputs)> = Vec::new();
    let surfaces: Vec<(&str, SrfIn)> = vec![
        ("plane", plane_z(false)),
        ("plane_down", plane_z(true)),
        (
            "plane_tilted",
            SrfIn::Plane {
                o: [1.0, -2.0, 0.5],
                n: [0.3, -0.4, 0.8],
            },
        ),
        ("bilinear", patch(1, 1, 1, 1, 1.0, false, false, &flat)),
        ("biquad", patch(2, 2, 2, 2, 1.0, false, false, &bump)),
        ("bicubic", patch(3, 3, 3, 2, 10.0, false, false, &bump)),
        ("tiny", patch(2, 2, 2, 2, 1e-3, false, false, &bump)),
        ("huge", patch(2, 2, 2, 2, 1e4, false, false, &bump)),
        ("crease_u", patch(2, 1, 4, 1, 1.0, true, false, &ridge)),
        ("crease_v", patch(1, 2, 1, 4, 1.0, false, true, &bump)),
        ("crease_uv", patch(2, 2, 4, 4, 1.0, true, true, &bump)),
        ("closed_u", closed_u()),
        ("pinched", pinched()),
    ];

    // CreateFaceInRegionFromSurface / CreateFaceFromSurface: natural and sub-domains
    for (name, s) in &surfaces {
        let n = natural(s);
        let (du, dv) = (n[2] - n[0], n[3] - n[1]);
        let sub = [n[0] + 0.25 * du, n[1] + 0.1 * dv, n[0] + 0.75 * du, n[1] + 0.9 * dv];
        for (dn, d) in [("nat", n), ("sub", sub)] {
            for region_null in [false, true] {
                v.push((
                    format!("in_region_{name}_{dn}_{}", u8::from(region_null)),
                    Inputs::InRegion {
                        surface: Some(s.clone()),
                        region_null,
                        dom: d,
                    },
                ));
            }
            v.push((
                format!("from_surface_{name}_{dn}"),
                Inputs::FromSurface {
                    surface: Some(s.clone()),
                    dom: d,
                },
            ));
        }
        // a domain outside the natural one (SM_ERR) and a sliver
        if let SrfIn::Bspline(_) = s {
            v.push((
                format!("in_region_{name}_outside"),
                Inputs::InRegion {
                    surface: Some(s.clone()),
                    region_null: false,
                    dom: [n[0] - 0.5, n[1], n[2], n[3]],
                },
            ));
            v.push((
                format!("in_region_{name}_sliver"),
                Inputs::InRegion {
                    surface: Some(s.clone()),
                    region_null: false,
                    dom: [n[0], n[1], n[0] + 1e-7 * du, n[3]],
                },
            ));
        }
        // CreateFacesFromSurface at every continuity
        for c in [0u8, 2, 3, 7, 10, 13] {
            v.push((
                format!("faces_from_surface_{name}_{c}"),
                Inputs::FacesFromSurface {
                    surface: Some(s.clone()),
                    continuity: c,
                },
            ));
        }
    }
    v.push((
        "in_region_null".to_string(),
        Inputs::InRegion {
            surface: None,
            region_null: false,
            dom: [0.0, 0.0, 1.0, 1.0],
        },
    ));
    v.push((
        "from_surface_null".to_string(),
        Inputs::FromSurface {
            surface: None,
            dom: [0.0, 0.0, 1.0, 1.0],
        },
    ));
    v.push((
        "faces_from_surface_null".to_string(),
        Inputs::FacesFromSurface {
            surface: None,
            continuity: 2,
        },
    ));

    // CreatePlanarFace(s)With3DCurves
    let square = rect(0.0, 0.0, 2.0, 2.0, 0.0);
    let hole = rect(0.5, 0.5, 1.5, 1.5, 0.0);
    let inner = rect(0.8, 0.8, 1.2, 1.2, 0.0);
    let far = rect(5.0, 0.0, 6.0, 1.0, 0.0);
    let tilted: Vec<[f64; 3]> = vec![[0.0, 0.0, 0.0], [3.0, 0.0, 1.0], [3.0, 2.0, 1.0], [0.0, 2.0, 0.0]];
    let tri = vec![[0.0, 0.0, 5.0], [4.0, 0.0, 5.0], [1.0, 3.0, 5.0]];
    let reversed: Vec<[f64; 3]> = square.iter().rev().copied().collect();
    let mut planar_sets: Vec<(&str, Vec<C3In>)> = vec![
        ("square", polygon(&square, false)),
        ("square_bspline", polygon(&square, true)),
        ("square_cw", polygon(&reversed, false)),
        ("triangle", polygon(&tri, false)),
        ("tilted", polygon(&tilted, false)),
        ("tiny", polygon(&rect(0.0, 0.0, 1e-3, 1e-3, 0.0), false)),
        ("huge", polygon(&rect(-1e5, -1e5, 1e5, 1e5, 3.0), false)),
        ("hole", [polygon(&square, false), polygon(&hole, false)].concat()),
        ("hole_first", [polygon(&hole, false), polygon(&square, false)].concat()),
        ("two_outer", [polygon(&square, false), polygon(&far, false)].concat()),
        (
            "nested3",
            [polygon(&square, false), polygon(&hole, false), polygon(&inner, false)].concat(),
        ),
        (
            "arc_lens",
            vec![
                C3In::Bspline(arc([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0])),
                C3In::Bspline(arc([2.0, 0.0, 0.0], [1.0, -1.0, 0.0], [0.0, 0.0, 0.0])),
            ],
        ),
        (
            "shuffled",
            {
                let mut c = polygon(&square, false);
                c.swap(0, 2);
                c.swap(1, 3);
                c
            },
        ),
        ("open", polygon(&square, false).into_iter().take(3).collect()),
        (
            "nonplanar",
            polygon(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, 1.0, 0.0]], false),
        ),
        ("empty", Vec::new()),
    ];
    // a zero-length line at a corner (SmLine makes the degenerate line)
    planar_sets.push((
        "degenerate_edge",
        [
            polygon(&square, false),
            vec![line3([0.0, 0.0, 0.0], [0.0, 0.0, 0.0])],
        ]
        .concat(),
    ));
    // a gap within / beyond tolerance
    let mut gap = polygon(&square, false);
    if let Some(C3In::Line { a, .. }) = gap.get_mut(0) {
        a[0] += 1e-6;
    }
    planar_sets.push(("gap", gap));
    for (name, curves) in &planar_sets {
        for tol in [1e-6, 1e-3] {
            for region_null in [false, true] {
                v.push((
                    format!("planar_face_{name}_{tol:e}_{}", u8::from(region_null)),
                    Inputs::PlanarFace {
                        region_null,
                        tol,
                        curves: curves.clone(),
                    },
                ));
            }
            v.push((
                format!("planar_faces_{name}_{tol:e}"),
                Inputs::PlanarFaces {
                    region_null: false,
                    tol,
                    curves: curves.clone(),
                },
            ));
        }
    }

    // CreateFacesWithCurves on a plane: one loop, a loop and its hole, a null array
    let sq = polygon(&rect(-0.5, -0.5, 0.5, 0.5, 0.0), false);
    let ho = polygon(&rect(-0.2, -0.2, 0.2, 0.2, 0.0), false);
    v.push((
        "faces_with_curves_square".to_string(),
        Inputs::FacesWithCurves {
            surface: plane_z(false),
            region_null: false,
            loops: vec![4],
            has3d: true,
            curves: sq.iter().cloned().map(same).collect(),
        },
    ));
    v.push((
        "faces_with_curves_hole".to_string(),
        Inputs::FacesWithCurves {
            surface: plane_z(false),
            region_null: true,
            loops: vec![4, 4],
            has3d: true,
            curves: sq.iter().chain(ho.iter()).cloned().map(same).collect(),
        },
    ));
    v.push((
        "faces_with_curves_null".to_string(),
        Inputs::FacesWithCurves {
            surface: plane_z(false),
            region_null: false,
            loops: vec![4],
            has3d: false,
            curves: Vec::new(),
        },
    ));

    // MakeFacesWithCurves: smooth surfaces (MakeFaceWithCurves) and creased ones (split)
    let plane_loop: Vec<LoopCurve> = polygon(&rect(-0.5, -0.5, 0.5, 0.5, 0.0), false)
        .into_iter()
        .map(|c| LoopCurve {
            c3: Some(c),
            uv: None,
            orient: 1,
        })
        .collect();
    for (name, s, orient) in [("plane", plane_z(false), 1u8), ("plane_opp", plane_z(false), 2)] {
        v.push((
            format!("make_faces_3d_{name}"),
            Inputs::MakeFaces {
                surface: s,
                dom: [-1.0, -1.0, 1.0, 1.0],
                orient,
                region_null: false,
                loops: vec![4],
                has3d: true,
                has_uv: false,
                curves: plane_loop.clone(),
                points: Vec::new(),
            },
        ));
    }
    for (name, s) in [
        ("biquad", patch(2, 2, 2, 2, 1.0, false, false, &bump)),
        ("crease_u", patch(2, 1, 4, 1, 1.0, true, false, &ridge)),
        ("crease_v", patch(1, 2, 1, 4, 1.0, false, true, &bump)),
        ("crease_uv", patch(2, 2, 4, 4, 1.0, true, true, &bump)),
    ] {
        for (ln, l) in [
            ("full", uv_rect(0.0, 0.0, 1.0, 1.0)),
            ("inner", uv_rect(0.1, 0.2, 0.9, 0.8)),
            ("left", uv_rect(0.1, 0.1, 0.4, 0.9)),
        ] {
            for points in [Vec::new(), vec![[0.3, 0.5, 0.0]]] {
                v.push((
                    format!("make_faces_uv_{name}_{ln}_{}", points.len()),
                    Inputs::MakeFaces {
                        surface: s.clone(),
                        dom: [0.0, 0.0, 1.0, 1.0],
                        orient: 1,
                        region_null: false,
                        loops: vec![4],
                        has3d: false,
                        has_uv: true,
                        curves: l
                            .iter()
                            .map(|c| LoopCurve {
                                c3: None,
                                uv: Some(c.clone()),
                                orient: 1,
                            })
                            .collect(),
                        points,
                    },
                ));
            }
        }
        // a hole inside the outer loop
        let outer = uv_rect(0.05, 0.05, 0.95, 0.95);
        let hole: Vec<CrvIn> = uv_rect(0.3, 0.3, 0.7, 0.7).into_iter().rev().collect();
        v.push((
            format!("make_faces_uv_{name}_hole"),
            Inputs::MakeFaces {
                surface: s.clone(),
                dom: [0.0, 0.0, 1.0, 1.0],
                orient: 1,
                region_null: false,
                loops: vec![4, 4],
                has3d: false,
                has_uv: true,
                curves: outer
                    .iter()
                    .map(|c| (c, 1u8))
                    .chain(hole.iter().map(|c| (c, 2u8)))
                    .map(|(c, orient)| LoopCurve {
                        c3: None,
                        uv: Some(c.clone()),
                        orient,
                    })
                    .collect(),
                points: Vec::new(),
            },
        ));
    }
    // no curve arrays (MakeFaceWithCurves' input check; SmCurveLoops::IsValid when split)
    for (name, s) in [
        ("plane", plane_z(false)),
        ("crease_u", patch(2, 1, 4, 1, 1.0, true, false, &ridge)),
    ] {
        v.push((
            format!("make_faces_no_curves_{name}"),
            Inputs::MakeFaces {
                surface: s,
                dom: [0.0, 0.0, 1.0, 1.0],
                orient: 1,
                region_null: false,
                loops: vec![4],
                has3d: false,
                has_uv: false,
                curves: Vec::new(),
                points: Vec::new(),
            },
        ));
    }
    // a creased plane-like ridge with a 3D loop crossing the crease (lines on the surface)
    let ridge_s = patch(2, 1, 4, 1, 1.0, true, false, &flat);
    v.push((
        "make_faces_3d_crease".to_string(),
        Inputs::MakeFaces {
            surface: ridge_s,
            dom: [0.0, 0.0, 1.0, 1.0],
            orient: 1,
            region_null: false,
            loops: vec![4],
            has3d: true,
            has_uv: false,
            curves: polygon(&rect(0.2, 0.2, 0.8, 0.8, 0.0), false)
                .into_iter()
                .map(|c| LoopCurve {
                    c3: Some(c),
                    uv: None,
                    orient: 1,
                })
                .collect(),
            points: Vec::new(),
        },
    ));
    v
}

// ---- generation -------------------------------------------------------------------------

fn gen_patch(rng: &mut Rng) -> SrfIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let nu = 1 + rng.below(4) as usize;
    let nv = 1 + rng.below(4) as usize;
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    let u_c0 = nu >= 2 && rng.chance(0.3);
    let v_c0 = nv >= 2 && !u_c0 && rng.chance(0.3);
    let seed = rng.below(1000) as usize;
    patch(p, q, nu, nv, scale, u_c0, v_c0, &move |i, j| {
        0.1 * (((i * 7 + j * 3 + seed) % 5) as f64) - 0.2
    })
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let s = if rng.chance(0.2) {
        let mut n = rng.point3(-1.0, 1.0);
        if n.iter().all(|c| c.abs() < 0.1) {
            n = [0.0, 0.0, 1.0];
        }
        SrfIn::Plane {
            o: rng.point3(-2.0, 2.0),
            n,
        }
    } else {
        gen_patch(rng)
    };
    let n = natural(&s);
    match rng.below(6) {
        0 | 1 => {
            let (du, dv) = (n[2] - n[0], n[3] - n[1]);
            let a = rng.uniform(0.0, 0.4);
            let b = rng.uniform(0.6, 1.0);
            let dom = if rng.chance(0.5) {
                n
            } else {
                [n[0] + a * du, n[1] + a * dv, n[0] + b * du, n[1] + b * dv]
            };
            Inputs::InRegion {
                surface: Some(s),
                region_null: rng.chance(0.3),
                dom,
            }
        }
        2 => Inputs::FacesFromSurface {
            surface: Some(s),
            continuity: *rng.choose(&[0u8, 2, 3, 7, 10, 13]).unwrap_or(&2),
        },
        3 | 4 => {
            let scale = *rng.choose(&[1e-3, 1.0, 1e3]).unwrap_or(&1.0);
            let k = 3 + rng.below(4) as usize;
            let c = rng.point3(-1.0, 1.0).map(|x| x * scale);
            let pts: Vec<[f64; 3]> = (0..k)
                .map(|i| {
                    let t = std::f64::consts::TAU * i as f64 / k as f64;
                    let r = scale * rng.uniform(0.5, 1.5);
                    [c[0] + r * t.cos(), c[1] + r * t.sin(), c[2]]
                })
                .collect();
            let curves = polygon(&pts, rng.chance(0.3));
            if rng.chance(0.5) {
                Inputs::PlanarFace {
                    region_null: rng.chance(0.3),
                    tol: *rng.choose(&[1e-6, 1e-4, 1e-3]).unwrap_or(&1e-6),
                    curves,
                }
            } else {
                Inputs::PlanarFaces {
                    region_null: false,
                    tol: 1e-6,
                    curves,
                }
            }
        }
        _ => {
            // UV loop inside the domain
            let (du, dv) = (n[2] - n[0], n[3] - n[1]);
            let a = rng.uniform(0.0, 0.3);
            let b = rng.uniform(0.7, 1.0);
            let l = uv_rect(n[0] + a * du, n[1] + a * dv, n[0] + b * du, n[1] + b * dv);
            match s {
                SrfIn::Bspline(_) => Inputs::MakeFaces {
                    surface: s,
                    dom: n,
                    orient: if rng.chance(0.8) { 1 } else { 2 },
                    region_null: false,
                    loops: vec![4],
                    has3d: false,
                    has_uv: true,
                    curves: l
                        .into_iter()
                        .map(|c| LoopCurve {
                            c3: None,
                            uv: Some(c),
                            orient: 1,
                        })
                        .collect(),
                    points: Vec::new(),
                },
                SrfIn::Plane { .. } => Inputs::FromSurface {
                    surface: Some(s),
                    dom: [-a, -a, b, b],
                },
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        UNIT
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = default_cases(self, ctx)?;
        for (index, (name, i)) in builtin().into_iter().enumerate() {
            v.push(Case {
                unit: UNIT.to_string(),
                name,
                source: crate::CaseSource::Fixture {
                    path: "built-in: topology.brep_p8 cases".to_string(),
                    index,
                    upstream_ref: Some(
                        "SM_API_test TestSmCreatePlane / TestSmCreatePlanarFaces / TestSmCreatePlanarCircle (face builders on planes and B-spline patches)"
                            .to_string(),
                    ),
                },
                inputs: serde_json::to_value(&i).map_err(|e| harness(e.to_string()))?,
            });
        }
        Ok(v)
    }
}
