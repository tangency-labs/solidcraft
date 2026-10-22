//! Production adapter probe for the independent SMLib comparison.
use solidcore::curve::{curve::Curve, line::Line};
use solidcore::geom::attribute::{Attribute, AttributeValue, Behavior};
use solidcore::geom::context::NotifyCallback;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::sweep::topology_sweep::{access::SweepAccess, store::SweepStore};
use solidcore::topology::brep::p1 as bp;
use solidcore::topology::{BrepId, VertexId};
use solidcore::{EntityRef, Result, SolidError};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const ATTRIBUTE: u64 = 4242;
const TYPES: [i64; 4] = [16021, 16016, 16002, 16011];
#[derive(Debug)]
struct Copies(Arc<Mutex<[i64; 4]>>);
impl NotifyCallback for Copies {
    fn execute(
        &mut self,
        op: NotifyOperation,
        caller: ObjectRef,
        _: Option<ObjectRef>,
        _: Option<ObjectRef>,
        _: Option<ObjectRef>,
    ) -> Result<()> {
        if op == NotifyOperation::Copy {
            if let Some(i) = TYPES.iter().position(|t| *t == caller.type_code) {
                *lock(&self.0)?.get_mut(i).ok_or_else(|| {
                    SolidError::invalid_input("merge_adapter", "copy type index")
                })? += 1;
            }
        }
        Ok(())
    }
}
fn lock(log: &Arc<Mutex<[i64; 4]>>) -> Result<std::sync::MutexGuard<'_, [i64; 4]>> {
    log.lock()
        .map_err(|_| SolidError::invalid_input("merge_adapter", "copy log poisoned"))
}
fn argument<T: std::str::FromStr>(args: &[String], index: usize) -> Result<T> {
    args.get(index)
        .ok_or_else(|| SolidError::invalid_input("merge_adapter", "argument missing"))?
        .parse()
        .map_err(|_| SolidError::invalid_input("merge_adapter", "argument invalid"))
}
fn point(store: &mut SweepStore, body: BrepId, p: Vec3) -> Result<VertexId> {
    let region = store.infinite_region(body)?;
    Ok(store.make_shell_vertex((body, region), p)?.1 .1)
}
fn dump(
    store: &mut SweepStore,
    id: BrepId,
    ints: &mut Vec<i64>,
    values: &mut Vec<f64>,
) -> Result<()> {
    if !store.contains_brep(id) {
        ints.push(0);
        return Ok(());
    }
    ints.push(1);
    let shells = store.with_brep_mut(id, |k, b, _, _| bp::shells(k, b, None))?;
    let b = store.brep(id)?;
    let vertices = bp::vertices(b, None)?;
    let edges = bp::edges(b, None)?;
    ints.extend([
        bp::regions(b, None)?.len() as i64,
        shells.len() as i64,
        bp::faces(b, None)?.len() as i64,
        vertices.len() as i64,
        edges.len() as i64,
        i64::from(b.editing_enabled()),
        i64::from(store.kernel(id)?.context.doing_boolean()),
    ]);
    for v in &vertices {
        let vertex = b.vertex(*v)?;
        values.extend([
            vertex.point.x,
            vertex.point.y,
            vertex.point.z,
            vertex.tolerance,
        ]);
        let attribute = store
            .attributes(id)?
            .find_attribute(store.object_ref(id, EntityRef::Vertex(*v))?.key, ATTRIBUTE);
        ints.push(
            attribute
                .and_then(|h| store.attributes(id).ok()?.get(h))
                .map_or(-1, |a| match a.value() {
                    AttributeValue::Long(v) => *v,
                    _ => -2,
                }),
        );
        ints.push(
            attribute
                .map(|h| store.attributes(id)?.users(h).map(|u| u.len() as i64))
                .transpose()?
                .unwrap_or(0),
        );
    }
    for edge in edges {
        let edge = b.edge(edge)?;
        ints.extend(edge.vertices.map(|v| {
            vertices
                .iter()
                .position(|a| *a == v)
                .map_or(-1, |i| i as i64)
        }));
        ints.push(edge.edgeuses.len() as i64);
        let curve = edge
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer("merge_adapter"))?;
        ints.push(curve.type_code());
        values.extend([edge.interval.min(), edge.interval.max(), edge.tolerance]);
        for f in [0., 0.25, 0.5, 0.75, 1.] {
            let p = curve.evaluate_point(
                store.kernel(id)?,
                edge.interval.min() + f * (edge.interval.max() - edge.interval.min()),
            )?;
            values.extend([p.x, p.y, p.z]);
        }
    }
    Ok(())
}
fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if !(5..=6).contains(&a.len()) {
        return Err(SolidError::invalid_input(
            "merge_adapter",
            "expected operation, geometry kind, shared context, scale, repetitions, optional source constructor context",
        ));
    }
    let operation: u32 = argument(&a, 0)?;
    let kind: u32 = argument(&a, 1)?;
    let shared: bool = match argument::<u32>(&a, 2)? {
        0 => false,
        1 => true,
        _ => {
            return Err(SolidError::invalid_input(
                "merge_adapter",
                "shared context flag",
            ))
        }
    };
    let scale: f64 = argument(&a, 3)?;
    let repetitions: u32 = argument(&a, 4)?;
    let constructor_uses_source = if a.len() == 6 {
        match argument::<u32>(&a, 5)? {
            0 => false,
            1 => true,
            _ => {
                return Err(SolidError::invalid_input(
                    "merge_adapter",
                    "constructor context flag",
                ))
            }
        }
    } else {
        false
    };
    if operation > 5
        || kind > 3
        || !scale.is_finite()
        || scale <= 0.
        || !(1..=3).contains(&repetitions)
        || (operation == 5 && repetitions != 1)
    {
        return Err(SolidError::invalid_input(
            "merge_adapter",
            "argument bounds",
        ));
    }
    let source_log = Arc::new(Mutex::new([0; 4]));
    let dest_log = if shared {
        source_log.clone()
    } else {
        Arc::new(Mutex::new([0; 4]))
    };
    let mut store = SweepStore::owned();
    let mut source_kernel = Kernel::default();
    source_kernel
        .context
        .set_user_notify_callback(Some(Box::new(Copies(source_log.clone()))));
    let sc = store.insert_kernel(source_kernel)?;
    let dc = if shared {
        sc
    } else {
        let mut k = Kernel::default();
        k.context
            .set_user_notify_callback(Some(Box::new(Copies(dest_log.clone()))));
        store.insert_kernel(k)?
    };
    let source = store.create_brep(sc)?;
    let dest = store.create_brep(dc)?;
    let tolerance = scale * 1e-6;
    for body in [source, dest] {
        store.with_brep_mut(body, |k, b, _, _| {
            solidcore::topology::brep::p2::set_tolerance(k, b, tolerance, true, true)
        })?;
    }
    let mut selected = Vec::new();
    if kind == 1 || kind == 2 {
        let first = point(&mut store, source, Vec3::new(1., 2., 3.) * scale)?;
        selected.push(first);
        let second = point(&mut store, source, Vec3::new(4., -1., 5.) * scale)?;
        selected.push(second);
        if kind == 2 {
            let curve = Curve::from(Line::create_line_segment(
                3,
                Vec3::new(1., 2., 3.) * scale,
                Vec3::new(4., -1., 5.) * scale,
                None,
            )?);
            let interval = curve.natural_interval();
            let region = store.infinite_region(source)?;
            store.make_wire_edge(
                (source, region),
                (source, first),
                (source, second),
                curve,
                interval,
                OrientType::Same,
            )?;
        }
        let source_key = store.object_ref(source, EntityRef::Vertex(first))?.key;
        let h = store.attributes_mut(source)?.insert(Attribute::long(
            ATTRIBUTE,
            73,
            Behavior::Reference,
        ))?;
        store.attributes_mut(source)?.add_attribute(source_key, h)?;
    }
    if kind != 3 {
        point(&mut store, dest, Vec3::new(-7., 4., 2.) * scale)?;
    }
    *lock(&source_log)? = [0; 4];
    *lock(&dest_log)? = [0; 4];
    let mut copied = 0;
    let mut mates = BTreeMap::new();
    let mut maybe_open = false;
    let result = (|| {
        if operation == 2 || operation == 3 || operation == 5 {
            let context_of = if constructor_uses_source {
                source
            } else {
                dest
            };
            store.with_merge(context_of, dest, source, tolerance, 0., |store, merge| {
                for _ in 0..repetitions {
                    match operation {
                        2 => store.copy_faces(source, &[], dest, merge, &mut mates)?,
                        3 => {
                            store.piecewise_merge(merge, true, true)?;
                        }
                        5 => {
                            store.union(merge)?;
                        }
                        _ => {
                            return Err(SolidError::invalid_input(
                                "merge_adapter",
                                "operation branch",
                            ))
                        }
                    }
                }
                Ok(())
            })
        } else {
            for _ in 0..repetitions {
                match operation {
                    0 => copied += store.copy_vertices(source, &selected, dest)?.len(),
                    1 => store.merge_brep(dest, source)?,
                    4 => maybe_open = store.orient(source, true, true)?,
                    _ => {
                        return Err(SolidError::invalid_input(
                            "merge_adapter",
                            "operation branch",
                        ))
                    }
                }
            }
            Ok(())
        }
    })();
    let (status, deferred) = match result {
        Ok(()) => (0, None),
        Err(SolidError::Upstream { code, .. }) => (code, None),
        Err(SolidError::Unported { unit, .. }) => (0, Some(unit)),
        Err(error) => return Err(error),
    };
    let mut ints = vec![
        i64::from(status),
        copied as i64,
        mates.len() as i64,
        i64::from(maybe_open),
    ];
    let mut values = Vec::new();
    dump(&mut store, source, &mut ints, &mut values)?;
    dump(&mut store, dest, &mut ints, &mut values)?;
    ints.extend(*lock(&source_log)?);
    ints.extend(*lock(&dest_log)?);
    println!(
        "{{\"ints\":{ints:?},\"values\":{values:?},\"deferred\":{}}}",
        deferred.map_or("null".to_owned(), |s| format!("{s:?}"))
    );
    Ok(())
}
