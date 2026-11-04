// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::math::Point2;

use crate::test_support::smbh_pcurves_test::{
    synthetic_geometry_with_pcurve_smbh, synthetic_geometry_with_rational_pcurve_smbh,
    synthetic_geometry_with_ref_pcurve_smbh, synthetic_geometry_with_wrapped_ref_pcurve_smbh,
    with_plane_pcurve_offset_vertices,
};
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

const UV_TOLERANCE: f64 = 1.0e-12;
const POINT_TOLERANCE_MM: f64 = 1.0e-10;

#[test]
fn pcurve_uv_offset_translates_cartesian_poles_and_keeps_unchanged_bytes() {
    for (smbh, offset, first, last) in [
        (
            synthetic_geometry_with_pcurve_smbh(),
            [-1.0, 2.0],
            Point2::new(-9.75, 20.5),
            Point2::new(-9.25, 21.5),
        ),
        (
            synthetic_geometry_with_rational_pcurve_smbh(),
            [-1.0, 2.0],
            Point2::new(-9.75, 20.5),
            Point2::new(-9.25, 21.5),
        ),
        (
            synthetic_geometry_with_wrapped_ref_pcurve_smbh(),
            [-1.0, 2.0],
            Point2::new(-7.5, 25.0),
            Point2::new(-2.5, 35.0),
        ),
        (
            synthetic_geometry_with_ref_pcurve_smbh(),
            [-2.0, 4.0],
            Point2::new(-19.75, 40.5),
            Point2::new(-19.25, 41.5),
        ),
    ] {
        let source = f3d_with_smbh(&smbh);
        let decoded = F3dCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .expect("offset decode");
        let pcurve = &decoded.ir().model.pcurves[0];
        assert_eq!(pcurve.uv_offset().expect("stored offset").get(), offset);
        assert_eq!(pcurve.parameter_range(), None);
        let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
            panic!("NURBS carrier")
        };
        let points = nurbs.pole_rows().raw_points();
        for (actual, expected) in [
            (*points.first().expect("first pole"), first),
            (*points.last().expect("last pole"), last),
        ] {
            assert!((actual.u - expected.u).abs() <= UV_TOLERANCE);
            assert!((actual.v - expected.v).abs() <= UV_TOLERANCE);
        }
        let (ir, _, fidelity) = decoded.into_parts();
        let mut encoded = Vec::new();
        crate::test_support::plan_inherited_write(&ir, &fidelity, &mut encoded)
            .expect("unchanged offset patch");
        assert_eq!(encoded, source);
        let after = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("patched offset decode");
        assert_eq!(after.ir().model.pcurves, ir.model.pcurves);
    }
}

#[test]
fn pcurve_uv_offset_plane_endpoints_reach_edge_vertices() {
    let source = f3d_with_smbh(&with_plane_pcurve_offset_vertices(
        synthetic_geometry_with_pcurve_smbh(),
        [-1.0, 2.0],
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("plane offset decode");
    let report = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
        .expect("plane offset validation");
    assert!(report.is_ok(), "{:?}", report.findings);
}

#[test]
fn pcurve_uv_offset_one_native_period_keeps_cylinder_points() {
    use crate::test_support::smbh_pcurves_test::synthetic_periodic_pcurve_with_uv_offset_smbh;
    let decode = |offset| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_periodic_pcurve_with_uv_offset_smbh(offset),
                )),
                &DecodeOptions::default(),
            )
            .expect("periodic support decode")
    };
    let zero = decode([0.0, 0.0]);
    let shifted = decode([0.0, std::f64::consts::TAU]);
    let PcurveGeometry::Nurbs { nurbs: zero_curve } = &zero.ir().model.pcurves[0].geometry else {
        panic!("zero NURBS")
    };
    let PcurveGeometry::Nurbs {
        nurbs: shifted_curve,
    } = &shifted.ir().model.pcurves[0].geometry
    else {
        panic!("shifted NURBS")
    };
    let support = zero.ir().model.surfaces[0]
        .geometry
        .solved()
        .expect("cylinder");
    let ctx = cadmpeg_test_support::service_decode_context();
    for (a, b) in zero_curve
        .pole_rows()
        .raw_points()
        .into_iter()
        .zip(shifted_curve.pole_rows().raw_points())
    {
        assert!((b.u - a.u - std::f64::consts::TAU).abs() <= UV_TOLERANCE);
        assert_eq!(a.v, b.v);
        let point_a = cadmpeg_ir::eval::decode::surface_point_solved(&ctx, support, a.u, a.v)
            .expect("cylinder point")
            .get();
        let point_b = cadmpeg_ir::eval::decode::surface_point_solved(&ctx, support, b.u, b.v)
            .expect("periodic cylinder point")
            .get();
        assert!(point_a.distance(point_b) <= POINT_TOLERANCE_MM);
    }
}

