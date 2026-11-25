"""In-scope SM_API entry points for the solidcore port (agent B plan).

Each group maps to regexes matched (fullmatch) against qualified names of functions *defined* in
upstream/source/SM_API/src. Overloads are all included. Groups are also used to attribute
upstream tests to units.
"""

ENTRY_GROUPS = {
    "context": [r"SmApiCreateContext", r"SmApiGetOrCreateContext"],
    "primitive": [
        r"SmApiCreate(Plane|PlanarCircle|Box|Sphere|SphereNoPole|Cone|ConeEllipticEnds|Cylinder|"
        r"CylindricalBox|Torus|Pyramid|PartialCone|PartialCylinder|PartialSphere|PartialTorus|"
        r"SwungPrimitive|BlendPrimitive)",
        # simple profile/path curves needed as inputs to sweeps/lofts (no point fitting)
        r"SmApiCreate(LineSegment|Line|Circle|Arc|Ellipse|Rectangle|RegularPolygon|Helix|Curve|CanonicalCurve)",
    ],
    "boolean": [
        r"SmApiBoolean", r"SmApiBoolean(Union|Difference|Intersection|Merge|WithOptions|Lists|Trees)",
        r"SmApiMergeBreps",
    ],
    "fillet": [
        r"SmApiFilletEdges", r"SmApiFilletEdgesPerEdge", r"SmApiVariableRadiusFillet",
        r"SmApiSetBevelCorners", r"SmApiSurfaceSurfaceFillet", r"SmApiChamferFillet",
        r"SmApiCircularFillet",
    ],
    "offset": [r"SmApiOffsetBrepFull", r"SmApiShellBrepFull"],
    "sweep": [
        r"SmApiCreate(LinearSweep|RotationalSweep|DraftSweep|PipeSweep|TaperExtrude|SweepAlongPlanarPath|"
        r"LinearSweepWithRepetitions|RotationalSweepWithRepetitions|CurveSweep|CurveSweepFromFaces|"
        r"CurveSweepFromEdges)",
        # lofts / skins
        r"SmApiCreateSkinPrimitive", r"SmApiCreateSkinFromFaces", r"SmApiCreateSkin",
        r"SmApiCreate(Extrude|SurfaceRevolution|Sweep|RuledSurface)",
    ],
    "heal": [
        r"SmApiStitch", r"SmApiStitchAdvanced", r"SmApiStitchIntoShell", r"SmApiStitchIntoSolid",
        r"SmApiSimpleFaceStitch", r"SmApiUnifyNormals", r"SmApiHealBrep",
    ],
    "mass": [
        r"SmApiBrepComputeVolume", r"SmApiBrepComputeArea", r"SmApiBrepComputeMassProperties",
        r"SmApiFaceComputeArea", r"SmApiFaceComputeCentroid", r"SmApiEdgeComputeLength",
    ],
    "query": [
        r"SmApiBrepBoundingBox", r"SmApiFaceBoundingBox", r"SmApiEdgeBoundingBox",
        r"SmApiBrepClosestPoint", r"SmApiBrepClosestPointAll", r"SmApiFaceClosestPoint",
        r"SmApiEdgeClosestPoint", r"SmApiCurveClosestPoint", r"SmApiCurveClosestPointAll",
        r"SmApiSurfaceClosestPoint", r"SmApiGetClosestPoint",
        r"SmApiFaceClassifyUV", r"SmApiEdgeClassifyParameter", r"SmApiEdgeParameterRange",
        r"SmApiEdgeTangent", r"SmApiFaceInternalPoint", r"SmApiVertexGetPoint",
        r"SmApiBrepIsManifoldSolid", r"SmApiBrepRelationship", r"SmApiBrepDistance",
        r"SmApiFaceGetBoundaryLoops", r"SmApiGetEdges", r"SmApiGetFaces", r"SmApiFindEdge",
        r"SmApiFindFaces",
        r"SmApiEvaluateSurface(Point|Normal|Derivatives)", r"SmApiEvaluateCurve",
    ],
    "copy_transform": [
        r"SmApiBrepCopy", r"SmApiTranslate", r"SmApiRotate", r"SmApiScale", r"SmApiScaleByPt",
        r"SmApiTransform",
    ],
}

# Kernel (non-SM_API) roots. The upstream tests and pt_TestFiles fixtures load native .smb/.brp
# files through SmBrep::ReadFromFile, so the native reader is needed to feed difftests.
KERNEL_ROOTS = {
    "fixture_io": [r"SmBrep::ReadFromFile"],
}

# Explicitly out of scope even if a regex above would match.
EXCLUDED_ENTRY = set()

# SM_API functions deliberately left out (documented in plan/closure.md).
OUT_OF_SCOPE_ENTRY = [
    "SmApiTessellate", "SmApiTessellateBoundaries", "SmApiPoly*", "SmApiDraw",
    "SmApiReadBrepFromFile", "SmApiReadPartFromFile", "SmApiWriteBrepToFile",
    "SmApiBoolean2d", "SmApiBooleanWithCurves", "SmApiNonManifoldBoolean", "SmApiPiecewiseMerge",
    "SmApiNonManifoldSweep", "SmApiNonManifoldRotationalSweep", "SmApiFilletPreview",
    "SmApiRemoveFillet", "SmApiCut", "SmApiProjectAndTrim", "SmApiProjectBrepOntoPlane",
    "SmApiProjectCurve", "SmApiCreateSilhouetteCurves", "SmApiTurnToNurbs",
    "SmApiCreateOffsetProfile", "SmApiBrepMaterialCensus",
    "SmApiIntersect*", "SmApiCreateSurface*", "SmApiCreateOffsetSurface",
    "SmApiCreateCurveApproxPoints", "SmApiCreateCurveInterpPoints", "SmApiConvertToLinesAndArcs",
    "SmApiDropCurveToSrf", "SmApiLiftUVCurveFromSrf", "SmApiMakeCurvesCompatible", "SmApiOffsetCurve",
    "SmApiOrderCurves", "SmApiRemoveCurveKnots", "SmApiEvaluateContinuity",
    "SmApiCreatePlanarFaces", "SmApiTrimProjectParallel", "SmApiTrimSurfaceWith3dCurves",
]
