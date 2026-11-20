//! Adapt solidcore's conical groove cutters to SolidCraft bodies.

use solidcore_ops::Error;
use solidcore_ops::point_thread as ops;
use solidcore_record::{Vec2, Vec3};

pub use ops::{ConicalThread, Shank, ShankEnd, ThreadHand};

use crate::{Body, BoolOp, KernelError, Result};

/// Bodies to cut from the selected cone and shank. The usual radial screw
/// sweep retains SolidCraft's saved flank topology option.
pub fn grooves(thread: &ConicalThread, continuous_flanks: bool) -> Result<Vec<Body>> {
    solidcraft_smlib::start();
    crate::guard("conical thread", || match ops::grooves(thread).map_err(failed)? {
        ops::Cutter::Tools(tools) => tools
            .into_iter()
            .map(|tool| {
                let (record, _) = tool.to_record().map_err(failed)?;
                Body::from_smlib_record(record)
            })
            .collect(),
        ops::Cutter::Swept(plan) => swept(*plan, continuous_flanks).map(|b| vec![b]),
    })
}

fn failed(error: Error) -> KernelError {
    match error {
        Error::Refused { reason, .. } => KernelError::Invalid(reason.into()),
        Error::ThreadConeTooSteep(_) => KernelError::Invalid(error.to_string()),
        Error::Cancelled => crate::watch::cancelled_error("conical thread"),
        Error::Internal(message) => KernelError::Internal(message),
        other => KernelError::Failed(format!("conical thread: {other}")),
    }
}

fn swept(plan: ops::Swept, continuous_flanks: bool) -> Result<Body> {
    let mut tools = Vec::new();
    for start in plan.starts {
        let mut tool = crate::sweep_helix_radial(
            &start.plane,
            &start.region,
            start.plane.origin,
            Vec3::from(start.axis),
            start.lead,
            plan.turns,
            &plan.law,
            continuous_flanks,
        )?;
        for (plane, positive) in plan.trim.iter().zip([true, false]) {
            tool = crate::half_space(&tool, plane, positive)?.0.ok_or_else(|| KernelError::Failed("the thread's groove misses its face".into()))?;
        }
        tools.push(tool);
    }
    let tool = if let [one] = tools.as_slice() { one.clone() } else { crate::union_all(&tools.iter().collect::<Vec<_>>())? };
    if let Some((plane, region)) = plan.core {
        let core = crate::revolve(&plane, &[region], Vec2::ZERO, Vec2::new(0.0, 1.0), std::f64::consts::TAU)?
            .pop()
            .ok_or_else(|| KernelError::Invalid("the thread's root has no volume".into()))?;
        crate::boolean(&tool, &core, BoolOp::Cut)?.ok_or_else(|| KernelError::Invalid("the thread's groove is empty".into()))
    } else {
        Ok(tool)
    }
}
