# Nx1 rail endpoint orientation default (2026-10-24)

The actual combined two-corner component caught six topology differences that
vertex positions alone did not expose: the new Nx1 producer passed `Same` to
MakeRailEdgeuse, while native omits the argument and defaults to `Unknown`
(SmFilletGeom.h:604; SmFilletCorner.cpp:6092). Both endpoint uses were inserted at
the front of the rail list, replacing the observed first endpoint. The producer
now passes Unknown so the existing helper derives orientation from the actual
original corner vertex. No shared edge-insertion algorithm or harness field changed.
The same seven retained native/Rust fixtures now give six exact passes, zero
failures and one later full-API deferral; all four vertex records and full topology
agree, maximum relative error zero. The first failing comparison is retained as
evidence that endpoint orientation/list order checks detect this regression.

### Real two-corner fillet stage catches forced rail orientation (2026-10-24)

`real-box-corner-vertices-component-actual-session` runs the shared production
corner loop and its native counterpart, with smoothing omitted on both sides.
All four actual vertex geometries and flags matched, but six directed rail
integers differed: the Nx1 producer passed `Same` where the native omitted
argument defaults to `Unknown` (SmFilletCorner.cpp6092; SmFilletGeom.h604).
This forced both ends to insert at the front of each rail. Using the existing
orientation inference fixes the identical fixture with every field retained.
Seven real-box cases now give six exact value passes and one full-API deferral.
The earlier direct vertex checkpoint now passes through the actual borrowed
offset-surface provider. No finished fillet or mass parity is claimed.

Full p3 after this component: 298 cases, 251 value passes, 47 deferred, zero
failures, max_rel_err=0. The remaining deferrals belong to executive p1.

### Retained corner-stage capability boundary

The shared Session corner-vertex component currently dispatches ordinary `NByOne`
corner topology/geometry. Other valid concrete corner contexts and unsupported
concrete rail-solver contexts return `Unported(blend.fillet_executive_p1)` at the
live virtual-dispatch seam; they are not converted into an ordinary native fillet
failure. The explicitly named direct Nx1 helpers still validate their own type.
This slice makes no cylinder, multi-edge, variable-radius or complete API claim.

### Real-box untrimmed fillet centerline loses its native Line subtype (2026-10-24)

An additive actual-session checkpoint retains the original 10×20×30 box, edge0 and radius0.5 through real preparation, corner creation, smooth prelude, shared corner vertices, all adjustment and direct first solver CalcFilletGeom. All six statuses and all sampled geometry agree (max relative error2.775557561562892e-16), but native centerline GetType is SmLine14010 while Rust reports BSpline14033. The only mismatch is ints.generated_geometry_state[15]; surface and both rail runtime types already agree. Native SmFilletSolver.cpp633–637 retains the derived line pointer; Rust solver/p2.rs345 slices it via bspline_part into FilletGeom's BSplineCurve slot. Preserve the actual curve representation; do not normalize the type comparison. Full executive_p3 baseline:300 cases,252 passes,47 existing deferrals,1 failure. The post-adjustment case passes; all14 previous CPP/Rust real-box records are byte-identical. Public SmApiFilletEdges remains genuinely deferred.

Resolution of the real-box centerline subtype finding: retain the canonical actual Curve (including Line) in FilletGeom, use its virtual copy/evaluation/reversal operations, and preserve the legacy borrowed B-spline-base accessor for base-only callers. The new observer reads the actual curve. Same fixtures and enforced runtime types now give 300 executive_p3 cases,253 passes,47 existing deferrals,0 failures,max_rel_err=0; perturb253/253 caught. Seven affected native units total4032 cases,3864 passes,136 existing deferrals,32 defined rejections,0 failures. All3732 existing affected-unit Rust-only per-case digests and all14 previous real-box CPP/Rust output records are unchanged. Full SmApiFilletEdges still genuinely defers; this proves retained untrimmed geometry, not a completed fillet.

### Actual fillet corner boundary curves lose Circle subtype in the SSI adapter (2026-10-24)

The original box/edge/radius case now runs shared all-solver geometry followed by each corner's tangent adjustment, repeated vertex geometry and boundary-edge calculation. All seven stage statuses and sampled points agree exactly, but both native end ribs are Circle14021 and Rust stores plain BSpline14033. New corner_edge_geometry_state indices6 and18 expose the mismatch. Native retains actual derived curves from sm_SrfSrfIntersection. Rust geom/p2's SrfSrfCurves already retains Curve, but the distinct geom/p4 adapter279–324 holds Option<BSplineCurve> and clones only each returned object's base before p5 stores it. Preserve the actual returned curves through this adapter; do not remove type checks or change SSI arithmetic. Red executive_p3 baseline:301 cases,253 passes,47 existing deferrals,1 failure; all18 prior real-box CPP/Rust records are byte-identical. Full public filleting remains genuinely unported.