#[test]
fn pcurve_uv_offset_edits_preserve_neutral_geometry_in_each_record_form() {
    use crate::test_support::smbh_pcurves_test::assert_pcurve_geometry_close;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::geometry::pcurve::{PcurveInlineForm, PcurveMetadata};
    use cadmpeg_ir::units::FiniteVector;
    for smbh in [
        synthetic_geometry_with_pcurve_smbh(),
        synthetic_geometry_with_rational_pcurve_smbh(),
        synthetic_geometry_with_wrapped_ref_pcurve_smbh(),
        synthetic_geometry_with_ref_pcurve_smbh(),
    ] {
        let source = f3d_with_smbh(&smbh);
        let decoded = F3dCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("offset decode");
        let (mut ir, _, fidelity) = decoded.into_parts();
        let offset = FiniteVector::new([3.0, -2.0]).expect("finite offset");
        match &mut ir.model.pcurves[0].metadata {
            PcurveMetadata::AsmInline { form } => {
                *form = PcurveInlineForm::new(
                    form.wrapper_reversed,
                    form.native_tail_flags,
                    offset,
                    form.fit_tolerance(),
                );
            }
            PcurveMetadata::General { form } => form.uv_offset = Some(offset),
        }
        let mut patched = Vec::new();
        crate::test_support::plan_inherited_write(&ir, &fidelity, &mut patched)
            .expect("offset patch");
        let decoded = F3dCodec
            .decode(&mut Cursor::new(patched), &DecodeOptions::default())
            .expect("patched decode");
        assert_eq!(
            decoded.ir().model.pcurves[0].metadata,
            ir.model.pcurves[0].metadata
        );
        assert_pcurve_geometry_close(
            &decoded.ir().model.pcurves[0].geometry,
            &ir.model.pcurves[0].geometry,
        );
        ir.source = None;
        ir.set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .expect("clear native records");
        let mut generated = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut generated))
            .expect("offset generation");
        let decoded = F3dCodec
            .decode(&mut Cursor::new(generated), &DecodeOptions::default())
            .expect("generated decode");
        assert_eq!(decoded.ir().model.pcurves[0].uv_offset(), Some(offset));
        assert_pcurve_geometry_close(
            &decoded.ir().model.pcurves[0].geometry,
            &ir.model.pcurves[0].geometry,
        );
    }
}

#[test]
fn pcurve_uv_offset_new_record_defaults_to_zero() {
    use crate::test_support::smbh_pcurves_test::assert_pcurve_geometry_close;
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("source decode");
    let (mut ir, _, _) = decoded.into_parts();
    ir.source = None;
    ir.set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .expect("clear native records");
    ir.model.pcurves[0].metadata = cadmpeg_ir::geometry::pcurve::PcurveMetadata::default();
    let mut generated = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut generated))
        .expect("new record generation");
    let after = F3dCodec
        .decode(&mut Cursor::new(generated), &DecodeOptions::default())
        .expect("new record decode");
    assert_eq!(
        after.ir().model.pcurves[0]
            .uv_offset()
            .expect("stored zero offset")
            .get(),
        [0.0, 0.0]
    );
    assert_eq!(after.ir().model.pcurves[0].parameter_range(), None);
    assert_pcurve_geometry_close(
        &after.ir().model.pcurves[0].geometry,
        &ir.model.pcurves[0].geometry,
    );
}
