//! Reproducible #431 scaling probe. Builds disjoint planar sheet faces with full
//! face/loop/edge/vertex-use topology, then times only the validation calls.
//! Run: `cargo run --profile difftest -p solidcore --example brep_validation_scaling -- 1000 8000 50002`
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::assert_array::AssertArray;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::validation::{contains_coincident_topology, validate_pointers};
use solidcore::topology::shell::{ShellMembers, ShellType};
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    LoopId, Loopuse, LoopuseId, LoopuseStart, Shell, Vertex, VertexId, Vertexuse, VertexuseId,
    VertexuseOwner,
};
use solidcore::{EntityRef, Kernel, Point2, Result, Vec3};
use std::time::Instant;

fn key(e: EntityRef) -> ObjectRef {
    let (t, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code: t,
        key: ObjectKey(1 + (kind << 32) + u64::from(i)),
    }
}
fn sheets(count: u32) -> Result<Brep> {
    let mut b = Brep::new_with_infinite_region()?;
    b.set_tolerance(1e-6);
    let region = b
        .infinite_region()
        .ok_or_else(|| solidcore::SolidError::invalid_input("scaling", "infinite region"))?;
    let shell = b.add_shell_record(Shell {
        region: Some(region),
        shell_type: ShellType::Faceuse,
        ..Shell::default()
    })?;
    b.region_mut(region)?.shells.push(shell);
    let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
    for i in 0..count {
        let f = FaceId::from_index(i);
        let l = LoopId::from_index(i);
        let fu0 = FaceuseId::from_index(i * 2);
        let fu1 = FaceuseId::from_index(i * 2 + 1);
        let lu0 = LoopuseId::from_index(i * 2);
        let lu1 = LoopuseId::from_index(i * 2 + 1);
        let x = f64::from(i) * 4.0;
        let points = [
            Vec3::new(x, 0.0, 0.0),
            Vec3::new(x + 1.0, 0.0, 0.0),
            Vec3::new(x + 1.0, 1.0, 0.0),
            Vec3::new(x, 1.0, 0.0),
        ];
        let mut surface = Surface::from(Plane::new(
            points[0],
            Vec3::X,
            Vec3::Y,
            Point2::new(1.0, 1.0),
            &domain,
        ));
        surface.set_owner(Some(key(EntityRef::Face(f))));
        b.add_face_record(Face {
            surface: Some(surface),
            faceuse: Some(fu0),
            uv_domain: domain,
            tolerance: 1e-6,
            ..Face::new(shell)
        })?;
        b.sync_face_surface_tolerance(f)?;
        b.add_faceuse_record(Faceuse {
            shell: Some(shell),
            face: Some(f),
            mate: Some(fu1),
            orientation: OrientType::Same,
            loopuses: vec![lu0],
            ..Faceuse::default()
        })?;
        b.add_faceuse_record(Faceuse {
            shell: Some(shell),
            face: Some(f),
            mate: Some(fu0),
            orientation: OrientType::Opposite,
            loopuses: vec![lu1],
            ..Faceuse::default()
        })?;
        b.add_loop_record(Loop {
            face: f,
            edges: Vec::new(),
            loopuse: Some(lu0),
        })?;
        b.add_loopuse_record(Loopuse {
            faceuse: Some(fu0),
            loop_: Some(l),
            mate: Some(lu1),
            start: LoopuseStart::Edgeuse(Some(EdgeuseId::from_index(i * 8))),
            orientation: OrientType::Same,
        })?;
        b.add_loopuse_record(Loopuse {
            faceuse: Some(fu1),
            loop_: Some(l),
            mate: Some(lu0),
            start: LoopuseStart::Edgeuse(Some(EdgeuseId::from_index(i * 8 + 1))),
            orientation: OrientType::Same,
        })?;
        b.face_mut(f)?.loops.push(l);
        for j in 0..4_u32 {
            let v = VertexId::from_index(i * 4 + j);
            let next = VertexId::from_index(i * 4 + (j + 1) % 4);
            let e = EdgeId::from_index(i * 4 + j);
            let eu0 = EdgeuseId::from_index(i * 8 + j * 2);
            let eu1 = EdgeuseId::from_index(i * 8 + j * 2 + 1);
            let vu0 = VertexuseId::from_index(i * 8 + j * 2);
            let vu1 = VertexuseId::from_index(i * 8 + j * 2 + 1);
            let p = *points
                .get(j as usize)
                .ok_or_else(|| solidcore::SolidError::invalid_input("scaling", "point"))?;
            let q = *points
                .get(((j + 1) % 4) as usize)
                .ok_or_else(|| solidcore::SolidError::invalid_input("scaling", "point"))?;
            b.add_vertex_record(Vertex {
                point: p,
                tolerance: 1e-6,
                vertexuses: vec![vu0, VertexuseId::from_index(i * 8 + ((j + 3) % 4) * 2 + 1)],
                poly_vertex: None,
            })?;
            let mut curve = Curve::from(Line::from_points(p, q, 3));
            curve.set_owner(Some(key(EntityRef::Edge(e))), None);
            b.add_edge_record(Edge {
                vertices: [v, next],
                edgeuses: vec![eu0, eu1],
                interval: Extent1d::from_min_max(0.0, 1.0),
                tolerance: 1e-6,
                curve: Some(curve),
            })?;
            b.add_edgeuse_record(Edgeuse {
                edge: Some(e),
                owner: EdgeuseOwner::Loopuse(Some(lu0)),
                ccw: Some(EdgeuseId::from_index(i * 8 + ((j + 1) % 4) * 2)),
                cw: Some(EdgeuseId::from_index(i * 8 + ((j + 3) % 4) * 2)),
                vertexuse: Some(vu0),
                orientation: OrientType::Same,
                ..Edgeuse::default()
            })?;
            b.add_edgeuse_record(Edgeuse {
                edge: Some(e),
                owner: EdgeuseOwner::Loopuse(Some(lu1)),
                ccw: Some(EdgeuseId::from_index(i * 8 + ((j + 3) % 4) * 2 + 1)),
                cw: Some(EdgeuseId::from_index(i * 8 + ((j + 1) % 4) * 2 + 1)),
                vertexuse: Some(vu1),
                orientation: OrientType::Opposite,
                ..Edgeuse::default()
            })?;
            b.add_vertexuse_record(Vertexuse {
                vertex: Some(v),
                owner: VertexuseOwner::Edgeuse(Some(eu0)),
                ..Vertexuse::default()
            })?;
            b.add_vertexuse_record(Vertexuse {
                vertex: Some(next),
                owner: VertexuseOwner::Edgeuse(Some(eu1)),
                ..Vertexuse::default()
            })?;
        }
        if let ShellMembers::Faceuses(fus) = &mut b.shell_mut(shell)?.members {
            fus.extend([fu0, fu1]);
        } else {
            b.shell_mut(shell)?.members = ShellMembers::Faceuses(vec![fu0, fu1]);
        }
        b.shell_mut(shell)?.faces.push(f);
    }
    Ok(b)
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let counts: Vec<u32> = std::env::args()
        .skip(1)
        .map(|s| s.parse())
        .collect::<std::result::Result<_, _>>()?;
    let counts = if counts.is_empty() {
        vec![1000, 8000, 50002]
    } else {
        counts
    };
    let k = Kernel::default();
    for n in counts {
        if n > 100_000 {
            return Err("at most 100000 faces".into());
        }
        let mut b = sheets(n)?;
        let mut reports = AssertArray::new();
        let start = Instant::now();
        let status = validate_pointers(&k, &mut b, Some(&mut reports), 2, true, &key)?;
        let pointer_seconds = start.elapsed().as_secs_f64();
        if status != 1000 || !reports.is_empty() {
            return Err(format!(
                "invalid scaling fixture: status={status}, reports={}, first={:?}",
                reports.len(),
                reports.reports().first()
            )
            .into());
        }
        let start = Instant::now();
        let coincidence =
            contains_coincident_topology(&k, &mut b, -1.0, true, Some(&mut reports), 2, &key)?;
        let coincidence_seconds = start.elapsed().as_secs_f64();
        if !coincidence.pairs.is_empty() || !reports.is_empty() {
            return Err("unexpected coincident topology".into());
        }
        println!("faces={n} edges={} vertices={} pointers_s={pointer_seconds:.6} coincidence_s={coincidence_seconds:.6}",b.edges().len(),b.vertices().len());
    }
    Ok(())
}
