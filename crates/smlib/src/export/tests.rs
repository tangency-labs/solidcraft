#[cfg(feature = "native")]
use super::*;

#[cfg(feature = "native")]
mod native_tests {
    use super::*;
    use crate::{CheckLevel, MEASURE_ACCURACY};
    use std::f64::consts::PI;

    /// SMLib's primitives with their closed-form volumes.
    #[allow(clippy::unwrap_used)] // test fixture
    fn primitives() -> Vec<(&'static str, Brep, f64)> {
        vec![
            ("box", Brep::make_box([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]).unwrap(), 6000.0),
            ("cylinder", Brep::make_cylinder([1.0, 2.0, 3.0], 5.0, 10.0).unwrap(), PI * 25.0 * 10.0),
            ("sphere", Brep::make_sphere([1.0, 2.0, 3.0], 5.0).unwrap(), 4.0 / 3.0 * PI * 125.0),
            ("frustum", Brep::make_cone([0.0; 3], 5.0, 2.0, 10.0).unwrap(), PI * 10.0 / 3.0 * (25.0 + 10.0 + 4.0)),
            ("widening frustum", Brep::make_cone([0.0; 3], 2.0, 5.0, 10.0).unwrap(), PI * 10.0 / 3.0 * (25.0 + 10.0 + 4.0)),
            ("cone", Brep::make_cone([0.0; 3], 5.0, 0.0, 10.0).unwrap(), PI * 10.0 / 3.0 * 25.0),
            ("torus", Brep::make_torus([0.0; 3], 10.0, 3.0).unwrap(), 2.0 * PI * PI * 10.0 * 9.0),
        ]
    }

    #[test]
    fn every_primitive_exports_validates_and_keeps_its_volume() {
        for (name, b, exact) in primitives() {
            let check = b.check(CheckLevel::All).unwrap();
            assert!(check.is_valid(), "{name}: {check}");
            let v = b.mass_properties(MEASURE_ACCURACY).unwrap().volume;
            assert!((v - exact).abs() < 1e-6 * exact, "{name}: SMLib volume {v}, closed form {exact}");
            let (native, report) = b.to_native_report().unwrap_or_else(|e| panic!("{name}: {e}"));
            native.validate().unwrap();
            native.check_geometry().unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(report, Report { nurbs_edges: report.nurbs_edges, ..Report::default() }, "{name}: every face agrees");
            let [f, e, n] = b.counts().unwrap();
            assert_eq!((native.faces.len(), native.edges.len(), native.vertices.len()), (f, e, n), "{name}");
            // One closed shell bounding one material region; no NURBS edges left.
            assert_eq!(native.shells.len(), 1, "{name}");
            assert_eq!(native.regions.len(), 2, "{name}");
            assert!(!native.regions[0].void && !native.regions[1].void, "{name}");
            assert!(native.edges.iter().all(|e| matches!(e.curve, b::Curve::Line { .. } | b::Curve::Circle { .. })), "{name}");
        }
    }

    #[test]
    fn every_primitive_goes_round_through_schema_4_unchanged() {
        for (name, s, _) in primitives() {
            let record = s.to_native().unwrap_or_else(|e| panic!("{name}: {e}"));
            let back = Brep::from_record(&record, true).unwrap_or_else(|e| panic!("{name}: {e}"));
            let check = back.check(CheckLevel::All).unwrap();
            assert!(check.is_valid(), "{name}: {check}");
            assert_eq!(back.counts().unwrap(), s.counts().unwrap(), "{name}");
            let (v0, v1) = (s.mass_properties(MEASURE_ACCURACY).unwrap().volume, back.mass_properties(MEASURE_ACCURACY).unwrap().volume);
            assert!((v1 - v0).abs() < 1e-7 * v0, "{name}: volume {v0} -> {v1}");
        }
    }

