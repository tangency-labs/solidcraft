//! Clean-room ISO 10303-21 (STEP) reader for the AP203/AP214 B-rep subset.
//!
//! [`import`] parses a Part 21 file ([`part21`]) and maps its B-rep bodies
//! ([`map`]) to a neutral, plain-data [`Model`] ([`model`]): shells, faces with
//! orientation, loops of oriented edges, vertices, and analytic or B-spline geometry,
//! in millimetres with every placement resolved. A face that no shell references (a
//! hand-written or geometrically bounded sheet) becomes a one-face open-shell body, so
//! sheet-only files do not report "no B-rep bodies". [`flat::encode`] turns a body into
//! one `f64` array for the C++ side of the difftest, so both kernels build from the
//! same numbers. Nothing here depends on either kernel.
//!
//! Written from the public ISO 10303-21 file syntax and the ISO 10303-41/42 entity
//! definitions used by AP203 and AP214 only; no other STEP implementation was
//! consulted. Inputs are untrusted: see [`Limits`]. Unsupported entities make the
//! body that reaches them [`model::Skipped`] with `unsupported: <ENTITY>`; nothing
//! panics.

pub mod flat;
mod geom;
pub mod map;
pub mod model;
pub mod part21;
pub mod script;
pub mod validate;

pub use model::Model;
pub use part21::{Limits, ParseError};

