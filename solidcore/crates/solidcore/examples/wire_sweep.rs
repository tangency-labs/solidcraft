//! Machine-readable production point-to-edge sweep, used by the local C++ comparison.
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::sweep::topology_sweep::{
    access::SweepAccess, engine::SweepTargets, geometry::SweepGeometry, store::SweepStore,
    SweepEntity, TopologySweep, TopologySweepOptions,
};
use solidcore::sweep::trans_sweep_geometry::TranslationalSweepGeometry;
use solidcore::topology::{edgeuse::p1 as eu, EdgeuseOwner};
use solidcore::{Result, SolidError};

fn arg(a: &[f64], i: usize) -> Result<f64> {
    a.get(i)
        .copied()
        .filter(|x| x.is_finite())
        .ok_or_else(|| SolidError::invalid_input("wire_sweep", "missing or nonfinite argument"))
}
fn main() -> Result<()> {
    let args = std::env::args()
        .skip(1)
        .map(|s| {
            s.parse::<f64>()
                .map_err(|_| SolidError::invalid_input("wire_sweep", "invalid number"))
        })
        .collect::<Result<Vec<_>>>()?;
    if args.len() != 11 {
        return Err(SolidError::invalid_input("wire_sweep", "expected point(3), vector(3), tolerance, shared, source flag, target flag, repetitions"));
    }
    let p = Vec3::new(arg(&args, 0)?, arg(&args, 1)?, arg(&args, 2)?);
    let delta = Vec3::new(arg(&args, 3)?, arg(&args, 4)?, arg(&args, 5)?);
    let tol = arg(&args, 6)?;
    if tol <= 0.0 || !(0.0..=4096.0).contains(&arg(&args, 10)?) || arg(&args, 10)?.fract() != 0.0 {
        return Err(SolidError::invalid_input(
            "wire_sweep",
            "positive tolerance and integer repetitions 0..4096 required",
        ));
    }
    let shared = arg(&args, 7)? != 0.;
    let mut store = SweepStore::owned();
    let c1 = store.insert_kernel(Kernel::default())?;
    let c2 = if shared {
        c1
    } else {
        store.insert_kernel(Kernel::default())?
    };
    let source = store.create_brep(c1)?;
    let target = store.create_brep(c2)?;
    store.brep_mut(source)?.set_tolerance(tol);
    store.brep_mut(target)?.set_tolerance(tol);
    let sr = store.infinite_region(source)?;
    let tr = store.infinite_region(target)?;
    let (_, original) = store.make_shell_vertex((source, sr), p)?;
    store
        .kernel_mut(source)?
        .context
        .set_doing_boolean(arg(&args, 8)? != 0.);
    store
        .kernel_mut(target)?
        .context
        .set_doing_boolean(arg(&args, 9)? != 0.);
    let mut targets = SweepTargets {
        vertices_higher: Some(vec![Some(original)]),
        ..Default::default()
    };
    let mut sweep = TopologySweep::new(TopologySweepOptions {
        stitching: false,
        ..Default::default()
    });
    sweep.repetitions = arg(&args, 10)? as u64;
    let result = sweep.do_sweep(
        &mut store,
        &SweepGeometry::Translation(TranslationalSweepGeometry::new(delta)),
        Some(source),
        Some((target, tr)),
        &mut targets,
    );
    let status = match &result {
        Ok(()) => 0,
        Err(SolidError::Upstream { code, .. }) => i64::from(*code),
        Err(e) => return Err(e.clone()),
    };
    let b = store.brep(target)?;
    let vertices: Vec<_> = b.vertices().ids().collect();
    let edges: Vec<_> = b.edges().ids().collect();
    let shells: Vec<_> = b.shells().ids().collect();
    let mut ints = vec![
        status,
        store.brep(source)?.vertices().len() as i64,
        store.brep(source)?.edges().len() as i64,
        b.regions().len() as i64,
        shells.len() as i64,
        b.faces().len() as i64,
        edges.len() as i64,
        vertices.len() as i64,
        edges
            .iter()
            .map(|e| b.edge(*e).map(|e| e.edgeuses.len()))
            .collect::<Result<Vec<_>>>()?
            .iter()
            .sum::<usize>() as i64,
        vertices
            .iter()
            .map(|v| b.vertex(*v).map(|v| v.vertexuses.len()))
            .collect::<Result<Vec<_>>>()?
            .iter()
            .sum::<usize>() as i64,
        sweep.to_same.len() as i64,
        sweep.to_higher.len() as i64,
        i64::from(store.kernel(source)?.context.doing_boolean()),
        i64::from(store.kernel(target)?.context.doing_boolean()),
    ];
    let source_point = store.brep(source)?.vertex(original.1)?.point;
    let mut values = vec![source_point.x, source_point.y, source_point.z];
    for &v in &vertices {
        let v = b.vertex(v)?;
        ints.push(v.vertexuses.len() as i64);
        values.extend([v.point.x, v.point.y, v.point.z, v.tolerance]);
    }
    for &s in &shells {
        let shell = b.shell(s)?;
        ints.extend([
            i64::from(shell.is_wire_shell()),
            i64::from(shell.region == Some(tr)),
        ]);
    }
    for &e in &edges {
        let edge = b.edge(e)?;
        for v in edge.vertices {
            ints.push(
                vertices
                    .iter()
                    .position(|x| *x == v)
                    .map_or(-1, |i| i as i64),
            );
        }
        ints.push(edge.edgeuses.len() as i64);
        let curve = edge
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer("wire_sweep"))?;
        ints.push(curve.type_code());
        values.extend([edge.interval.min(), edge.interval.max(), edge.tolerance]);
        for f in [0., 0.25, 0.5, 0.75, 1.] {
            let q = curve.evaluate_point(
                store.kernel(target)?,
                edge.interval.min() + (edge.interval.max() - edge.interval.min()) * f,
            )?;
            values.extend([q.x, q.y, q.z]);
        }
        for &u in &edge.edgeuses {
            let use_ = b.edgeuse(u)?;
            let v = use_.vertexuse.and_then(|vu| b.vertexuse(vu).ok()?.vertex);
            let shell = match use_.owner {
                EdgeuseOwner::Shell(s) => s,
                _ => None,
            };
            ints.extend([
                eu::orientation(b, u)? as i64,
                eu::mate(b, u)?
                    .and_then(|m| edge.edgeuses.iter().position(|x| *x == m))
                    .map_or(-1, |i| i as i64),
                eu::radial(b, u)?
                    .and_then(|m| edge.edgeuses.iter().position(|x| *x == m))
                    .map_or(-1, |i| i as i64),
                v.and_then(|v| vertices.iter().position(|x| *x == v))
                    .map_or(-1, |i| i as i64),
                shell
                    .and_then(|s| shells.iter().position(|x| *x == s))
                    .map_or(-1, |i| i as i64),
            ]);
        }
    }
    let original = SweepEntity::Vertex(original.0, original.1);
    let same = sweep.get_to_same(Some(original));
    let higher = sweep.get_to_higher(Some(original));
    ints.extend([
        same.and_then(|x| {
            if let SweepEntity::Vertex(_, v) = x {
                vertices.iter().position(|x| *x == v)
            } else {
                None
            }
        })
        .map_or(-1, |i| i as i64),
        higher
            .and_then(|x| {
                if let SweepEntity::Edge(_, e) = x {
                    edges.iter().position(|x| *x == e)
                } else {
                    None
                }
            })
            .map_or(-1, |i| i as i64),
        i64::from(same.is_some_and(|x| sweep.get_from_same(Some(x)) == Some(original))),
        i64::from(higher.is_some_and(|x| sweep.get_from_lower(Some(x)) == Some(original))),
    ]);
    println!("{{\"ints\":{ints:?},\"values\":{values:?}}}");
    if !store.take_cleanup_errors().is_empty() {
        return Err(SolidError::invalid_input("wire_sweep", "cleanup errors"));
    }
    Ok(())
}
