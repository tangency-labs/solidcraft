# Primitive creation fixture records

A case is `{ "op": N, "data": [...] }`. The checked FFI wrapper validates the
entire record before either engine runs. A curve block contains seven numbers:
`[kind, x, y, z, a, b, c]`. Kind 0 builds a real B-spline line from xyz to abc;
kind 1 builds a quadratic B-spline circular arc centred at xyz, radius a and
increasing angles b/c in degrees. These are the actual kinds used by the upstream
file-local helpers, not replacements for analytic topology fixtures.

| op | data |
|---:|------|
| 0 | `tag analytics clear copy`, then four maps of four target indices; `-1` means absent, `0..3` are stable fixture topology identities. The native sweep starts with two stale entries. |
| 1 | first curve, second curve, tolerance |
| 2 | `closed periodic extension_distance count`, then the original path curves |
| 3 | `profile_count existing_output_count tolerance`, path curve, profiles, existing output curves |
| 4 | primitive selector, seven parameters, frame origin XYZ, X axis XYZ, Y axis XYZ, initial edit flag, initial map flag |
| 5 | curve, rotation-axis base XYZ, axis direction XYZ |
| 6 | boolean operation, initial result flag, edit A, edit B, face count A, face count B, tolerance, then A and B face blocks |

Primitive selectors are 0 static rectangle, 1 static circle, 2 box, 3 cylindrical
box, 4 full cone, 5 full sphere, 6 full torus, 7 pole-free sphere, 8 elliptic-ended
cone, 9 curve sweep and 10 empty planar boolean. Parameters follow the
corresponding upstream signature; parameter 7 is the static primitive tolerance,
pole-free sphere tolerance or cap flag. For an empty boolean, parameter 1 is its
operation and parameter 6 seeds an already-populated result slot.

Maps compare counts and fixed-order lookups rather than native hash iteration.
Curve outputs compare their natural intervals and position/first derivative at
fractions 0, 1/4, 1/2, 3/4 and 1. Primitive records compare actual statuses, edit/map
state and topology counts where Rust reaches a defined result. A Rust `Unported`
classifies the whole case as deferred; native-only geometry/topology is not parity.

The full-span cylindrical box is rejected by the exact native three-point-circle
preflight (UB-57). Rejected inputs never count as passes. The resource/geometry
limits of these native fixture families are documented in the checked FFI module.

`overflow.json` retains eight fuzz regressions. Four profile placements preserve
X after `MakeUnitOrthoVectors` writes it and subsequently fails. Four two-point
G1 extensions are rejected before NLib's failed-normalization fallback reads a
third control point outside its array. The preflight follows the original join
and closed/periodic decisions, so huge lines that require no extension remain
accepted and compared in the two additional witnesses.

Selector 8 may append rear X tilt (degrees) at index 19; older records default
it to zero. Selector 9 may append a curve count at index 19 and that many curve
blocks. The sweep consumes its native input curves even on failure.

For op 6, each face block is `[kind, x_tilt_degrees, z_offset, u0, v0, u1, v1]`:
kind 0 is a canonical plane, kind 1 a unit sphere (Rust retains its analytic
surface-of-revolution base, the available `Surface` variant). Planes are tilted around X;
A and B may each have zero to four faces. Rust builds face/surface records for
these validation paths; C++ builds trimmed native faces. A working sphere block
is `[1, 0, 0, 0, 0, 1, 1]` (UV domain `[0, 0, 1, 1]`), as used by
`boolean_nonplanar_first` and `boolean_nonplanar_second` in `review.json`.
Other domains, such as `[0, -45, 90, 45]`, can be rejected by native face
construction before `Boolean2D` runs. The compared record
contains status, restored edit flags and result-slot presence, without claiming
that the two fixture topologies are equivalent. `review.json` covers non-planar
first/later faces, plane offsets around twice the zone tolerance, normal tilts
around the 0.0001-degree limit (including the 0.01-degree regression), multiple
coplanar faces and empty operands. Coplanar cases reach surface coverage then
defer at intersection, so the differential result does not certify their
coverage mutations or the downstream boolean. A kernel unit test pins the
angular acceptance/rejection and restoration of caller state.

The same review file varies rear X tilt independently and supplies a line, full
circle, open arc and skew line to the linear sweep with/without caps and signed
or zero sweep distances. Deferred construction is listed separately in reports.

Four negative-distance full-circle sweeps reproduced a native nontermination in
the analytic-cylinder branch at c9c5979 (HANG-13). SMLib 1.0.1 sweeps a negative
distance as a positive one against the direction, so they are compared like the
other sweeps.

For selector 9, parameter 6 selects real analytic `SmLine`/`SmCircle` objects
when nonzero; zero keeps the B-spline line/circle builders. Rust uses the matching
Line/Ellipse variants. Both families include full circles and open arcs, so the
`IsAnalytic` STEP-interval choice and the copy/extrusion branches are reached.
Their missing face construction still makes those cases deferred, not geometry
parity.