/// Parses `bytes` and maps every B-rep body.
pub fn import(bytes: &[u8], limits: &Limits) -> Result<Model, ParseError> {
    // Some AP203 files use the Part 21 short entity names. The parser stores the raw
    // name, so normalize the handful of geometry entities needed by the mapper.
    const ALIASES: [(&str, &str); 11] = [
        ("FCSRF", "FACE_SURFACE"),
        ("FCOTBN", "FACE_OUTER_BOUND"),
        ("PLYLP", "POLY_LOOP"),
        ("CRTPNT", "CARTESIAN_POINT"),
        ("DRCTN", "DIRECTION"),
        ("A2PL3D", "AXIS2_PLACEMENT_3D"),
        ("CLSSHL", "CLOSED_SHELL"),
        ("BSSWK", "B_SPLINE_SURFACE_WITH_KNOTS"),
        ("BSCWK", "B_SPLINE_CURVE_WITH_KNOTS"),
        ("TRMCRV", "TRIMMED_CURVE"),
        ("CMCRSG", "COMPOSITE_CURVE"),
    ];
    let text = String::from_utf8_lossy(bytes);
    let mut normalized = text.into_owned();
    for (short, long) in ALIASES {
        normalized = normalized.replace(&format!("={short}("), &format!("={long}("));
    }
    let file = part21::parse(normalized.as_bytes(), limits)?;
    Ok(map::map_file(&file, limits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bound, Curve, Surface};

    /// A tetrahedron (0,0,0) (10,0,0) (0,10,0) (0,0,10), written by hand, in
    /// centimetres (so every length maps x10), plus a disk face with a circular edge.
    const TETRA: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('hand-written test'),'2;1');
FILE_NAME('tetra','2026-10-25T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=CARTESIAN_POINT('',(0.,1.,0.));
#4=CARTESIAN_POINT('',(0.,0.,1.));
#11=VERTEX_POINT('',#1);
#12=VERTEX_POINT('',#2);
#13=VERTEX_POINT('',#3);
#14=VERTEX_POINT('',#4);
#20=DIRECTION('',(1.,0.,0.));
#21=DIRECTION('',(0.,1.,0.));
#22=DIRECTION('',(0.,0.,1.));
#23=DIRECTION('',(-1.,1.,0.));
#24=DIRECTION('',(-1.,0.,1.));
#25=DIRECTION('',(0.,-1.,1.));
#30=LINE('',#1,#40);
#31=LINE('',#1,#41);
#32=LINE('',#1,#42);
#33=LINE('',#2,#43);
#34=LINE('',#2,#44);
#35=LINE('',#3,#45);
#40=VECTOR('',#20,1.);
#41=VECTOR('',#21,1.);
#42=VECTOR('',#22,1.);
#43=VECTOR('',#23,1.4142135623730951);
#44=VECTOR('',#24,1.4142135623730951);
#45=VECTOR('',#25,1.4142135623730951);
#50=EDGE_CURVE('',#11,#12,#30,.T.);
#51=EDGE_CURVE('',#11,#13,#31,.T.);
#52=EDGE_CURVE('',#11,#14,#32,.T.);
#53=EDGE_CURVE('',#12,#13,#33,.T.);
#54=EDGE_CURVE('',#12,#14,#34,.T.);
#55=EDGE_CURVE('',#13,#14,#35,.T.);
/* bottom z=0, normal -Z: 0 -> 2 -> 1 */
#60=EDGE_LOOP('',(#61,#62,#63));
#61=ORIENTED_EDGE('',*,*,#51,.T.);
#62=ORIENTED_EDGE('',*,*,#53,.F.);
#63=ORIENTED_EDGE('',*,*,#50,.F.);
#64=FACE_OUTER_BOUND('',#60,.T.);
#65=AXIS2_PLACEMENT_3D('',#1,#22,#20);
#66=PLANE('',#65);
#67=ADVANCED_FACE('',(#64),#66,.F.);
/* y=0, normal -Y: 0 -> 1 -> 3 */
#70=EDGE_LOOP('',(#71,#72,#73));
#71=ORIENTED_EDGE('',*,*,#50,.T.);
#72=ORIENTED_EDGE('',*,*,#54,.T.);
#73=ORIENTED_EDGE('',*,*,#52,.F.);
#74=FACE_OUTER_BOUND('',#70,.T.);
#75=AXIS2_PLACEMENT_3D('',#1,#21,$);
#76=PLANE('',#75);
#77=ADVANCED_FACE('',(#74),#76,.F.);
/* x=0, normal -X: 0 -> 3 -> 2 */
#80=EDGE_LOOP('',(#81,#82,#83));
#81=ORIENTED_EDGE('',*,*,#52,.T.);
#82=ORIENTED_EDGE('',*,*,#55,.F.);
#83=ORIENTED_EDGE('',*,*,#51,.F.);
#84=FACE_OUTER_BOUND('',#80,.T.);
#85=AXIS2_PLACEMENT_3D('',#1,#20,$);
#86=PLANE('',#85);
#87=ADVANCED_FACE('',(#84),#86,.F.);
/* slanted, normal (1,1,1): 1 -> 2 -> 3 */
#90=EDGE_LOOP('',(#91,#92,#93));
#91=ORIENTED_EDGE('',*,*,#53,.T.);
#92=ORIENTED_EDGE('',*,*,#55,.T.);
#93=ORIENTED_EDGE('',*,*,#54,.F.);
#94=FACE_OUTER_BOUND('',#90,.T.);
#95=DIRECTION('',(1.,1.,1.));
#96=AXIS2_PLACEMENT_3D('',#2,#95,$);
#97=PLANE('',#96);
#98=ADVANCED_FACE('',(#94),#97,.T.);
#100=CLOSED_SHELL('',(#67,#77,#87,#98));
#101=MANIFOLD_SOLID_BREP('tetra',#100);
/* a disk: one closed circular edge on a plane, an open shell */
#110=CARTESIAN_POINT('',(5.,0.,0.));
#111=VERTEX_POINT('',#110);
#112=CARTESIAN_POINT('',(4.,0.,0.));
#113=AXIS2_PLACEMENT_3D('',#112,#22,#20);
#114=CIRCLE('',#113,1.);
#115=EDGE_CURVE('',#111,#111,#114,.T.);
#116=ORIENTED_EDGE('',*,*,#115,.T.);
#117=EDGE_LOOP('',(#116));
#118=FACE_BOUND('',#117,.T.);
#119=PLANE('',#113);
#120=ADVANCED_FACE('',(#118),#119,.T.);
#121=OPEN_SHELL('',(#120));
#122=SHELL_BASED_SURFACE_MODEL('',(#121));
#200=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.));
#201=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));
#202=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-05),#200,'distance_accuracy_value','');
#203=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#202))GLOBAL_UNIT_ASSIGNED_CONTEXT((#200,#201))REPRESENTATION_CONTEXT('',''));
#204=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101,#65),#203);
#205=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#122),#203);
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn imports_hand_written_tetrahedron() {
        let m = import(TETRA.as_bytes(), &Limits::default()).unwrap();
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);
        assert_eq!(m.bodies.len(), 2);
        let t = &m.bodies[0];
        assert_eq!(t.step_id, 101);
        assert_eq!(t.unit_mm, 10.0);
        assert!((t.uncertainty_mm.unwrap() - 1e-4).abs() < 1e-18);
        assert_eq!(
            (
                t.shells.len(),
                t.faces.len(),
                t.edges.len(),
                t.vertices.len()
            ),
            (1, 4, 6, 4)
        );
        assert!(t.shells[0].closed);
        let v12 = t.vertices.iter().find(|v| v.step_id == 12).unwrap();
        assert_eq!(v12.point, [10.0, 0.0, 0.0]);
        // Edge #53 runs (10,0,0) -> (0,10,0) along a line of magnitude 10 sqrt 2.
        let e = &t.edges[t.edges.iter().position(|e| e.step_id == 53).unwrap()];
        assert!((e.t_start - 0.0).abs() < 1e-12 && (e.t_end - 1.0).abs() < 1e-12);
        let Curve::Line { magnitude, .. } = e.curve else {
            panic!()
        };
        assert!((magnitude - 10.0 * 2f64.sqrt()).abs() < 1e-12);
        let f = &t.faces[0];
        assert!(!f.same_sense);
        let Surface::Plane { frame } = f.surface else {
            panic!()
        };
        assert_eq!(frame.z, [0.0, 0.0, 1.0]);
        let Bound::Edges(es) = &f.loops[0].bound else {
            panic!()
        };
        assert_eq!(es.len(), 3);
        assert!(es[0].forward && !es[1].forward);
        // Every edge is used twice, once in each direction.
        for (i, _) in t.edges.iter().enumerate() {
            let uses: Vec<bool> = t
                .faces
                .iter()
                .flat_map(|f| match &f.loops[0].bound {
                    Bound::Edges(es) => es.clone(),
                    Bound::Vertex(_) => vec![],
                })
                .filter(|oe| oe.edge == i)
                .map(|oe| oe.forward)
                .collect();
            assert_eq!(uses.len(), 2);
            assert_ne!(uses[0], uses[1]);
        }
        // The disk: a closed circle edge spanning one period from angle 0.
        let d = &m.bodies[1];
        assert_eq!(d.kind, model::BodyKind::ShellModel);
        assert!(!d.shells[0].closed);
        let e = &d.edges[0];
        assert_eq!((e.start, e.end), (0, 0));
        assert!(e.t_start.abs() < 1e-12 && (e.t_end - 360.0).abs() < 1e-12);
        let Curve::Circle { radius, frame } = e.curve else {
            panic!()
        };
        assert_eq!(radius, 10.0);
        assert_eq!(frame.origin, [40.0, 0.0, 0.0]);
        // The flat encoding starts with its header and covers every vertex.
        let flat = flat::encode(t);
        assert_eq!(&flat[..7], &[21.0, 1.0, 0.0, 4.0, 6.0, 4.0, 1.0]);
        assert_eq!(&flat[7..10], &[0.0, 0.0, 0.0]);
        // The encoding round-trips through the validating decoder.
        for b in &m.bodies {
            let e = flat::encode(b);
            assert_eq!(flat::encode(&flat::decode(&e).unwrap()), e);
        }
        // Truncations, bad indices and non-finite numbers are refused.
        for cut in 0..flat.len() {
            assert!(flat::decode(&flat[..cut]).is_err());
        }
        let mut bad = flat.clone();
        bad[3] = 1e18;
        assert!(flat::decode(&bad).is_err());
        let mut bad = flat.clone();
        bad[8] = f64::NAN;
        assert!(flat::decode(&bad).is_err());
    }

    #[test]
    fn no_brep_and_garbage() {
        let m = import(
            b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=PRODUCT('a','b','',());ENDSEC;END-ISO-10303-21;",
            &Limits::default(),
        )
        .unwrap();
        assert!(m.bodies.is_empty() && m.skipped.is_empty());
        assert!(import(b"\x00\xff garbage", &Limits::default()).is_err());
    }
}
