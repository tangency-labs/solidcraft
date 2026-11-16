# blend.fillet_executive_p3: distant rim mistaken for endpoint band overlap (2026-11-06)

Issue: #2903. PR: #2924. Ordinary SolidCraft operation from its kernel update #12413.

The captured 60×40×30 shelled box has wall 3, outer vertical rounds R6, inner rounds R3,
and an inner floor round R1. The final inner-top-rim round/chamfer size is 0.47.
The actual application operand passes full integrity checks. Its symmetric mirror
seeds should build the same closed body (35 faces, 80 edges, 48 vertices), but one
seed returns `NoRoom/BandsOverlap` while its mirror completes. Both main `513e6fcfc`
and candidate `9856ffee5` reproduce with a fresh Core kernel importing the captured
SMB. Clearing application round history does not change it. An ops reconstruction
of the recipe does not reproduce; it has different geometry representations.

## Root cause

`FilletSession::geometry_bands_clear_other_rails` intends to exclude projections
onto source-edge endpoints: they are beside the band rather than across it.
It checks only whether the returned parameter is strictly within the edge interval.
The constrained closest-point solve can return an endpoint one parameter ulp inside
that interval. The following chord test then treats a distant point as inside an
unbounded strip instead of excluding the endpoint projection.

For the first rejection, edge B has interval `[2.6532560294479715, 2.713865182121104]`
and returned parameter `2.7138651821211037`. Its evaluated closest point is exactly
`E=(3,34,30)`, the edge endpoint. The rail endpoint is `R=(2.53,34,30)`. The tested
point is `P=(2.544036467113312,5.688205100494965,30)`, 28.311794899505 mm away across
the face. Its projection along the width-0.47 chord is 0.0140364671133, between the
existing margins, so the operation is falsely refused. The mirror's rounding of the
endpoint parameter changes the classification.

Recognize projected endpoints in geometric space using the existing maximum solver
approximation tolerance, before the chord-width test. No parameter epsilon, new
flag, retry, reordered chain, altered band margin or relaxed output assertion is
introduced. The guard remains a sampled test (15 points on each rail), not a complete
intersection proof; endpoint neighborhoods are treated within the same tolerance.

## Independent expected volumes

Let `A=1−π/4`, `M=5/6−π/4`, and the inner rounded-rectangle perimeter
`L=152+6π`. Integrating the cross section and its first moment against the parallel
curve Jacobian `1±κu`, with total turning `2π`, gives:

- Base: `60*40*30 −54*34*27 −4*A*6²*30 +4*A*3²*27 +L*A −2π*M`.
- Rim round removal: `L*A*r² +2π*M*r³`.
- Rim chamfer removal: `L*d²/2 +π*d³/3`.

At size 0.47 the expected round volume is 21737.746007562786 and chamfer volume
21726.89743733982 mm³. The regression checks each of the four mirror seeds against
these closed forms at relative 1e-9, topology counts, manifold status and Level2
integrity. Each starts from a fresh input before any measurement/check warms it.

Literature read: Patrikalakis, Maekawa and Cho, *Shape Interrogation for Computer
Aided Design and Manufacturing*, §11.2.1 (offset derivative `1+κd`) and §5.4.3
(point/procedural-curve minimization, including separate endpoint checks):
https://web.mit.edu/hyperbook/Patrikalakis-Maekawa-Cho/node219.html and
https://web.mit.edu/hyperbook/Patrikalakis-Maekawa-Cho/node69.html.
A constrained boundary minimum is distinct from the interior closest-point
condition. Geometric endpoint recognition preserves that distinction independently
of curve parameterization. These sources do not prescribe this specific fix.

Validation results belong in the PR description. The captured input and regression
are `crates/solidcore/tests/data/issue_2903_shelled_box_rim.smb` and
`crates/solidcore/tests/issue_2903_mirrored_shell_rim.rs`.


## Second design and operation corpus

The application Kernel APIs also reconstruct `typical/filleted-shelled-box` after its
height changes from 30 to 33: centered 70×50, outer R6 vertical rounds, then top-open
inward shell 2, leaving inner R4 corners. The original matrix size, measured from
the original display polylines, remains 0.627. This is a geometry control for an
edit, not a test of application timeline naming/reference resolution.

The captured edited operand has 19 faces, 48 edges and 32 vertices. On pristine
main, two mirrored corners refuse both round and chamfer; corrected Core builds all
four, with 27 faces, 64 edges and 40 vertices, manifold status and Level2 checks.
Its base volume is `(70*50−4*A*6²)*33−(66*46−4*A*4²)*31`; use the same top-rim
removal formulas with `L=192+8π`. No floor-round term is present. Expected round
volume is 20771.58921103918 and chamfer volume 20747.043401874635 mm³.

The four `fillets/2903-{original,edited}-rim-{round,chamfer}` corpus entries replay
these ordinary operations through the unchanged public ops runner. All four refuse
with no room on pristine main; all four build with this correction, with maximum
relative volume deviation 7.9e-12. Counts and expected volumes are analytic; no
reference result or known-failure entry is repinned. Each includes its actual
corrected-operation render. The native symmetry test additionally exercises every
mirrored seed, each on a fresh input.


## Additional archived-operand controls

A cached headless CLI identified as application source
`74e059674d32744b51f69a6ea7298ea29322fad9` evaluated `59-enclosure-base`,
`sensor_enclosure` and `pi-case`, plus the latter after its first driving parameter
`board_w` changed from 85 to 93.5 mm. Its kernel revision is unverified: these are
additional geometry controls, not the exact application-pin reproduction above.
The cache's official record decoder, structural validation and built-record
validation accept the extracted operands; Core's full integrity check also passes.

Replaying the same records on pristine main and this correction, all four mirror
seeds of each round/chamfer use the same tangent contour. Enclosure and Sensor Case
each move from six successes and two no-room refusals to eight successes. Pi's
original and edited operands already pass all eight operations on pristine main:
they do not reproduce its reported timeline failure. All 32 corrected controls
pass full integrity checks and retain identical mirrored face/edge/vertex counts. Per contour and
operation, mirrored volumes spread by at most 5.7e-14 relative. These measurements
are consistency checks, not new expected-volume goldens or proof that every reported
application action is fixed. In particular, Pi's actual failing timeline action
still needs reproduction or verification by the application matrix.
