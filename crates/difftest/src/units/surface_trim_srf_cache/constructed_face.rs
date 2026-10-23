//! The existing synthetic face protocol over independently constructed topology.
//!
//! Mirrors `op_face` in `harness/cpp/shim/surface_trim_srf_cache.cpp`. Cache setup
//! follows the native-face probes, but no exported native topology or results are
//! needed: the landed p8 constructor consumes the original B-spline input.

use std::collections::BTreeMap;

use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::intersect::point_class::PointClassType;
use solidcore::query::face::p2::BrepCacheFace;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::{p3, p8};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology_traverser::Marks;
use solidcore::EntityRef;

use super::{containment_code, flag, push_dom, BSplineSurface, Kernel, Point2, PtIn, Run};
use super::{SolidError, Surface, SurfaceCacheOptions, TrimSrfCache};

fn operation_status<T>(result: &solidcore::Result<T>) -> solidcore::Result<f64> {
    match result {
        Ok(_) => Ok(0.0),
        Err(error) if error.is_unported() => Err(error.clone()),
        Err(error) => Ok(super::status(error)),
    }
}

fn object_kind(object: Option<EntityRef>) -> f64 {
    // The native protocol compares GetType(), not pointer values or arena ids.
    object.map_or(-1.0, |entity| {
        super::super::topology_brep_p8::key_of(entity).type_code as f64
    })
}

pub(super) fn run(k: &mut Kernel, surface: BSplineSurface, build: bool, points: &[PtIn]) -> Run {
    let mut brep = p3::new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    let mut records = BTreeMap::new();
    let mut attributes = AttributeStore::default();
    let key_of = super::super::topology_brep_p8::key_of;
    let this_face = |face| key_of(EntityRef::Face(face));
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let mut marks = Marks::default();
    let mut fenv = p8::FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let domain = surface.natural_uv_domain();
    let made = p8::create_face_from_surface(
        k,
        &mut brep,
        Some(Surface::from(surface)),
        &domain,
        &mut fenv,
    );
    let mut out = vec![operation_status(&made)?];
    let Ok(face) = made else {
        return Ok(out);
    };
    let record = brep.face(face)?;
    let source = record
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::null_pointer("difftest::constructed_face"))?;
    // Retain exactly the constructed face's geometry, owner and tolerance, as the
    // native-face lane does. A normal Clone can fix B-spline knots a second time.
    let SurfaceKind::BSpline(source_bss) = source.kind() else {
        return Err(SolidError::invalid_input(
            "difftest::constructed_face",
            "B-spline face constructor returned a different surface kind",
        ));
    };
    let mut snapshot = Surface::from(source_bss.clone_exact());
    snapshot.set_owner(source.owner());
    snapshot.set_owner_zone_tol_3d(source.owner_zone_tol_3d());
    let mut cache: TrimSrfCache<EntityRef> =
        TrimSrfCache::new(snapshot, SurfaceCacheOptions::default());
    cache.bind_face(this_face(face), EntityRef::Face(face), record.tolerance)?;
    let mut context = BrepCacheFace {
        b: &mut brep,
        f: face,
    };
    if build {
        let built = cache.build_tree_with_face(k, &mut context);
        out.push(operation_status(&built)?);
        let base = cache.base();
        out.extend([
            flag(base.process_boundary_curves),
            flag(base.point_test_enabled),
            flag(base.have_t_surface_cache),
            containment_code(base.face_containment_done),
        ]);
        push_dom(&mut out, &base.uv_domain());
        out.push(base.tree().map_or(0, |tree| tree.nodes().len()) as f64);
    }
    // Do not force a tree build when build=false. These mutable face services
    // permit only the trim preparation/classification reached by each query, and
    // retain its writes on the same B-rep, just like the native calls.
    for point in points {
        let uv = Point2::new(point.uv[0], point.uv[1]);
        let tested = cache.point_test_in(k, &mut context, uv, point.test_tol.then_some(point.tol));
        out.push(operation_status(&tested)?);
        let (ok, object) = tested.unwrap_or((false, None));
        out.extend([flag(ok), object_kind(object)]);
        let classified = cache.point_classify_in(k, &mut context, uv, point.tol);
        out.push(operation_status(&classified)?);
        let (class, object) = classified.unwrap_or((PointClassType::Unknown.code(), None));
        out.extend([f64::from(class), object_kind(object)]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::{BssIn, InlineSrf, Inputs, OpIn, SrfIn};
    use super::*;

    fn input(build: bool) -> Inputs {
        let now = solidcore::nurbs::math::NOW;
        Inputs {
            srf: BssIn {
                oob: true,
                form: super::super::default_form(),
                srf: SrfIn::Inline(InlineSrf {
                    net: vec![
                        vec![[0.0, 0.0, 0.0, now], [0.0, 1.0, 0.0, now]],
                        vec![[1.0, 0.0, 0.0, now], [1.0, 1.0, 0.25, now]],
                    ],
                    p: 1,
                    q: 1,
                    u: vec![0.0, 0.0, 1.0, 1.0],
                    v: vec![0.0, 0.0, 1.0, 1.0],
                }),
            },
            op: OpIn::Face {
                build,
                points: [[0.5, 0.5], [-0.1, 0.5], [0.0, 0.5], [0.0, 0.0]]
                    .into_iter()
                    .enumerate()
                    .map(|(index, uv)| PtIn {
                        uv,
                        test_tol: index % 2 == 0,
                        tol: 1e-6,
                    })
                    .collect(),
            },
        }
    }

    #[test]
    fn face_queries_compare_real_inside_outside_edge_and_vertex_results() {
        for build in [false, true] {
            let input = input(build);
            let rust = super::super::rust(&input).unwrap();
            assert!(rust.deferred.is_none(), "{rust:?}");
            assert_eq!(rust.values["status"], [0.0]);
            let out = &rust.values["out"];
            let queries = if build {
                assert_eq!(out[0], 0.0, "tree build failed");
                assert_eq!(&out[1..5], &[0.0, 1.0, 1.0, 1.0]);
                assert!(out[9] > 0.0, "tree has no nodes");
                &out[10..]
            } else {
                out.as_slice()
            };
            assert_eq!(queries.len(), 24);
            let classes: Vec<_> = queries.as_chunks::<6>().0.iter().map(|q| q[4]).collect();
            assert_eq!(
                classes,
                [
                    f64::from(PointClassType::Face.code()),
                    f64::from(PointClassType::Unknown.code()),
                    f64::from(PointClassType::Edge.code()),
                    f64::from(PointClassType::Vertex.code()),
                ]
            );
            #[cfg(feature = "cpp")]
            {
                let cpp = super::super::cpp(&input).unwrap();
                assert_eq!(cpp, rust, "native record differs with build={build}");
            }
        }
    }
}
