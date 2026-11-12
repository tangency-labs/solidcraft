//! A CAM job's machine rates (docs/cam-jobs.md, "Machine rates"): the job's `machine_rates`
//! (or the file `machine_rates_path` names) read and checked, and the written program timed
//! against them with `solidcraft_cam::machine_time`, so the reply's times are the machine's and
//! the blocks its controller would slow are reported.

use std::path::Path;

use solidcraft_cam::gcode5::Post5;
use solidcraft_cam::kinematics::Machine;
use solidcraft_cam::machine_time::{DwellUnit, MachineRates, machine_time};

use super::{beside, cam_err, parse, read_json};
use crate::Result;
use crate::params::bad;
use crate::schema::cam::CamJob;
use crate::schema::cam_rates::{MachineRatesSpec, MachineTimeReport, SpindleSpeedReport};

/// The job's machine rates, checked; `None` when it gives none.
pub(super) fn rates_of(cmd: &str, job: &CamJob, base: Option<&Path>) -> Result<Option<MachineRates>> {
    let spec: MachineRatesSpec = match (&job.machine_rates, &job.machine_rates_path) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give the job `machine_rates` or `machine_rates_path`, not both")),
        (None, None) => return Ok(None),
        (Some(r), None) => r.clone(),
        (None, Some(path)) => {
            let path = beside(base, path);
            parse(cmd, &read_json(cmd, &path, "machine_rates_path")?, &format!("machine_rates_path {path}"))?
        }
    };
    let rates = MachineRates {
        rapid_xyz_mm_min: spec.rapid_xyz_mm_min,
        rapid_z_mm_min: spec.rapid_z_mm_min,
        rapid_a_deg_min: spec.rapid_a_deg_min,
        max_feed_mm_min: spec.max_feed_mm_min,
        max_feed_z_mm_min: spec.max_feed_z_mm_min,
        max_a_deg_min: spec.max_a_deg_min,
        spindle_rpm: spec.spindle_rpm,
        spindle_start_seconds: spec.spindle_start_seconds,
        travel_mm: spec.travel_mm,
        accel_mm_s2: spec.accel_mm_s2,
        accel_z_mm_s2: spec.accel_z_mm_s2,
        accel_a_deg_s2: spec.accel_a_deg_s2,
        tool_change_seconds: spec.tool_change_seconds,
        home_z_mm: spec.home_z_mm,
    };
    rates.validate().map_err(|e| cam_err(cmd, "`machine_rates`", e))?;
    Ok(Some(rates))
}

/// `text`, written through `post` for `machine`, timed against `rates`.
pub(super) fn time_report(cmd: &str, text: &str, machine: &Machine, post: &Post5, rates: &MachineRates) -> Result<MachineTimeReport> {
    let unit = DwellUnit::of(&post.base).map_err(|e| cam_err(cmd, "the post", e))?;
    let t = machine_time(text, &machine.letters(), rates, unit).map_err(|e| cam_err(cmd, "timing the program against `machine_rates`", e))?;
    Ok(MachineTimeReport {
        cut_minutes: t.cut_minutes,
        written_cut_minutes: t.written_cut_minutes,
        rapid_minutes: t.rapid_minutes,
        spindle_minutes: t.spindle_minutes,
        home_returns: t.home_returns,
        total_minutes: t.total_minutes,
        slowed_blocks: t.slowed_blocks,
        rotary_over: t.rotary_over,
        feed_over: t.feed_over,
        peak_rotary_deg_min: t.peak_rotary_deg_min.map(|(_, r)| r),
        peak_rotary_line: t.peak_rotary_deg_min.map(|(l, _)| l),
        peak_feed_mm_min: t.peak_feed_mm_min.map(|(_, f)| f),
        peak_feed_line: t.peak_feed_mm_min.map(|(l, _)| l),
        spindle_out_of_range: t.spindle_out_of_range.into_iter().map(|(line, rpm)| SpindleSpeedReport { line, rpm }).collect(),
        span_mm: t.span_mm,
        warnings: t.warnings,
        limits: t.limits,
    })
}
