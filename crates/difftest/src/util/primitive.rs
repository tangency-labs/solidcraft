//! Geometry and trim records shared by the primitive API and p1 runners.

use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Vec3};
use solidcore::topology::brep::p1;
use solidcore::topology::Brep;
use solidcore::{Result, SolidError};

fn point(out: &mut Vec<f64>, value: Vec3) {
    out.extend([value.x, value.y, value.z]);
}

fn sample(out: &mut Vec<f64>, value: Result<Vec3>) -> Result<()> {
    match value {
        Ok(p) => {
            out.push(0.0);
            point(out, p);
        }
        Err(e) if e.is_unported() => return Err(e),
        Err(SolidError::Upstream { code, .. }) => out.push(f64::from(code)),
        Err(_) => out.push(-1.0),
    }
    Ok(())
}

/// Appends reachable vertex positions/tolerances, edge samples and trim counts,
/// then face domains/tolerances and surface samples, in native owning-list order.
/// Mirrors `harness/cpp/shim/support/primitive_record.h`.
pub fn append(k: &Kernel, brep: &Brep, out: &mut Vec<f64>) -> Result<()> {
    for id in p1::vertices(brep, None)? {
        let vertex = brep.vertex(id)?;
        point(out, vertex.point);
        out.push(vertex.tolerance);
    }
    for id in p1::edges(brep, None)? {
        let edge = brep.edge(id)?;
        out.extend([edge.tolerance, edge.interval.min(), edge.interval.max()]);
        out.push(f64::from(edge.curve.is_some()));
        if let Some(curve) = &edge.curve {
            for fraction in [0.17, 0.37, 0.83] {
                sample(
                    out,
                    curve.evaluate_point(k, edge.interval.evaluate(fraction)),
                )?;
            }
        }
        out.push(edge.edgeuses.len() as f64);
        let mut trims = 0;
        for &eu in &edge.edgeuses {
            trims += usize::from(brep.edgeuse(eu)?.uv_trim_curve.is_some());
        }
        out.push(trims as f64);
    }
    for id in p1::faces(brep, None)? {
        let face = brep.face(id)?;
        let domain = face.uv_domain;
        out.extend([
            face.tolerance,
            domain.min().x,
            domain.min().y,
            domain.max().x,
            domain.max().y,
        ]);
        out.push(f64::from(face.surface.is_some()));
        if let Some(surface) = &face.surface {
            for uv in [
                Point2::new(0.17, 0.37),
                Point2::new(0.37, 0.61),
                Point2::new(0.83, 0.23),
            ] {
                sample(out, surface.evaluate_point(k, domain.evaluate(uv.x, uv.y)))?;
            }
        }
    }
    Ok(())
}

/// Fires a miss, a side hit and an apex hit on a constructed pyramid, preserving
/// the lazily created UV trims between rays, as the native record does.
pub fn append_pyramid_rays(k: &mut Kernel, brep: &mut Brep, out: &mut Vec<f64>) -> Result<()> {
    use solidcore::geom::extent3d::Extent3d;
    use solidcore::intersect::point_class::PointClassification;
    use solidcore::query::brep::{BrepIdentity, BrepQuery};
    use solidcore::topology::shell::{ray_fire, ShellRayInput, ShellRayState};
    use solidcore::topology::BrepId;
    let mut bounds = Extent3d::new();
    for id in p1::vertices(brep, None)? {
        bounds.add_point(brep.vertex(id)?.point);
    }
    let size = bounds.max() - bounds.min();
    let mid = bounds.mid();
    let mut notifications = solidcore::topology::brep::p2::ArenaNotifyState::new(brep);
    let env = notifications.env(None);
    let keys = |object| match object {
        solidcore::topology::brep_cache::CacheObject::Edge(e)
        | solidcore::topology::brep_cache::CacheObject::Curve(e) => {
            (env.key_of)(solidcore::EntityRef::Edge(e))
        }
        solidcore::topology::brep_cache::CacheObject::Face(f)
        | solidcore::topology::brep_cache::CacheObject::Surface(f) => {
            (env.key_of)(solidcore::EntityRef::Face(f))
        }
        solidcore::topology::brep_cache::CacheObject::Vertex(v) => {
            (env.key_of)(solidcore::EntityRef::Vertex(v))
        }
        other => solidcore::topology::brep_cache::arena_identity(other),
    };
    let id = BrepId::from_index(0);
    let world = BrepQuery::new(BrepIdentity {
        brep_id: id,
        keys: &keys,
        owner_of: &|_| None,
    });
    let shells = p1::shells(k, brep, None)?;
    let Some(&shell) = shells.first() else {
        return Ok(());
    };
    for (start, direction) in [
        (bounds.max() + size * 1000.0, Vec3::X),
        (
            Vec3::new(mid.x, mid.y, bounds.min().z + size.z * 0.25),
            Vec3::X,
        ),
        (Vec3::new(mid.x, mid.y, bounds.max().z + size.z), -Vec3::Z),
    ] {
        let attempt = ray_fire(
            k,
            brep,
            id,
            shell,
            &world,
            ShellRayInput {
                start,
                direction,
                zone_tolerance: 1e-5,
                sphere_bound: None,
                state: ShellRayState {
                    region: None,
                    classification: PointClassification::new(1e-5),
                },
            },
        );
        let status = match attempt.status {
            Ok(()) => 0.0,
            Err(e) if e.is_unported() => return Err(e),
            Err(SolidError::Upstream { code, .. }) => f64::from(code),
            Err(e) => return Err(e),
        };
        let mut trims = 0;
        for edge in p1::edges(brep, None)? {
            for &eu in &brep.edge(edge)?.edgeuses {
                trims += usize::from(brep.edgeuse(eu)?.uv_trim_curve.is_some());
            }
        }
        out.extend([
            status,
            f64::from(attempt.state.region.is_some()),
            f64::from(attempt.state.classification.point_class().code()),
            trims as f64,
        ]);
    }
    Ok(())
}