    /// A tagged body's own history, renumbered with its export's order on both sides, is the
    /// identity in canonical order: the order says where each SMLib face and edge went.
    #[test]
    fn the_export_order_renumbers_a_history_into_the_record() {
        for (name, s, _) in primitives() {
            let t = s.tagged(0).unwrap();
            let (record, order) = t.to_native_ordered().unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((order.faces.len(), order.edges.len()), (record.faces.len(), record.edges.len()), "{name}");
            let h = t.history().unwrap().history(std::slice::from_ref(&order), &order).unwrap();
            for (i, f) in h.faces.iter().enumerate() {
                assert_eq!(f.origins, vec![solidcraft_geom::brep::history::FaceOrigin::Face { operand: 0, face: i }], "{name}: face {i}");
            }
            for (i, e) in h.edges.iter().enumerate() {
                assert_eq!(e.origins, vec![solidcraft_geom::brep::history::EdgeOrigin::Edge { operand: 0, edge: i }], "{name}: edge {i}");
                // The faces it bounds, as the record's uses say.
                let mut faces: Vec<usize> = record.edges[i].uses.iter().map(|u| u.face as usize).collect();
                faces.sort();
                faces.dedup();
                assert_eq!(e.faces, faces, "{name}: edge {i}");
            }
        }
    }

    #[test]
    fn edges_are_on_smlibs_curves() {
        let cyl = Brep::make_cylinder([0.0; 3], 5.0, 10.0).unwrap().to_native().unwrap();
        let side = cyl.faces.iter().find(|f| matches!(f.surface, b::Surface::Cylinder { .. })).unwrap();
        // The side is on SMLib's NURBS form of the cylinder (its domain is the form's): the whole
        // turn, so its domain's two u ends meet, from the base to the top.
        let form = b::eval::SurfaceEval::new(side.parameters()).unwrap();
        let [[u0, u1], [v0, v1]] = side.domain;
        for v in [v0, v1] {
            let (a, z) = (form.point([u0, v]).unwrap(), form.point([u1, v]).unwrap());
            assert!(a.dist(z) < 1e-9, "{a:?} {z:?}");
        }
        let heights = [form.point([u0, v0]).unwrap().z, form.point([u0, v1]).unwrap().z];
        assert!(heights.iter().any(|h| h.abs() < 1e-9) && heights.iter().any(|h| (h - 10.0).abs() < 1e-9), "{heights:?}");
        for e in &cyl.edges {
            // Evaluated on the curve its interval is on (a circle's NURBS form).
            let eval = b::eval::CurveEval::new(e.parameters()).unwrap();
            let [a, z] = e.interval;
            // Ends on the edge's vertices.
            for (t, v) in [(a, e.vertices[0]), (z, e.vertices[1])] {
                let p = eval.point(t).unwrap();
                let q = cyl.vertices[v as usize].point;
                assert!((p - Vec3::new(q[0], q[1], q[2])).len() < 1e-9, "{:?} at {t}", e.curve);
            }
            if let b::Curve::Circle { radius, .. } = e.curve {
                let [a, z] = e.curve_interval().unwrap();
                assert!((radius - 5.0).abs() < 1e-12 && (z - a - TAU).abs() < 1e-9);
            }
        }
        // The narrowing cone keeps SMLib's negative half angle.
        let cone = Brep::make_cone([0.0; 3], 5.0, 0.0, 10.0).unwrap().to_native().unwrap();
        let half = cone.faces.iter().find_map(|f| if let b::Surface::Cone { half_angle, .. } = f.surface { Some(half_angle) } else { None });
        assert!((half.unwrap() + 0.5f64.atan()).abs() < 1e-12);
    }

