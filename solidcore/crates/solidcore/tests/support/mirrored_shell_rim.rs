//! Captured shell-rim operands and their blend builder, shared by the geometry and ledger tests.
#![allow(dead_code, clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::blend::definition_check::BlendFaceLog;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::AttributeStore;
use solidcore::math::PI;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel, Vec3};

/// Load the original captured operand with the rig's fix set.
pub fn input() -> (Kernel, Brep) {
    input_from_bytes(include_bytes!("../data/issue_2903_shelled_box_rim.smb"))
}

/// Load the edited-height capture with the rig's fix set.
pub fn edited_input() -> (Kernel, Brep) {
    input_from_bytes(include_bytes!(
        "../data/issue_2903_edited_shelled_box_rim.smb"
    ))
}

/// Read a fresh operand and enable definition logging without preparing its geometry.
fn input_from_bytes(bytes: &[u8]) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    let fixes: Vec<String> =
        serde_json::from_str(include_str!("../kernel_cases/fixes.json")).unwrap();
    for name in fixes {
        *k.fixes.flag_mut(&name).unwrap() = true;
    }
    k.blend_face_log = Some(BlendFaceLog::default());
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let b = read_brep_from_file(&mut k, Some(bytes.to_vec()), true, false, &mut env).unwrap();
    (k, b)
}

/// The rim's circular round or equal-distance chamfer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A circular section.
    Round,
    /// An equal-distance linear section.
    Chamfer,
}

/// Build the entire inner rim from one seed, using the same construction in both tests.
pub fn build_rim(
    k: &mut Kernel,
    b: &mut Brep,
    point: Vec3,
    kind: Kind,
    size: f64,
) -> solidcore::Result<()> {
    let picked = b
        .edge_list()
        .iter()
        .copied()
        .map(|e| {
            (
                e,
                edge_closest_point(k, b, e, point).unwrap().closest.distance,
            )
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    assert!(picked.1 < 1e-8, "rim selection missed {point:?}");
    let edges =
        solidcore::blend::blend_refusal::tangent_chain(k, b, &[picked.0], 0.5 * PI / 180.0)?;
    assert_eq!(edges.len(), 8, "the entire inner rim");
    {
        let mut s = FilletSession::new(k, b)?;
        s.set_swept_turns(false);
        match kind {
            Kind::Round => s.prepare_per_edge_constant_radius(
                k,
                &edges,
                &[size],
                &[FilletSurfaceGeneratorType::Circular],
                1,
                1.0,
            )?,
            Kind::Chamfer => {
                s.prepare_chamfers_along(k, &edges, &[ChamferSize::Distances([size, size])], &[])?;
            }
        }
        s.create_corners(k, None)?;
        if kind == Kind::Chamfer {
            for corner in s.fillet_corners()? {
                s.set_corner_bevel(corner, true)?;
            }
        }
        s.set_overflow_first(kind == Kind::Round);
        s.set_classify_only(false);
        s.do_filleting(k, None)?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default())?;
    b.refresh_cached_topology();
    Ok(())
}
