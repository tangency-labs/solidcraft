// Generated from main's replay of this test (Level 2 check of main, PR #21463):
// each result body's failure on main, its edge ids written `EdgeId(_)`, and how often.
const MAIN_FAILURES: [(&str, &str, usize); 29] = [
    (
        "issue_2176_tabbed_plate_tab_side_r0_5",
        "Level 2: SmBSplineCurve::Test #11 Bad SpeedPt: 2D BSplineCurve Warning: Interior 1st Deriv too slow, less than AvgSpeed/10000 (often an almost kink or almost singular point - never good practice) (offset=2.5724395e-12, tolerance=6.6664096e-05) [owner SmBSplineCurve]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        "Level 2: SmBSplineCurve::Test #11 Bad SpeedPt: 2D BSplineCurve Warning: Interior 1st Deriv too slow, less than AvgSpeed/10000 (often an almost kink or almost singular point - never good practice) (offset=4.3325182e-08, tolerance=5.2022959e-05) [owner SmBSplineCurve]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        "Level 2: SmBSplineCurve::Test #13 Bad CurveCusp: 2D BSplineCurve Warning: Extremely small radius of curvature (often an almost kink point - sometimes what's desired) (offset=2.3145701e-20, tolerance=0.0074535286) [owner SmBSplineCurve]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        "Level 2: SmBSplineCurve::Test #15 Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints (causes internal point singularity) (tolerance= 1e-06) [owner SmBSplineCurve]",
        3,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        "Level 2: SmFace::Test #36 Face Near missing Seam: Face Surface Seam has suspicious Edge near seam [owner SmFace]",
        3,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        "edge EdgeId(_) (ellipse) off a face's surface: [Some(false), Some(true)]",
        2,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        "edge EdgeId(_) (line) off a face's surface: [Some(false), Some(true)]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        "Level 2: SmBSplineCurve::Test #11 Bad SpeedPt: 2D BSplineCurve Warning: Interior 1st Deriv too slow, less than AvgSpeed/10000 (often an almost kink or almost singular point - never good practice) (offset=4.3325182e-08, tolerance=5.2022959e-05) [owner SmBSplineCurve]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        "Level 2: SmBSplineCurve::Test #13 Bad CurveCusp: 2D BSplineCurve Warning: Extremely small radius of curvature (often an almost kink point - sometimes what's desired) (offset=2.3145701e-20, tolerance=0.0074535286) [owner SmBSplineCurve]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        "Level 2: SmBSplineCurve::Test #15 Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints (causes internal point singularity) (tolerance= 1e-06) [owner SmBSplineCurve]",
        3,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        "Level 2: SmFace::Test #36 Face Near missing Seam: Face Surface Seam has suspicious Edge near seam [owner SmFace]",
        3,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        "edge EdgeId(_) (ellipse) off a face's surface: [Some(false), Some(true)]",
        2,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        "edge EdgeId(_) (line) off a face's surface: [Some(false), Some(true)]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        "Level 2: SmBSplineCurve::Test #11 Bad SpeedPt: 2D BSplineCurve Warning: Interior 1st Deriv too slow, less than AvgSpeed/10000 (often an almost kink or almost singular point - never good practice) (offset=4.3325182e-08, tolerance=5.2022959e-05) [owner SmBSplineCurve]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        "Level 2: SmBSplineCurve::Test #13 Bad CurveCusp: 2D BSplineCurve Warning: Extremely small radius of curvature (often an almost kink point - sometimes what's desired) (offset=2.3145701e-20, tolerance=0.0074535286) [owner SmBSplineCurve]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        "Level 2: SmBSplineCurve::Test #15 Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints (causes internal point singularity) (tolerance= 1e-06) [owner SmBSplineCurve]",
        3,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        "Level 2: SmFace::Test #36 Face Near missing Seam: Face Surface Seam has suspicious Edge near seam [owner SmFace]",
        3,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        "edge EdgeId(_) (ellipse) off a face's surface: [Some(false), Some(true)]",
        2,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        "edge EdgeId(_) (line) off a face's surface: [Some(false), Some(true)]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        "Level 2: SmBSplineCurve::Test #11 Bad SpeedPt: 2D BSplineCurve Warning: Interior 1st Deriv too slow, less than AvgSpeed/10000 (often an almost kink or almost singular point - never good practice) (offset=4.3325182e-08, tolerance=5.2022959e-05) [owner SmBSplineCurve]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        "Level 2: SmBSplineCurve::Test #13 Bad CurveCusp: 2D BSplineCurve Warning: Extremely small radius of curvature (often an almost kink point - sometimes what's desired) (offset=2.3145701e-20, tolerance=0.0074535286) [owner SmBSplineCurve]",
        1,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        "Level 2: SmBSplineCurve::Test #15 Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints (causes internal point singularity) (tolerance= 1e-06) [owner SmBSplineCurve]",
        5,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        "Level 2: SmFace::Test #36 Face Near missing Seam: Face Surface Seam has suspicious Edge near seam [owner SmFace]",
        3,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        "edge EdgeId(_) (ellipse) off a face's surface: [Some(false), Some(true)]",
        2,
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        "edge EdgeId(_) (line) off a face's surface: [Some(false), Some(true)]",
        1,
    ),
    (
        "sweep5-nurb_box1-all-r0.5",
        "Level 2: AssertValid skipped: ValidatePointers failed",
        1,
    ),
    (
        "sweep5-nurb_box1-all-r0.5",
        "Level 2: SmBrep::Test #5 Tolerance: Face tolerance relative to brep tolerance [owner SmFace]",
        15,
    ),
    (
        "sweep5-nurb_box1-all-r0.5",
        "Level 2: SmBrep::Test #59 Tolerance: Edge tolerance relative to brep tolerance (offset=0.020938086, tolerance=  0.01) [owner SmEdge]",
        36,
    ),
    (
        "sweep5-nurb_box1-all-r0.5",
        "Level 2: SmBrep::Test #82 Tolerance: Vertex tolerance is too wide to excuse by clearance (offset=0.020938086, tolerance=  0.02) [owner SmVertex]",
        18,
    ),
];