    /// A hole tangent to the block's outline from inside: the material touches itself along the
    /// tangent line, where SMLib's one shell uses the edge four times. The record has two copies
    /// of that edge, each bounding one wedge of material (a face of the side and the half of the
    /// hole on the same side of the line), so every edge has two uses; the line's two vertices
    /// stay shared. The record goes back into SMLib as a valid solid of the same volume.
    #[test]
    fn a_hole_tangent_to_the_outline_comes_back_as_a_manifold_boundary() {
        let block = Brep::make_box([0.0; 3], [31.0, 29.0, 10.0]).unwrap();
        let hole = Brep::make_cylinder([5.0, 14.5, -1.0], 5.0, 12.0).unwrap();
        let r = block.boolean(&hole, crate::BooleanOp::Subtract).unwrap().unwrap();
        assert!(!r.is_manifold_solid().unwrap(), "SMLib keeps the touching boundary as one non-manifold shell");
        let (record, order) = r.to_native_ordered().unwrap();
        let [f, e, v] = r.counts().unwrap();
        assert_eq!((record.faces.len(), record.edges.len(), record.vertices.len()), (f, e + 1, v));
        assert_eq!((record.shells.len(), record.regions.len()), (1, 2));
        assert!(record.edges.iter().all(|e| e.uses.len() == 2));
        record.check_geometry().unwrap();
        let on_line = |e: &b::Edge| {
            e.vertices.iter().all(|v| {
                let p = record.vertices[*v as usize].point;
                p[0].abs() < 1e-9 && (p[1] - 14.5).abs() < 1e-9
            })
        };
        // On the curve the interval is on (a circle's NURBS form).
        let middle = |e: &b::Edge| b::eval::CurveEval::new(e.parameters()).unwrap().point(0.5 * (e.interval[0] + e.interval[1])).unwrap();
        let copies: Vec<&b::Edge> = record.edges.iter().filter(|e| on_line(e)).collect();
        assert_eq!(copies.len(), 2);
        for copy in copies {
            // Each face's other edges lie on one side of y = 14.5: the same side for both faces.
            let sides: Vec<f64> = copy
                .uses
                .iter()
                .map(|u| {
                    let b::Loop::Edges { edges } = &record.faces[u.face as usize].loops[u.r#loop as usize] else { panic!("vertex loop") };
                    let others: Vec<f64> =
                        edges.iter().map(|o| &record.edges[o.edge as usize]).filter(|o| !on_line(o)).map(|o| middle(o).y - 14.5).collect();
                    others.iter().sum::<f64>() / others.len() as f64
                })
                .collect();
            assert!(sides[0] * sides[1] > 0.0, "the copy joins faces on opposite sides of the line: {sides:?}");
        }
        // SMLib's history of the tangent line goes to both copies.
        assert_eq!(order.pieces.len(), 2);
        let back = Brep::from_record(&record, true).unwrap();
        let check = back.check(CheckLevel::All).unwrap();
        assert!(check.is_valid(), "{check}");
        let want = 31.0 * 29.0 * 10.0 - PI * 25.0 * 10.0;
        let got = back.mass_properties(MEASURE_ACCURACY).unwrap().volume;
        assert!((got - want).abs() < 1e-6 * want, "volume {got} vs {want}");
    }

    /// A blind hole from z = 2 tangent to the block's side y = 20 from inside: the side is one
    /// face on both sides of the tangent line (joined below the hole), so pairing the faces round
    /// the line across the material would give it two coincident copies of the line, which
    /// SMLib refuses. The line is then copied across the empty sectors: the side uses one copy
    /// twice (a crack, as SMLib keeps a face meeting itself), the hole's two halves the other.
    #[test]
    fn a_blind_hole_tangent_to_a_side_leaves_the_side_a_crack() {
        let block = Brep::make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
        let hole = Brep::make_cylinder([10.0, 15.0, 2.0], 5.0, 9.0).unwrap();
        let r = block.boolean(&hole, crate::BooleanOp::Subtract).unwrap().unwrap();
        let (record, _) = r.to_native_ordered().unwrap();
        assert!(record.edges.iter().all(|e| e.uses.len() == 2));
        record.check_geometry().unwrap();
        let on_line = |e: &b::Edge| {
            e.vertices.iter().all(|v| {
                let p = record.vertices[*v as usize].point;
                (p[0] - 10.0).abs() < 1e-9 && (p[1] - 20.0).abs() < 1e-9
            })
        };
        let copies: Vec<&b::Edge> = record.edges.iter().filter(|e| on_line(e)).collect();
        assert_eq!(copies.len(), 2);
        let kinds: Vec<(bool, bool)> = copies
            .iter()
            .map(|e| {
                let faces: Vec<&b::Face> = e.uses.iter().map(|u| &record.faces[u.face as usize]).collect();
                let side = |f: &b::Face| matches!(f.surface, b::Surface::Plane { .. });
                (faces.iter().all(|f| side(f)), e.uses[0].face == e.uses[1].face)
            })
            .collect();
        assert!(kinds.contains(&(true, true)), "one copy is the side's crack: {kinds:?}");
        assert!(kinds.contains(&(false, false)), "the other joins the hole's halves: {kinds:?}");
        let back = Brep::from_record(&record, true).unwrap();
        let check = back.check(CheckLevel::All).unwrap();
        assert!(check.is_valid(), "{check}");
        let want = 6000.0 - PI * 25.0 * 8.0;
        let got = back.mass_properties(MEASURE_ACCURACY).unwrap().volume;
        assert!((got - want).abs() < 1e-6 * want, "volume {got} vs {want}");
    }

    /// A 100 x 60 x 30 block less a loft from its whole 100 x 60 outline at z = 20 (flush with its
    /// sides) down to a 10 x 10 pad at z = 11: the slab above z = 20 is cut loose, touching the
    /// rest along the outline's four edges round the pocket the loft leaves. SMLib keeps the
    /// pocket as a void of its own, bounded by the slab's underside and the loft's faces, so each
    /// solid's one boundary faces both the void and the space round the solids. The record has
    /// the two solids' closed shells, each listed by its solid and by the infinite region (the
    /// pocket is part of the empty space once the solids' shared edges are split), and no void.
    #[test]
    fn solids_touching_round_a_pocket_have_one_shell_each_and_no_void() {
        use crate::sweeps::Curve;
        let block = Brep::make_box([0.0; 3], [100.0, 60.0, 30.0]).unwrap();
        let rect = |pts: [[f64; 3]; 4]| -> Vec<Curve> { (0..4).map(|i| Curve::Line { start: pts[i], end: pts[(i + 1) % 4] }).collect() };
        let pad = rect([[2.0, 2.0, 11.0], [12.0, 2.0, 11.0], [12.0, 12.0, 11.0], [2.0, 12.0, 11.0]]);
        let top = rect([[0.0, 0.0, 20.0], [100.0, 0.0, 20.0], [100.0, 60.0, 20.0], [0.0, 60.0, 20.0]]);
        let tool = Brep::loft(&[pad, top], 1, true).unwrap();
        let r = block.boolean(&tool, crate::BooleanOp::Subtract).unwrap().unwrap();
        assert!(r.check(CheckLevel::All).unwrap().is_valid());
        let arrays = r.arrays().unwrap();
        let types = arrays.ints.get(&crate::flat::REGION_TYPE).cloned().unwrap_or_default();
        assert_eq!(types.iter().skip(1).filter(|t| **t != 0).count(), 1, "SMLib keeps the pocket as a void: {types:?}");
        let record = r.to_native().unwrap();
        assert_eq!(record.shells.len(), 2);
        assert_eq!(record.regions.len(), 3);
        assert!(record.regions.iter().all(|g| !g.void));
        assert_eq!(record.regions[0].shells.len(), 2, "the infinite region lists both solids");
        record.check_geometry().unwrap();
        // V = 180000 - h/6 (A0 + 4 Am + A1), h = 9, Am = 55 x 35.
        let want = 180000.0 - 9.0 / 6.0 * (100.0 + 4.0 * 55.0 * 35.0 + 6000.0);
        let back = Brep::from_record(&record, true).unwrap();
        let check = back.check(CheckLevel::All).unwrap();
        assert!(check.is_valid(), "{check}");
        let got = back.mass_properties(MEASURE_ACCURACY).unwrap().volume;
        assert!((got - want).abs() < 1e-6 * want, "volume {got} vs {want}");
    }

    /// The variable round r1 -> r5 along a box's top back edge run on into the R5 round of its
    /// corner (ladder 07-fillets/012): its end trimmed by the R5 cylinder is a NURBS edge, whose
    /// interval SMLib gave 7e-15 before the first knot. The record's interval is the NURBS's own
    /// where it overshoots it by rounding, and the record validates; farther past it stays an error.
    #[test]
    fn a_nurbs_edge_interval_a_rounding_past_its_knots_is_moved_onto_them() {
        let block = Brep::make_box([0.0; 3], [60.0, 40.0, 20.0]).unwrap();
        let rounded = block.fillet(&[[60.0, 40.0, 10.0]], 0.1, 5.0).unwrap();
        let r = rounded.fillet_variable([27.5, 40.0, 20.0], 0.1, [0.0, 40.0, 20.0], 1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 5.0]).unwrap();
        let arrays = r.arrays().unwrap();
        let kinds = arrays.ints.get(&EDGE_CURVE_TYPE).cloned().unwrap_or_default();
        let counts = arrays.ints.get(&EDGE_NURBS_VERTEX_COUNT).cloned().unwrap_or_default();
        let orders = arrays.ints.get(&EDGE_NURBS_ORDER).cloned().unwrap_or_default();
        let forms = arrays.ints.get(&EDGE_NURBS_FORM).cloned().unwrap_or_default();
        let knots = arrays.reals.get(&EDGE_NURBS_KNOTS).cloned().unwrap_or_default();
        // Each NURBS block's first and last knots of its domain, in edge order.
        let (mut at, mut block_of) = (0usize, Vec::new());
        for (n, o) in counts.iter().zip(&orders) {
            let (n, o) = (*n as usize, *o as usize);
            block_of.push((knots[at + o - 1], knots[at + n]));
            at += n + o;
        }
        let mut nurbs = 0usize;
        let mut edge = None;
        for (e, kind) in kinds.iter().enumerate() {
            let has_block = *kind == CURVE_NURBS || forms.get(e) == Some(&1);
            if *kind == CURVE_NURBS && edge.is_none() {
                edge = Some((e, block_of[nurbs]));
            }
            nurbs += usize::from(has_block);
        }
        let (e, (first, last)) = edge.expect("the round's end on the cylinder is a NURBS edge");
        let shifted = |by: f64| {
            let mut a = arrays.clone();
            a.reals.get_mut(&EDGE_RANGE).unwrap()[2 * e] = first - by;
            a
        };
        let (record, _) = native(&shifted(7e-15)).unwrap();
        record.validate().unwrap();
        assert!(record.edges.iter().any(|x| x.interval[0] == first && x.interval[1] <= last), "an interval on the NURBS's first knot");
        assert!(native(&shifted(1e-3)).map(|(r, _)| r.validate()).map_or(true, |v| v.is_err()), "a real overshoot is not moved");
    }

    #[test]
    fn hostile_arrays_are_errors_never_panics() {
        let good = Brep::make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap().arrays().unwrap();
        assert!(native(&good).is_ok());
        assert!(native(&Arrays::default()).is_err());
        let ids: Vec<i32> = good.ints.keys().copied().collect();
        for id in ids {
            for edit in 0..4 {
                let mut f = good.clone();
                let a = f.ints.get_mut(&id).unwrap();
                match edit {
                    0 => a.clear(),
                    1 => a.iter_mut().for_each(|x| *x = 1 << 40),
                    2 => a.iter_mut().for_each(|x| *x = -1),
                    _ => a.push(7),
                }
                let _ = native(&f);
            }
        }
        let ids: Vec<i32> = good.reals.keys().copied().collect();
        for id in ids {
            for x in [f64::NAN, f64::INFINITY, 1e300, 0.0] {
                let mut f = good.clone();
                f.reals.get_mut(&id).unwrap().iter_mut().for_each(|v| *v = x);
                let _ = native(&f);
            }
        }
    }
}