Resolution of the boundary-Circle finding: the private geom/p4 SSI carrier retains the actual Curve objects accepted by the same native B-spline cast filter; p4 virtual endpoint/reversal/store operations and p5 ownership transfer retain derived objects. The existing public base-only UvCurves API keeps its explicit compatibility projection. With the same fixture and unchanged observer, executive_p3 now has 301 cases,254 passes,47 existing deferrals,0 failures,max_rel_err=0; perturb254/254 caught. Six affected native suites:2615 cases,2431 passes,78 existing deferrals,106 defined rejections,0 failures. All2314 previous affected-unit Rust digests and all18 earlier CPP/Rust box records are unchanged. Full SmApiFilletEdges remains genuinely deferred; the next real component is rail trimming.
### Fillet rollover classification: retained body-qualified identities

The production Session's pseudo rail curves and target boundary topology can have
equal local arena IDs. The legacy solver-p5 fixture encodes pseudo curves using
offset edge IDs; that fixture convention is not a production identity resolver.
Rail classifications now retain explicit body and geometry-role handles, with a
separate Legacy namespace for indexed entrypoints. Legacy adapters preserve their
existing algorithms and outputs without applying that offset convention to Session.

The actual classification loan splits pseudo topology from its attached mutable
classification, preserving partial writes on ordinary and Unported errors without
geometry snapshots or owner rebinding. Scoped target-edge projections reject
pseudo and geometry handles. The canonical getter exposes scoped handles; indexed
input conversion is explicit and fallible. These changes do not complete missing
recovery notifications, custom producer-owner resolution, or the public fillet API.


### Paired retained fillet rail classification (2026-10-24)

The unchanged real box/edge/radius now runs enabled TestRollOver after shared TrimRails, then repairs the actual ordered adjusted-corner list. Native and Session both attach classifications to the real pseudo rail curves: each has one interval [0.25,0.75], boundary Target edges and a mid Target face (faces3/1). Rail0 endpoints classify on target edges8/5 at t29.5; rail1 on edges9/1 at t9.5. Actual adjusted-corner lists are empty. Target vertex/edge tolerances remain1e-5; classification remains enabled. DoingBoolean and CC DoingFillets both remain false in this direct component, matching its actual caller context; this does not claim the outer DoFilleting context scope.

The new checkpoint compares 432 exact fields and 282 values, max_rel_err=0, with no preclassification evaluation or lazy geometry lookup. Existing real-box CPP/Rust output records (22) and affected-unit Rust digests (1340, Rust-only retention) remain byte-identical. Five affected native suites total1643 cases,1583 value passes,47 existing deferrals,13 defined rejections,0 failures. Executive_p3:303 cases,256 passes,47 deferrals; perturb256/256 caught. Actual body/role identities replace no target/pseudo IDs with numeric offsets.

This ordinary fixture proves attached CC routing and expected Target Edge/Face identities, not general unexpected non-topology point identity or nonempty rollover repair. The observer preserves an explicit unknown marker for unsupported object roles. Recovery owner notifications and the full SmApiFilletEdges entrypoint remain genuinely Unported; no completed fillet or resultant mass parity is claimed.


### Paired intermediate fillet face construction (2026-10-24)

The unchanged box now continues through actual intermediate SmBrep allocation with target tolerance/editing, every corner CalcCornerGeom, then each solver's tolerance propagation and actual MakeFaceBrep. Both native corner predicates return normally; no corner-patch callback is fabricated. The separate intermediate body has4 vertices,4 edges,1 face,1 shell,1 region and8 directed edgeuses, tolerance4.0000000000000003e-5. Actual boundary objects are Line14010/Circle14021/Line14010/Circle14021; the face surface is Cylinder15012. No type normalization is used.

The new checkpoint compares564 exact fields and389 values, max_rel_err0, including ordered edge/loop topology, each edgeuse's startvertex/mate/radial/cw/ccw/face, fixed primary/mate faceuse orientations and shell/region labels, actual geometry domains/statuses/samples, and topology tolerances. All observations follow the actual construction operations; no attributes or stitching stage is skipped into the claim. Existing24 CPP/Rust real-box outputs and1805 affected-unit Rust-only case digests remain byte-identical. Four affected native suites total2109 cases,1878 value passes,132 existing deferrals,99 defined rejections,0 failures. Executive_p3:304 cases,257 value passes,47 existing deferrals; perturb257/257 caught.

The original full SmApiFilletEdges case still genuinely defers. This proves a real retained intermediate fillet face, not a completed target solid or mass properties. Actual attribute attachment, stitching and later target integration remain separate pipeline stages.


### Fillet source surface loses its consumed managed-face identity (2026-10-24)

