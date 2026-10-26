//! Direct native helper proof; not a full `ProcessRollOver` fixture.
use super::{ffi, harness};
use crate::{HarnessError, Output};
use solidcore::blend::fillet_geom::p1::{FilletBrep, FilletGeom, FilletGeomType, FilletVertex};
use solidcore::blend::fillet_solver::p1::{split_rail, RailOwnership, RetainedRailOwnership};
use solidcore::curve::{curve::Curve, line::Line};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::context::NotifyCallback;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::math::Vec3;
use solidcore::topology::{brep::p3, edge, vertex, VertexId};
use solidcore::EntityRef;
use solidcore::{Kernel, SolidError};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct Event {
    system: bool,
    caller: ObjectRef,
    data: [Option<ObjectRef>; 3],
    owner: Option<ObjectRef>,
}
#[derive(Debug)]
struct Recorder {
    system: bool,
    events: Arc<Mutex<Vec<Event>>>,
    owners: BTreeMap<ObjectKey, Option<ObjectRef>>,
}
impl NotifyCallback for Recorder {
    fn execute(
        &mut self,
        operation: NotifyOperation,
        caller: ObjectRef,
        a: Option<ObjectRef>,
        b: Option<ObjectRef>,
        c: Option<ObjectRef>,
    ) -> solidcore::Result<()> {
        if operation == NotifyOperation::ChangeOwner {
            self.events
                .lock()
                .map_err(|_| invalid("poisoned event record"))?
                .push(Event {
                    system: self.system,
                    caller,
                    data: [a, b, c],
                    owner: self.owners.get(&caller.key).copied().flatten(),
                });
        }
        Ok(())
    }
    fn owner_link(&mut self, object: ObjectKey, owner: Option<ObjectRef>) {
        self.owners.insert(object, owner);
    }
}
fn flag(value: bool) -> f64 {
    if value {
        1.
    } else {
        0.
    }
}
const OP: &str = "difftest::split_rail";
fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}
fn required<T>(value: Option<T>) -> solidcore::Result<T> {
    value.ok_or_else(|| invalid("missing actual split object or identity"))
}
fn integer(value: f64) -> Result<i64, HarnessError> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < i64::MIN as f64
        || value >= i64::MAX as f64
    {
        return Err(harness("noninteger split field"));
    }
    Ok(value as i64)
}
fn output(flat: &[f64]) -> Result<Output, HarnessError> {
    let [status, count, tail @ ..] = flat else {
        return Err(harness("missing split header"));
    };
    if *status != 0.0 {
        return Err(harness("direct split did not complete"));
    }
    let event_count =
        usize::try_from(integer(*count)?).map_err(|_| harness("negative split event count"))?;
    let prefix_len = event_count
        .checked_mul(7)
        .and_then(|n| n.checked_add(12))
        .ok_or_else(|| harness("record overflow"))?;
    let expected_len = prefix_len
        .checked_add(24)
        .ok_or_else(|| harness("record overflow"))?;
    if tail.len() != expected_len {
        return Err(harness("invalid split record length"));
    }
    let (prefix, numerical) = tail
        .split_at_checked(prefix_len)
        .ok_or_else(|| harness("missing split prefix"))?;
    let mut exact = vec![integer(*status)?, integer(*count)?];
    exact.extend(
        prefix
            .iter()
            .copied()
            .map(integer)
            .collect::<Result<Vec<_>, _>>()?,
    );
    let (initial, samples) = numerical
        .split_at_checked(12)
        .ok_or_else(|| harness("missing split geometry"))?;
    let mut values = initial.to_vec();
    let (points, remainder) = samples.as_chunks::<4>();
    if points.len() != 3 || !remainder.is_empty() {
        return Err(harness("invalid split sample count"));
    }
    for &[status, x, y, z] in points {
        if status != 0.0 {
            return Err(harness("split curve sample failed"));
        }
        exact.push(integer(status)?);
        values.extend([x, y, z]);
    }
    Ok(Output::new()
        .ints("actual_identity_topology", exact)
        .values("actual_curve", values))
}
pub(super) fn cpp() -> Result<Output, HarnessError> {
    output(
        &ffi::eval_static(ffi::StaticOp::SplitRail, "", &[]).map_err(|e| harness(e.to_string()))?,
    )
}
pub(super) fn rust() -> Result<Output, HarnessError> {
    let run = || -> solidcore::Result<Vec<f64>> {
        let mut k = Kernel::default();
        let mut pseudo = FilletBrep::new();
        let mut vertices = Vec::new();
        for point in [Vec3::ZERO, Vec3::X] {
            let id = pseudo.brep_mut().add_vertex(point)?;
            pseudo.insert_fillet_vertex(id, FilletVertex::new(&k, None));
            vertices.push(id);
        }
        let [start, end]: [VertexId; 2] = vertices
            .try_into()
            .map_err(|_| invalid("invalid endpoint count"))?;
        let rail = pseudo.make_fillet_edge(Some(start), Some(end), None)?;
        let line = Curve::from(Line::from_points(Vec3::ZERO, Vec3::X, 3));
        pseudo.brep_mut().edge_mut(rail)?.curve = Some(line);
        edge::set_tolerance(pseudo.brep_mut(), rail, 0.125, true, true)?;
        let mut attributes = AttributeStore::default();
        RetainedRailOwnership::with_context(
            &mut k,
            &mut pseudo,
            &mut attributes,
            |_, pseudo, ownership| {
                let (owner, brep) = ownership.edge_owner(rail)?;
                let curve = required(pseudo.brep_mut().edge_mut(rail)?.curve.as_mut())?;
                ownership.set_curve_owner(rail, curve, owner, brep)
            },
        )?;
        let serial = required(pseudo.brep().notify_serial())?;
        let old_key = required(required(pseudo.brep().edge(rail)?.curve.as_ref())?.notify_key())?;
        let geom = pseudo.add_geom(FilletGeom::new(&k, None, FilletGeomType::Default)?);
        let events = Arc::new(Mutex::new(Vec::new()));
        // This case owns a fresh Context, with no existing callback to displace.
        k.context.set_user_notify_callback(Some(Box::new(Recorder {
            system: false,
            events: events.clone(),
            owners: BTreeMap::new(),
        })));
        k.context.set_sys_notify_callback(Some(Box::new(Recorder {
            system: true,
            events: events.clone(),
            owners: BTreeMap::new(),
        })));
        let (middle, new_rail) = RetainedRailOwnership::with_context(
            &mut k,
            &mut pseudo,
            &mut attributes,
            |k, pseudo, ownership| split_rail(k, pseudo, ownership, geom, rail),
        )?;
        let copied = required(pseudo.brep().edge(new_rail)?.curve.as_ref())?;
        let owner = p3::topology_ref(serial, EntityRef::Edge(new_rail));
        let actual = ObjectRef {
            key: required(copied.notify_key())?,
            type_code: copied.type_code(),
        };
        let events = events
            .lock()
            .map_err(|_| invalid("poisoned event record"))?;
        let mut out = vec![0., events.len() as f64];
        for e in events.iter() {
            out.extend([
                flag(e.system),
                e.caller.type_code as f64,
                flag(e.caller == actual),
                flag(e.data[0] == Some(owner)),
                flag(e.data[1].is_none()),
                flag(e.data[2].is_none()),
                flag(e.owner.is_none()),
            ]);
        }
        out.extend([
            copied.type_code() as f64,
            flag(actual.key != old_key),
            flag(copied.owner() == Some(owner)),
            flag(edge::vertex(pseudo.brep(), rail)? == Some(start)),
            flag(edge::other_vertex(pseudo.brep(), rail, Some(start))? == Some(middle)),
            flag(edge::vertex(pseudo.brep(), new_rail)? == Some(middle)),
            flag(edge::other_vertex(pseudo.brep(), new_rail, Some(middle))? == Some(end)),
            pseudo.brep().edge(rail)?.edgeuses.len() as f64,
            pseudo.brep().edge(new_rail)?.edgeuses.len() as f64,
            pseudo.brep().vertex(start)?.vertexuses.len() as f64,
            pseudo.brep().vertex(middle)?.vertexuses.len() as f64,
            pseudo.brep().vertex(end)?.vertexuses.len() as f64,
        ]);
        let domain = copied.natural_interval();
        out.extend([
            pseudo.brep().edge(new_rail)?.tolerance,
            domain.min(),
            domain.max(),
        ]);
        for id in [start, middle, end] {
            out.push(vertex::tolerance(pseudo.brep(), id)?);
        }
        // The newly allocated middle vertex's point is unset by sm_SplitRail.
        // Only the two explicitly initialized endpoint coordinates are read.
        for id in [start, end] {
            let point = vertex::point(pseudo.brep(), id)?;
            out.extend([point.x, point.y, point.z]);
        }
        for f in [0., 0.5, 1.] {
            let p = copied.evaluate_point(&k, domain.evaluate(f))?;
            out.extend([0., p.x, p.y, p.z]);
        }
        Ok(out)
    };
    output(&run().map_err(|e| harness(e.to_string()))?)
}