A passive addition to the unchanged intermediate-face fixture exposes four genuine identity mismatches while every previous564 exact fields and389 values remain unchanged. Native source FG surface is the very surface consumed into intermediateface0 and owned by that Face(type16002); Rust retains an independent FG surface with owner=None, while its managed face owns a clone. New source_surface_binding indices3/4/5/6 are native1/16002/0/0 versus Rust0/0/-1/-2. No sampled geometry or runtime type is normalized or removed.

Native MakeFaceBrep passes m_pFilletSurface directly/consumed at SmFilletGeom.cpp9616/9639; its destructor9038–9054 does not delete that surface. DoFilleting.cpp2995 then resolves the first FG surface's owner as Face and calls ShrinkGeometry. A stale clone or guessed owner cannot replace this actual managed-surface mutation relationship. The owning kernel worker is implementing a producer-proven Owned→ManagedFace route; this red observer makes the gap visible. All26 previous CPP/Rust real-box records remain byte-identical after removing only the added source_surface_binding key for retention comparison. The new section is never removed from actual difftest comparison.
### Fillet intermediate stitching: live attributes and notifications

The intermediate body's ordered edges receive the executive's retained E attribute
before the existing StitchFaces algorithm runs. Its mutating helper calls now accept
the same borrowed notification records, attribute store and kernel callback context;
legacy entrypoints retain their existing adapters. The shared executive stage returns
counts and gaps only after success, preserving BAD_MERGE mutation on ordinary error.

The normal box-edge kernel probe retains the same four marker users and reports zero
stitched edges, four lamina edges and zero gaps. Paired native proof is pending. This
is not general stitch-mutation or complete fillet parity: a newly copied split curve
still requires its actual identity provider, and nested notification-context leases
remain a limitation for mutation paths that request fresh kernel geometry keys.
Separately, the face-stage native alias observer exposes a real FG-surface versus
managed-face ownership mismatch; the stitching adapter does not repair or hide it.

### Fillet surface installation: actual managed binding and indexed compatibility

Native MakeFaceBrep installs its original fillet surface into the generated face.
The real Session now transfers that surface through the shared p6/p7 construction
algorithm, retaining either the original owned value or its installed face on every
return. Partial face allocation alone does not imply installation. Native owner
restoration and topology-versus-late-error boundaries remain distinct.

The canonical FG slot is Owned or ManagedFace(actual body identity, FaceId).
Session::fillet_surface resolves the actual object; an owned-only getter reports
missing body context after transfer rather than silently returning None. The real
box test observes the same surface object and actual canonical face owner before
attribute attachment and stitching.

The indexed slice API retains its existing copied-surface construction backend,
including its prior cliff trimming route. It does not claim shared surface binding.
The retained-solver Session successor always uses original-surface transfer; its
cliff transfer through the trimming tool remains a genuine missing capability at
the actual final call. No fixture, tolerance, runtime type or ownership observation
is normalized to hide that boundary.
### Fillet consumed-surface binding fixed; actual attribute/stitch paired evidence

The unchanged same-box face/stitch checkpoints now resolve the original surface through its actual managed Face. Native and Rust source_surface_binding both equal [1,1,1,1,16002,0,0]; all four prior owner/alias mismatches are fixed without removing any fields. All26 older CPP/Rust box outputs remain identical outside that intentionally corrected binding section. Mode11 retains571 exact fields/389 values; Mode12 retains623 exact fields/391 values, all native values exact. E is the actual executive attribute with four ordered intermediate-edge users. Actual Stitch returns0 stitched,4 lamina and0 gaps. Whole executive_p3:305 cases,258 value passes,47 existing deferrals,0 failures; full API remains honestly deferred.

An affected-suite run caught five legacy cliff cases newly deferred by the initial transfer backend. The explicit indexed compatibility path now preserves their existing behavior: geom_p4 all400 cases pass, with no case/status/tolerance waiver. The actual retained Session path always transfers the original surface and still reports its unavailable cliff transfer honestly. This is ordinary same-box component evidence, not full filleting, target integration, shrink, mass or broad notification parity.

### Direct first-fillet-face shrink: native paired domain and geometry proof

After actual E attachment and Stitch, the unchanged ordinary box runs the shared first-FG-per-solver owner-face shrink component. Native and Rust canonical surface AND stored Face UV domains change from [0,0,1,1] to [0.25,0,0.75,1]. Raw edge intervals, actual Line/Circle types, source-to-face alias and all later samples remain compared. New checkpoint682 exact fields/439 values matches exactly; whole executive_p3 306 cases/259 value passes/47 existing deferrals/0 failures. All28 older CPP/Rust box outputs remain identical; perturb catches14/14 implemented box cases.

This is the direct component with unchanged false Boolean/fillet flags. Native actual surface PreEdit removes an existing local TrimSrfCache; nested generator, Surface and Face notifications, owner-clear/restore and temporary object lifecycle are independently observed but NOT claimed as Rust parity. The current numeric checkpoint does not erase these gaps or claim complete CreateFilletBrep/DoFilleting/public API parity.
