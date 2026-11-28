//! The certified pass of the line/surface leaf solve: every transversal crossing of a line
//! with one leaf of a surface whose net is the surface, found with a proof that none is
//! left in the boxes it decides.
//!
//! Not upstream. The leaf solve of `SmLSIGlobalSolver::LocalSolve`
//! ([`super::p2::LsiGlobalSolver::local_solve_branch`]) solves a few guesses by Newton's
//! method and decides from the patch's normal map whether a second crossing can exist. That
//! map is no proof (a screw strip has no normal perpendicular to its axis, yet a line
//! parallel to the axis crosses it once a turn), and a guess that settles on one of two
//! crossings of a leaf loses the other: a ray that enters and leaves a torus near its crown
//! within one leaf was given its exit as the first hit. The guesses stay as they are (they
//! give today's numbers for the roots they find); this pass runs after them and adds what
//! they left.
//!
//! The method is interval Newton on `F(u, v, t) = S(u, v) − P − t D` over a scratch
//! subdivision of the leaf's parameter box (Toth, "On ray tracing parametric surfaces",
//! SIGGRAPH 1985; the tests are Krawczyk's operator as Moore and Rump state them: Rump,
//! "Verification methods", Acta Numerica 19 (2010), Thm 13.3, and
//! `docs/definitions-and-certificates.md` §4.3). Nothing of the surface cache is changed:
//! the boxes are plain parameter rectangles of the surface's own net
//! ([`crate::geom::nurbs_enclosure`]), enclosed in rounded intervals. For each box, in this
//! order:
//!
//! 0. **Cut at a knot line** inside it, so that every box enclosed lies on one polynomial
//!    piece of the net (a box whose enclosure is refused, weights not proved positive over
//!    it, is halved).
//! 1. **Excluded by distance** when its enclosure proves every point of the surface over it
//!    farther than the tolerance from the line. The tolerance is the leaf solve's own: a
//!    line passing within it is a hit there (`SmSurface::LocalCurveIntersect`), so a box is
//!    never dropped for merely missing the line. **Excluded by t** when its line parameters
//!    (the feet of its points, widened by the tolerance) lie wholly outside the line's own
//!    interval: the solve's line interval, and for a ray the parameters from just behind its
//!    start (the bound a ray's hit is skipped by). With neither, nothing is excluded by t.
//!    The leaf's interval is a slab test of rounded boxes, no proof, and excludes nothing.
//! 2. **Regular** when the contraction `q = ‖I − Y F′(X)‖` of Krawczyk's operator over the
//!    box is below 1: every matrix of the interval Jacobian is then nonsingular, `F` is
//!    injective on the box (`F(a) − F(b) = A (a − b)` with each row of `A` a gradient at its
//!    own point of the box, so `A` lies in the hull: the reason a normal map at single points
//!    proves nothing), and the box holds at most one root, with any line parameter (`F′`
//!    does not depend on `t`). Krawczyk's operator then decides: its image apart from the
//!    box, **no root**; strictly inside, exactly one. A root on the box's border passes
//!    neither test from inside: it is certified in a small box about it
//!    ([`crate::geom::krawczyk::krawczyk_region`], epsilon-inflation), and the box is
//!    decided once the hull of the two is regular too. A box with no root holds no
//!    interior local minimum of the gap to the line either (`[S_u S_v −D]` would be singular
//!    there), so the least gap over it lies on its border: it ends the search for roots, but
//!    it is no proof that the line passes farther than the tolerance.
//! 3. **Left** when it is neither and is at the floor: its image is no larger than the
//!    tolerance, it cannot be halved, or the cutting stops there because the line runs along
//!    the surface or touches it in a degenerate contact. That last is a box over which the
//!    surface lies within the tolerance of a plane through the line and the system of the
//!    line's fold on the surface, `f₁ = 0`, `∂(f₁, f₂)/∂(u, v) = 0` (`f₁`, `f₂` the
//!    distances from two perpendicular planes through the line, the first holding the
//!    surface normal at the box's middle), has no regular interval Jacobian over the box. A
//!    non-degenerate isolated contact (a tangent contact whose fold is regular), a near miss
//!    or the stretch between the two crossings of a shallow secant has one (the fold is a
//!    point there), and such a box is halved on, so that two crossings a hair apart are
//!    still certified one by one; along a line lying in the surface, or beside it, the
//!    determinant vanishes identically and no halving ever would decide the box. So does a
//!    degenerate isolated contact (a line tangent to a torus's crown circle: the
//!    determinant vanishes on two curves crossing at the contact), so a box left is no proof
//!    that the line lies in the surface. A box left holds what Krawczyk's test cannot decide
//!    (a tangent contact, a pole, an apex, a line in the surface); this pass reports nothing
//!    for it, and the guesses' answer for it stands as it was.
//! 4. **Halved** otherwise, across the parameter direction that carries the surface farther
//!    from the line over the box (the larger of `|S_u × D| Δu` and `|S_v × D| Δv`): seen
//!    along the line the halves are then about as wide as they are long, which is what
//!    makes a box regular, and a direction the line runs along is not cut for nothing. A
//!    direction in which the box's image is already within half the tolerance is at the
//!    floor's size and is not cut again (both there: step 3).
//!
//! A certified root is refined by Newton's method inside the box it is the only root of,
//! and reported only when it lies on the leaf's line interval and the gap `|S − L|` at the
//! reported point is itself enclosed within the tolerance; otherwise it is unshown.
//!
//! **What the pass decided.** Each pass ends in a [`Stop`]: not run (a line or domain
//! nothing can be proved of), finished (every box decided, excluded or left), or at the
//! limit. It records every box it did not exclude by distance as a [`FloorBox`]: its
//! `(u, v)` box, its line parameters and a [`FloorKind`] (excluded by t, left, no root,
//! edge, root, unshown, or unvisited: the boxes still to visit when the pass stopped at the
//! limit). An edge box is a box with no root one of whose sides lies on a border where a
//! path down the gap to the line can end without a root: the net's knot range (a seam the
//! solve wraps across among it, below: a crease), the solve's `(u, v)` domain, an interior
//! knot line of multiplicity at least the degree (the net below `C¹` in parameter there, a
//! crease not proved `G¹`), or a
//! stretch of the leaf's border that no searched leaf or culled strip covers
//! ([`LeafSides`]: a leaf outside the face, which gets no distance verdict). A
//! line passing within the tolerance just beyond such a border, with no root on either
//! side, leaves only boxes with no root about it; their record is what keeps that contact.
//! A box that is no edge and has no root needs a neighbour's record for its contact. A box
//! the pass did not visit carries its line parameters too (the whole line where they cannot
//! be enclosed), so that a ray can tell whether it lies before a hit.
//!
//! **The order of the boxes.** The pass visits the box of least line parameter first (a
//! half takes the least line parameter of the box it is cut from), of equal ones the last
//! cut. What it decides of a box reads the box alone, so the order changes only where a pass
//! at its limit stops: the boxes it left lie along the line past the ones it decided. A
//! crossing's line parameter is the foot of its point, inside the line parameters of every
//! box holding it, so every crossing of the leaf before the least line parameter of the boxes
//! left lies in a box decided, as every crossing of a finished pass does; the solve adds those
//! crossings and keeps the guesses' answer past them ([`publishable_roots`]: below the least
//! line parameter of the boxes left by every leaf of the solve at its limit, so that no
//! crossing a leaf at its limit adds comes after one another leaf left undecided; a finished
//! leaf adds all its crossings, as before). A
//! shallow chord of a wide cylinder's side, whose leaf takes several thousand boxes along the
//! silhouette between its crossings (where the line's direction lies in the tangent plane and
//! no box about it is regular), gets its first crossing so.
//!
//! **The exclusion phase.** After a finished pass (only there: the records of a leaf say
//! that a box is excluded by distance by its absence, which an unfinished leaf does not
//! prove), a phase with its own budget ([`MAX_EXCLUSION_BOXES`] enclosures) cuts the boxes
//! with no root, plain and edge, as step 3 cuts, down to their floor (a direction is not cut
//! once its image is within half the tolerance), and drops each piece its enclosure proves
//! farther than the tolerance (step 1). Pieces of a regular box with no root are regular and
//! hold no root. Soundness rests on one invariant: nothing leaves the records without a
//! proof that the surface over it is farther than the tolerance from the line; a box with
//! no root holds no root, so keeping it never loses a contact. Running out of the budget
//! keeps what the phase has not decided; it never makes the leaf stop at the limit.
//!
//! **Culled strips.** The traversal culls a leaf by the line's distance or parameter (its
//! box grown by the tolerance, its pseudo box, the overlap of the two, a ray's start, the
//! line's interval) and prunes a subtree by its sphere bound: slab and sphere tests of
//! rounded boxes, no proof. The domain of each (met with the solve's) is a strip that
//! [`enclose_strip`] runs step 1 on, halving in a work list with its own budget
//! ([`MAX_STRIP_BOXES`] enclosures): a box is excluded by distance, excluded by t (a record),
//! or kept undecided ([`FloorKind::Culled`], with its line parameters, the whole line where
//! they cannot be enclosed) at the floor's size or where it cannot be enclosed; out of the
//! budget, the boxes left to visit are kept undecided and the strip stops at its limit. A
//! strip records every box it does not exclude by distance, as a pass does, so it reads as a
//! searched leaf with no roots: one all excluded by distance proves that no contact is
//! there. A leaf outside the face (its trim classification) gets no distance verdict and no
//! strip: a stretch of a leaf's border facing it is a boundary.
//!
//! **The drop.** After every leaf's pass and every strip, one step over the whole solve
//! ([`drop_fenced`]) drops sets of boxes with no root proved farther than the tolerance.
//! Below, a box is *excluded* when no record holds it in a leaf or strip whose pass finished
//! or stopped at its limit and whose records the solve kept: such a pass records every box
//! it did not exclude by distance. A box excluded by t is a record and does not count (`g`,
//! the gap to the infinite line, is not bounded by a t verdict). A set `R` of regular boxes
//! with no root, plain or edge, is dropped whole when
//!
//! 1. every boundary side of a member (the surface's, the solve's, a stretch facing nothing
//!    searched) is shown farther than the tolerance by an enclosure of that side;
//! 2. every crease side of a member is shown farther by its own enclosure, or lies in closed
//!    excluded boxes (no other member meets it, and what lies across it is read as below);
//!    a crease where the net breaks (a knot of multiplicity above the degree) is shown by its
//!    own enclosure only: the two pieces' edges there are apart, so the box excluded across
//!    it holds the other piece's edge, not this one's;
//! 3. every other point of `R`'s outer border lies in a closed excluded box: no record but a
//!    member meets a member, corners included, and every leaf across a member's side on its
//!    leaf's border is read as above (a leaf whose pass did not run, or whose records were
//!    not kept, proves nothing, and keeps the set).
//!
//! `R` is compact, so `g` has a least value over it, at some `p`. On a boundary or crease
//! side, 1 or 2 gives `g(p) > tol`, and on `R`'s outer border 3 does. Otherwise every box
//! about `p` is a member, so regular, and `S` is `C¹` there (step 0 cuts at every knot, and a
//! side that is no crease has knot multiplicity below the degree, for a rational net too);
//! `g > 0`, since no member holds a root; so `p` would be a critical point of `g`, with
//! `S − L(t)` orthogonal to `S_u`, `S_v` and `D`, which a regular box rules out. The sets are
//! the connected sets of such records (closed boxes meeting, across leaf borders too), each
//! taken whole, so no set touches another and none waits for another to drop: the release
//! order is a single round, decided on the records as they were before any drop, so no leaf
//! drops by reading the final records of a leaf that reads its own. A seam side keeps a
//! boundary's test, its own enclosure (the sets do not reach across it to the other end of
//! the range, so a crease's second way is not read there). The side enclosures have their
//! own budget
//! ([`MAX_DROP_ENCLOSURES`]); past it, a set not yet shown is kept. A box left at the floor
//! is never a member: the argument needs a regular box.
//!
//! **Components.** After the drop, the kept boxes of every searched leaf and strip (left,
//! edge, plain with no root, root, unshown, unvisited, culled; not those excluded by t) and
//! the whole domain of every leaf whose pass did not run make the solve's [`components`]:
//! boxes join where their closed `(u, v)` boxes meet (across leaf borders too, and across a
//! seam the solve wraps, below) and their line parameters overlap. A component records its
//! boxes, its `(u, v)` and
//! line parameter hulls, whether it reaches an end of the line's own interval, whether it
//! lies in or touches a leaf at the limit, a leaf not run or a stretch nothing searched, and
//! the certified roots in it. The solve's
//! [`side_record`] ([`SideRecord`], owned by its solution array) is not run when no solved
//! leaf's pass ran, whatever the strips found (the strips are then not enclosed), or, with
//! every leaf culled, when no strip ran; complete when every component is decided and
//! stood for (below) and every cull has its strip; and undecided otherwise, over the line
//! parameters of each component that is not, and over the whole line's interval where a
//! cull's strip could not be placed. Nothing reads the records yet.
//!
//! **Typing** ([`type_components`]). A component is a crossing when the line passes from
//! one side of the surface to the other across it (an odd number of times), touching when
//! it leaves on the side it came from, and undecided otherwise. It is undecided whatever
//! its sides when it holds or touches a leaf whose pass stopped at its limit or did not
//! run, touches a stretch nothing searched (a trim cull: it may straddle the face's
//! border), or holds a strip box step 1 did not exclude or a certified root not reported
//! whose line parameters are not wholly past the line's own interval.
//!
//! **A component reaching an end** of the line's own interval (answer8, Q2) is typed on its
//! line parameters clipped to that interval ([`type_one`]), the other components' clipped
//! too for test 2 below: only certified roots whose enclosure's line parameters lie inside
//! the interval are counted; one wholly past it is not (a point in its unique box is
//! undecided), one reaching across the end (once narrowed about its root by Krawczyk's
//! operator about Newton's point, in its unique box: [`tight_root`]) leaves the component
//! undecided. Each clipped end `E = L(s)` of a component its count, sides or foot decide
//! must be certified farther than the tolerance from the net over the component: every box
//! of it whose line parameters hold `s` has `S − E` enclosed longer than the tolerance
//! ([`end_off_the_net`]; a regular box halved within a budget, a box at the floor enclosed
//! whole). A box whose line parameters do not hold `s` needs no test: they are its points'
//! feet widened by the tolerance as a stretch of the line, rounded outward, so every point
//! of it has its foot more than that from `s`, more than the tolerance from `E` along the
//! line; a record excluded by t lies wholly past the interval, and a point excluded by
//! distance or dropped is farther than the tolerance from the whole line; a point of the
//! net within the tolerance of `E` outside this component lies in another component
//! reaching the same end, which needs its own certificate. The sides also read `|f(E)|`
//! above half the tolerance and `ε` (`|f|` is the distance to the complete surface, or to
//! the cone's profile line, no more, and the net over the hull is within `ε` of it), and
//! the foot rule the foot's side at `E` apart from 0. An end within the tolerance of the net
//! (a ray starting on the face) stays undecided ([`Why::ReachesEnd`]), and so does an end
//! not finite. A point solution past the interval is undecided, and so is one in the unique
//! box of a root past it, reported or not.
//! Otherwise:
//!
//! - **Every box regular** (no box left at the floor): every exact root of `F` with `(u, v)`
//!   in the component lies in one of its boxes (the gap there is 0, so no distance verdict
//!   took it, and its `t` is its foot, inside the box's line parameters), a box with no root
//!   holds none, and a root box holds exactly one, a transversal crossing (`[S_u S_v −D]` is
//!   nonsingular there). Its parity is the count of its distinct certified roots: two are
//!   one where an enclosure lies in the other's unique box, two where their enclosures are
//!   apart, and the component is undecided when two are neither. It is undecided too when a
//!   box has a side on a boundary (the surface's or the solve's), or a root's enclosure
//!   reaches the knots' range or the solve's border (a seam among them): a root there may be
//!   counted again by the patch across it, and, at a seam, two unique boxes on its two ends
//!   cannot be shown to hold one root (the net is not proved `C¹` across it, which Krawczyk's
//!   operator over a box straddling it would need). Any kind of surface.
//! - **A box left at the floor**, on a surface of revolution whose net is a patch of a
//!   sphere, a cylinder or cone, or a ring torus ([`Revolved`], its stored axes orthonormal
//!   to within [`FRAME_DEFECT`]): the side function `f` (the distance to the sphere, the
//!   radius past the cylinder's or the cone's line in the half-plane through the axis, the
//!   distance to the torus's tube circle in that half-plane), continuous on all of space,
//!   zero exactly on the complete surface Σ (the whole sphere or torus, the infinite
//!   cylinder, one infinite nappe of the cone), one sign on each side. The net is not Σ: its
//!   control points are rounded. With `[t₀, t₁]` the component's line parameters, `σ` the
//!   line over them and `H` its `(u, v)` hull:
//!   1. `|f|` at `L(t₀)` and at `L(t₁)` is proved above half the tolerance;
//!   2. no other component's line parameters meet `[t₀, t₁]`;
//!   3. `H` is covered by the domains of searched leaves and strips (no trim cull inside);
//!   4. **ε**: a certified bound on `|f|` over the net above `H`, below a quarter of the
//!      tolerance: the implicit polynomial `F` of Σ composed with each Bézier patch `H`
//!      meets, in homogeneous coordinates and Bernstein form, enclosed with the rounding of
//!      the patches' knot insertions ([`side_bounds`]), over a lower bound of `F`'s other
//!      factor ([`side_eps`]: `|w| + r ≥ r` for the sphere; for the torus
//!      `(d + r)((ρ + R)² + h² − r²) ≥ r (R² − r²)`; for the cone `ρ + R ≥ R > 0`, `R`
//!      the closed form's radius at the heights of the net over `H`, which also certifies
//!      the nappe `f` describes, `F` vanishing on both; where it is not positive, at the
//!      apex, undecided);
//!   5. **the chart**: the net over `H` lies off the axis and off the profile circle's
//!      centre ([`in_the_chart_domain`], from ε), and its four edges, enclosed in pieces,
//!      certify a chart `C` ([`Chart`]): every edge piece's direction about the axis within
//!      a quarter turn of one direction `d` (and its direction about the profile circle's
//!      centre within a quarter turn of one `d′`, sphere and torus); and every piece of σ
//!      over which `|f|` is not proved above half the tolerance lies strictly in `C`: within
//!      those quarter turns, its direction about the axis strictly past every edge piece of
//!      the turn's low edge and before every one of its high edge, and its height (cone) or
//!      its direction about the profile circle's centre (sphere, torus) strictly past every
//!      piece of the profile's low edge and before every one of its high edge, in the
//!      orientation the net's points give (each test is of the piece's whole enclosure, the
//!      edge pieces' quantities in a centred form along the edge, halved within the budget
//!      [`MAX_SIDE_PIECES`]).
//!
//!   Then the parity of the net's crossings over the component (the degree mod 2 of
//!   `Φ = S − L` over `H × [t₀, t₁]`, the count of its roots when all are transversal, as
//!   [`by_count`] reads it) is that of `f`'s sign change between the ends: (a) where
//!   `|f| > ε` a point is off the net over `H`, and the net's side there agrees with the sign
//!   of `f`; (b) `G = (χ, f)`, `χ` the direction about the axis with the height or the
//!   direction about the profile circle's centre, is smooth and one to one with a smooth
//!   inverse off the axis and the centre, so `G ∘ S − G ∘ L` has `Φ`'s zeros and degree mod
//!   2, with this step: the angles of `χ ∘ S` are lifted on `H` (off the axis and the
//!   centre, by the chart's domain), and the lifts on `∂H` stay within a quarter turn of
//!   `d` (and `d′`), though `S` over the inside of `H` may wind; a root whose lifts differ
//!   from `L`'s by whole turns `(k, m) ≠ (0, 0)` is a zero of the map shifted by them, one
//!   coordinate of which keeps one sign on all of `∂H` (every edge piece passes the
//!   quarter-turn tests), so that class has winding number 0 and degree 0 under the
//!   homotopy of (d), and the parity is that of the unshifted map (`det DG` keeps one sign,
//!   so the local indices carry over); (c) by 5, for each value `c` of `χ` on the pieces of
//!   σ, `χ ∘ S − c` on `∂H` is negative on one edge and positive on the opposite one in
//!   each coordinate, so it is homotopic without a zero to the rectangle's affine map
//!   (Poincaré–Miranda), of degree ±1: the net over `H` covers `C`; (d) the `t` where
//!   `|f(L(t))|` is not above half the tolerance lie in runs strictly inside `[t₀, t₁]`
//!   (at the ends `|f|` is above it, by 1), and on a closed run `[a, b]`, `|f| = tol/2` at
//!   `a` and `b`, the homotopy `(χ(S) − χ(L((1 − s) t + s a)), (1 − s) f(S) − f(L(t)))` has no zero on its boundary
//!   (by (c) on `∂H`, by `|f| ≥ tol/2 > ε` at `a` and `b`), and at `s = 1` it is a product of
//!   degree ±1 times the sign change of `f` over the run; (e) between runs `f` keeps its
//!   sign. So opposite signs at the ends make a crossing, one sign touching; by 2 and 3 the
//!   roots over `H × [t₀, t₁]` are this component's. A hull reaching a sphere's pole is
//!   undecided (the net over it is not off the axis, so no chart's domain holds it), and so
//!   is one spanning a whole turn (no half-plane holds its edges) and a cone's apex.
//! - **A box left at the floor, no side function** (a B-spline, an extrusion, a general
//!   surface of revolution, and a sphere, cone or torus [`Revolved`] does not take; the
//!   1-D profile foot is valid only on a net exactly rotational,
//!   and no net here is: its control points are rounded, so the foot is the 2-D one
//!   everywhere): the foot `(u*(t), v*(t))` of `L(t)` solves `G = ((S − L) · S_u,
//!   (S − L) · S_v) = 0`, and its side is `s(t) = n · (L(t) − S(u*, v*))`, `n = S_u × S_v`
//!   ([`by_foot`]). With 2 and 3 as above:
//!   1. `[t₀, t₁]` is covered by stretches `T_k` (halved within [`MAX_FOOT_BOXES`] tests),
//!      each with a box `U_k` strictly inside the net's knots and the solve's domain (none
//!      straddles a seam, which is not proved `C¹`: a stretch whose boxes lie on both of its
//!      ends has none), on which Krawczyk's operator of `G` with `t` an
//!      interval parameter over `T_k` lies strictly inside `U_k` ([`foot_box`]): every `t`
//!      in `T_k` has exactly one foot in `U_k` (`S` is `C¹` over `U_k`, so `G` is Lipschitz,
//!      piecewise `C¹`, and the mean-value form reads the hull of the one-sided second
//!      partials across a knot line the net is `C¹` across; a crease is refused);
//!   2. neighbours are chained: at their common `t`, the tight box of one's foot (its
//!      operator at that `t` alone, taken again on its meet with the box) lies in the
//!      other's box, so the two feet are one;
//!   3. every box of the component whose line parameters meet `T_k` lies in `U_k`, and
//!      `U_k` is covered by searched leaves and strips;
//!   4. `n` is enclosed apart from 0 over every `U_k`;
//!   5. `s(t₀)` and `s(t₁)` are enclosed apart from 0 (over the tight foot box there).
//!
//!   Then `s(t) = 0` exactly at a root (by 4, `S_u`, `S_v` and `n` span space, and `L − S`
//!   is orthogonal to all three); every root over the component is a foot (a root is a
//!   critical point of the distance), so by the boxes' condition (3) and uniqueness the
//!   branch's; a zero of `s` is a root at a foot in a searched leaf or strip, recorded in a
//!   box whose line parameters hold `t`, so, no other component's line parameters meeting
//!   these, of this component; and `n` is one continuous function along the
//!   chained branch (no crease inside a box, neighbours sharing the foot), so the parity of
//!   the crossings is that of `s`'s sign change: `Bᵀ Φ = (G, −ŝ)` with `B = [S_u S_v n]`
//!   invertible, and the homotopy of `ŝ(x, t)` to `s(t)` along the branch keeps the zero
//!   set. Opposite signs make a crossing, one sign touching (answer8, Q1). A branch
//!   leaving the domain (a crossing just past the patch's edge), feet jumping between two
//!   critical points (a saddle) or a second sheet in the hull the foot does not follow (a
//!   fold) fail 1, 2 or 3. A stretch whose line parameters meet those of a box of the
//!   component with a side on the knots' range or the solve's domain, where the solve does
//!   not wrap, fails before any Krawczyk test: `U_k` would have to hold that box (3) and lie
//!   strictly inside that border (1). Here and along a frame below such a stretch is halved
//!   untested while it can be ([`held_on_a_border`]), and the stretch at the floor is
//!   tested, for its reason.
//! - **The same, where the closest-point foot is not certified at a stretch at the floor**
//!   (a crease or a seam the solve wraps in the way: `G` holds `S_u` and `S_v`, which jump
//!   there, and a closest point can sit on a crease for a whole range of `t` with `G` zero
//!   on neither side, so no bound on a jump makes that rule sound; a Newton step off the
//!   range; a box grown to a border): the foot **along a frame** ([`by_frame`]; answer12,
//!   Q2). Three vectors `e₁`, `e₂`, `m` are fixed for the component before any test
//!   ([`Frame::of`]: `m` the rounded middle of the enclosure of `S_u × S_v` over its middle
//!   box, `e₁`, `e₂` a rounded completion, `det B` of `B = [e₁ e₂ m]` enclosed apart from 0;
//!   how well they fit decides how often the rule succeeds, never what it proves). With
//!   `E = [e₁ e₂]`, `G̃(x, t) = Eᵀ (S(x) − L(t))` vanishes where the line through `L(t)`
//!   along `e₁ × e₂` meets the net, and `h(x, t) = m · (L(t) − S(x))` is the side: `Bᵀ (S −
//!   L) = (G̃, −h)`, so the roots of `Φ = S − L` are the common zeros of `G̃` and `h`. `G̃`
//!   reads `S`, `S_u` and `S_v` only, and is continuous wherever `S` is. With 2 and 3 as
//!   above:
//!   1. `[t₀, t₁]` is covered by stretches `T_k`, each with a box `U_k` on which Krawczyk's
//!      operator of `G̃` about a point `y` of it, `t` an interval parameter over `T_k`, lies
//!      strictly inside `U_k`, its contraction below 1 ([`frame_box`]). Its Jacobian `J(U) =
//!      Eᵀ [S_u S_v]` is first partials only, enclosed over `U` as the hull of both sides'
//!      at every knot line inside it. `U_k` crosses a knot line only where the net is
//!      continuous (multiplicity at most the degree, weights shown positive:
//!      [`LineNet::over`] refuses a break) and an end of the knots' range only at a seam the
//!      solve wraps ([`LineNet::seams`]: exact closure and the whole range), as two pieces
//!      in the net's parameters glued there, together less than a turn wide; it lies
//!      strictly inside every other border. On a segment crossing finitely many knot lines
//!      a continuous, piecewise `C¹` `S` has `S(x) − S(y) = ∫ DS(γ(τ)) (x − y) dτ` piece by
//!      piece, and the interval matrix is convex, so `G̃(x, t) − G̃(y, t) ∈ J(U) (x − y)`:
//!      all that Krawczyk's existence and uniqueness use. Across a seam the lifted `S` is
//!      continuous (exact closure) and the translation keeps `∂S/∂u`. So each `t` in `T_k`
//!      has exactly one zero `x*(t)` of `G̃` in `U_k`, every matrix of `J(U_k)` is
//!      nonsingular and `x ↦ G̃(x, t)` is one to one on `U_k`: `det J = (e₁ × e₂) · (S_u ×
//!      S_v)` keeps one sign, the net over `U_k` a graph over the plane of `e₁`, `e₂` with
//!      one orientation on both sides of a crease. A crease folding the surface over
//!      relative to the frame fails here and stays undecided (this replaces 4 of the
//!      closest-point rule);
//!   2. at a seam, offsets `x − y` are taken along the lifted interval in intervals rounded
//!      outward ([`offsets_of`]: through the seam `(x − r₀) + (r₁ − y)`, never a parameter
//!      shifted by the range's width), `y` a parameter of the net on one piece, and the
//!      inclusion compares the operator's offsets with `U_k`'s rounded inward; Newton's
//!      steps toward `y` ([`frame_newton`], first partials only, never the closest-point
//!      steps) may pass a seam by a rounded shift, since they only propose;
//!   3. neighbours are chained through their tight boxes at the common `t`; every box of
//!      the component whose line parameters meet `T_k` lies in a piece of `U_k`, and
//!      `U_k`'s pieces are covered by searched leaves and strips;
//!   4. `h(x*(t₀), t₀)` and `h(x*(t₁), t₁)` are enclosed apart from 0 over the tight box.
//!
//!   Then, mod 2 (the determinant may be negative on a box, so no homotopy to `x − x*(t)`
//!   is read): every root over the component is a zero of `G̃` in the `U_k` of its `t` (by
//!   3), so it lies on the chained branch, and the roots are the zeros of `φ(t) = h(x*(t),
//!   t)`; `Ψ(x, t) = (G̃(x, t), t)` is a continuous injection on each `U_k × T_k`, so a
//!   homeomorphism onto its image (invariance of domain), of degree 1 mod 2, and at a
//!   junction the chained foot lies strictly inside both boxes; hence the index mod 2 of
//!   `(G̃, h)` at each root is that of `φ` at its `t`, and the parity of the crossings is
//!   that of `φ`'s sign change between `t₀` and `t₁`. Opposite signs make a crossing, one
//!   sign touching ([`Typed::FrameFoot`]). One frame serves every stretch (frames differing
//!   by stretch would need `h` apart from 0 at every junction). The rule runs only after
//!   the closest-point rule failed at a stretch at the floor, so it never moves a component
//!   that rule decides; where it fails too, the component keeps the closest-point rule's
//!   reason, with its own beside it ([`Component::frame_why`]).
//!
//! The distinct roots of a component are counted from its root boxes: one where an
//! enclosure lies in the other's unique box, two where the enclosures are apart, and the
//! component is undecided when two are neither.
//!
//! **The seam** (answer10, Q1). Where the stored net closes exactly in a direction (its
//! first and last rows one homogeneous row up to a common factor, [`closes_exactly`]: every
//! control point of both, so every point of the seam, with no tolerance) and the solve's
//! domain reaches both ends of the knots' range there ([`LineNet::seams`]), the parameter
//! space wraps: `(u₀, v) ↔ (u₁, v)` is a translation with the same `v`, so it keeps the
//! orientation and `S` is continuous across it. The seam is then a knot line of the face,
//! never a boundary: a crease ([`SideKind::Crease`]), since `S_u` may jump there (unequal
//! end knot spans) and `C¹` across it is not proved, so no Krawczyk box (the leaves are
//! native) and no closest-point foot box straddles it (a box of the foot along a frame,
//! which needs `S` continuous only, does). [`leaf_sides`] reads the leaves on the range's
//! other end across a seam side (a stretch none covers is a trim cull, a boundary). Two
//! boxes on opposite ends whose stretches along the seam meet share a point of the seam,
//! one point of the net with one foot, so [`components`] joins them where their line
//! parameters overlap, as across a leaf border; a net 1 ulp from closing has two edge
//! points there and never wraps. A component joined across a seam is read as its two
//! pieces in native coordinates, glued there ([`Span`]: no shift by the range's width,
//! which rounds); the side test encloses and covers piecewise and certifies the chart on
//! the lifted rectangle's edges, made of both pieces' edges, and its degree and winding
//! argument needs only continuity across the seam. Its stretches merged are a connected arc
//! of the turn: one piece covering the whole range is a whole turn, whose lift is no
//! rectangle less than a turn wide, so a component whose boxes cover the whole range and
//! that holds a box left at the floor is undecided ([`Why::WholeTurn`]: the side chart's
//! degree and winding and the foot chain read a lift); one whose boxes are all regular is
//! typed by its count, which reads no lift (each root unique in its native box and off the
//! seam, the end certificate and the placing box by box; answer11, Q3). What a box touches across a seam (a leaf at its
//! limit or not run, a trim cull) is read only for a component joined across it
//! ([`facing`]): one that is not keeps the flags it had, its seam sides facing a trim cull
//! boundaries. A partial turn, or a solve over part of the range, never wraps.
//!
//! An undecided component keeps why ([`Why`]), down to the condition that failed: for 2,
//! what is known of the components meeting it ([`Overlap`]: across a seam or on opposite
//! ends of the knots' range, line parameters apart without the tolerance's widening); for the
//! foot rule, which step failed ([`FootWhy`]) and, for a stretch at the floor, which
//! condition of its foot box ([`Stretch`]). A diagnosis only: nothing reads it. Apart from
//! why, a component records whether it reaches a boundary side ([`Component::at_boundary`]:
//! the surface's or the solve's border where the solve does not wrap, or a trim cull,
//! answer10, Q2): a contact there may belong to the face across the edge, which no face's
//! own solve can type against the exact data, so a consumer fires another ray for it; an
//! undecided component that reaches none is a gap of the typing.
//!
//! The solutions of the solve are then typed ([`type_solutions`]). With a cull not placed
//! or the components not built, every one is undecided, and so is every range. A point is
//! undecided unless its gap to the line is enclosed below the tolerance and it lies in a box
//! of a decided component (the boxes of two components never both hold a point with its
//! line parameter). There, a point in the unique box of one of the component's roots
//! carries that root, a crossing (the two are one contact) when it is that root's only
//! carrier; a point in the unique boxes of two roots not shown one, or of another
//! component's root, is undecided. What the boxes left at the floor hold beyond the roots
//! is the component's parity less its count of roots: with every root carried once, the
//! single other point is touching where that is even and a crossing where it is odd;
//! otherwise (two other points or more, not one seam location, whose parity cannot tell two
//! crossings from two touching points) they are undecided. A component typed by its sides in leaves
//! not all wholly inside the face may straddle its border (a leaf on the trim is searched
//! whole), so its other points are undecided. A component the solutions do not stand for
//! (a root not carried once, an odd rest with no single point to carry it, or undecided)
//! stays undecided in the side record: a ray would otherwise pass a crossing it was not
//! given.
//!
//! **The seam pair** (answer11, Q1). The leaf solve and p8's dedupe measure u and v
//! natively, so a contact on a seam the solve wraps can come back twice, once at each end
//! of the knots' range (their parameters differ by the range's width, so the dedupe never
//! merges them). Two points of a component are one seam location ([`seam_pair`]) when every
//! one of these holds: (1) the solve wraps in that direction and the component is joined
//! across that seam; (2) the component is decided, not typed by its sides or foot in leaves
//! not all inside the face, every one of its certified roots carried exactly once, and
//! neither point in any root's unique box (each is another point, so the pair never enters a
//! count, and a seam crossing identified here never becomes two roots); (3) the two are its
//! only other points; (4) one lies in a box of it reaching the range's low end and the other
//! in one reaching its high end, and those two boxes are joined directly by the seam rule of
//! [`components`] (a link through the rest of the component is not one); (5) both lie where
//! the two boxes meet, their line parameters in the meet of the boxes' line parameters and
//! their coordinates along the seam in the meet of the boxes' stretches along it (at a
//! corner, the corner), each with its gap shown below the tolerance. The pair is then one
//! slot of the rest: both crossings where the rest is odd, both touching where it is even,
//! and the component is stood for. Its parity is its rule's, which an identification never
//! changes; what reaches a consumer is the points, so the two carry one id
//! ([`ContactId`], `Solution::shared_contact`; never an index, which the rescale, the ray's
//! truncation, the dedupe and the coincidence pass shift), and the consumer's contract is
//! that a linked pair counts once: over every component stood for, its crossings, a linked
//! pair once, add up to its parity. A tolerance merge of a linked point loses the id and
//! gives the merged contact, undecided for a crossing (the safe direction). A double root
//! next to the seam, two real crossings: certified, they are two roots with two carriers,
//! which (2) excludes; in boxes at the floor, the component's rule holds their parity, and
//! the pair stands for it at a location (5) keeps true.
//!
//! **Reuse within an operation.** What a solve's passes find before they read its
//! solutions, the [`Search`] (every leaf's pass, every strip, the drop, the components and
//! their types), is a function of the net, the line ([`PassLine`]), and the leaves' and
//! strips' domains, line intervals and inside flags alone: nothing else is read, and every
//! step is deterministic. An operation whose solves repeat (the Boolean's solve with face
//! caches runs again from its start each time it builds a face's cache or makes a trim, so
//! the line solves of every attempt before repeat) keeps its searches in a
//! [`LinePasses`], under a [`PassKey`] that holds every one of those inputs bit for bit
//! ([`LineNet`]'s every value read, each `f64` by its bits): a solve finds a search only
//! under an equal key, and takes the value its own search would compute. The roots it adds,
//! and the typing of its solutions, are made from its own solutions as before. A search a
//! cancel stopped is never kept. Results are unchanged; the work is not: a solve that takes
//! a kept search polls the cancel token less, and an attempt whose root is refused for a
//! trim to make has searched all its leaves first, so a fuel budget
//! ([`CancelToken::with_fuel`]) may run out at another point; a search taken is not in the
//! line counters
//! ([`crate::kernel::WorkCounters::line_searches_reused`] counts it). The kernel's
//! `memoize_curve_caches` off keeps nothing, to check that.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`LineNet`], [`isolate`], [`PassLine`], [`CertifiedRoot`], [`Isolation`], [`Stop`], [`FloorBox`], [`FloorKind`], [`Exclusion`], [`LeafSides`], [`leaf_sides`], [`enclose_strip`], [`SearchedLeaf`], [`drop_fenced`], [`Drops`], [`components`], [`Components`], [`Component`], [`side_record`], [`Revolved`], [`type_components`], [`Typed`], [`Why`], [`Overlap`], [`FootWhy`], [`Stretch`], [`type_solutions`], [`SolutionAt`], [`Search`], [`PassKey`], [`LinePasses`] | (not upstream) |
//!
//! Deviations: none (no upstream counterpart). Bounds: no recursion (work lists); a box is
//! halved until its middle is one of its ends; at most [`MAX_SCRATCH_BOXES`] boxes a leaf,
//! past which the pass stops for that leaf and reports what it has certified (each root
//! stands on its own proof), and at most [`MAX_EXCLUSION_BOXES`] enclosures in its
//! exclusion phase, so a leaf's records are at most the two together and the boxes left to
//! visit; at most [`MAX_STRIP_BOXES`] enclosures a strip; at most
//! [`crate::geom::krawczyk::KRAWCZYK_STEPS`] Krawczyk steps a box; at most
//! [`MAX_MEETING_TESTS`] pairs of boxes tested in one sweep, [`MAX_SOLVE_BOXES`] boxes in
//! one solve's drop and components, past which nothing is dropped, the components are not
//! built and the whole line is undecided, and [`MAX_DROP_ENCLOSURES`] side enclosures in
//! one solve's drop; at most [`MAX_SIDE_PIECES`] piece and edge enclosures in one
//! component's side test, [`MAX_FOOT_BOXES`] foot tests in each of its foot rules (a
//! stretch meeting a box on a border the rule's box may not reach is halved untested, at
//! one test, and only the stretch at the floor is tested; and, along a
//! frame, at most [`FRAME_TIGHTENINGS`] operators for each tight box: two a junction of the
//! chain, one an end), [`MAX_COVER_RECTS`] leaves in its coverage test and [`MAX_ROOT_BOXES`] root boxes
//! told apart; at most [`MAX_KEPT_SEARCHES`] searches and [`MAX_KEPT_BYTES`] bytes kept in
//! one [`LinePasses`], past which a search is not kept (a solve repeating it searches); at
//! most [`crate::surface::implicit::MAX_SIDE_POINTS`] control points in a net the side
//! bounds read, and as many in the Bézier patches they decompose it into, past which (or
//! past [`crate::nurbs::knots::MAX_DEGREE`]) the net has no side bound.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::math::det;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::blend::fillet_section::interval::{cmax, cmin, round_down, round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::Nurbs;
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::krawczyk::{
    contraction, inverse_of_columns2, inverse_of_columns3, k_operator, krawczyk_region, newton,
    plus_c_times, start_minus_y, Linearised, KRAWCZYK_STEPS,
};
use crate::geom::nurbs_enclosure::nurbs_partials_over_box_about;
use crate::geom::solution_array::{Contact, ContactId, SideRecord};
use crate::kernel::{CancelToken, Kernel, WorkCounters};
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface::BSplineSurfaceForm;
use crate::surface::implicit::{side_bounds, Revolution, SideBound};
use crate::surface::surface::Surface;

const OP: &str = "surface::surface::certified_line::isolate";

/// The most enclosures one leaf's exclusion phase makes (module docs, "The exclusion
/// phase"): the root pass's own bound, so that a leaf's records stay within the two
/// bounds together and the boxes left to visit.
pub(crate) const MAX_EXCLUSION_BOXES: usize = MAX_SCRATCH_BOXES;

/// The most boxes one leaf's pass encloses. A leaf with a transversal crossing takes some
/// tens (the issue's torus leaf with two crossings at 2° and 2.5°: 63); the most measured
/// over the kernel's goldens and cases is recorded with the pull request that set this,
/// and the bound is several times that. Past it the pass stops for the leaf.
pub(crate) const MAX_SCRATCH_BOXES: usize = 1 << 12;

/// The most enclosures one culled strip makes (module docs, "Culled strips"): a leaf's own
/// bound, since a strip is a leaf, or a subtree of leaves, the traversal culled.
pub(crate) const MAX_STRIP_BOXES: usize = MAX_SCRATCH_BOXES;

/// The most side enclosures one solve's drop makes (module docs, "The drop"): one for each
/// of the most records the drop reads.
pub(crate) const MAX_DROP_ENCLOSURES: usize = MAX_SOLVE_BOXES;

/// Three intervals: a vector's components, or a box in `(u, v, t)`.
type I3 = [Ivl; 3];

/// The exact net of a surface the certified pass runs on, with its parameter range.
#[derive(Debug, Clone)]
pub(crate) struct LineNet {
    net: Nurbs,
    /// The knots' range in u and in v.
    range: [[f64; 2]; 2],
    /// The interior knots of multiplicity at least the degree, in u and in v: the knot lines
    /// across which the net is below `C¹` in parameter (each once, ascending).
    creases: [Vec<f64>; 2],
    /// Of the creases, those of multiplicity above the degree, in u and in v: where the net
    /// breaks, its two pieces' edges apart (each once, ascending).
    breaks: [Vec<f64>; 2],
    /// The complete surface of revolution the net is a patch of, for the kinds the side
    /// test reads ([`Revolved`]); `None` for every other surface.
    revolved: Option<Revolved>,
    /// The net as the surface stores it, for [`side_bounds`] (with
    /// [`LineNet::revolved`] only).
    form: Option<crate::nurbs::srf_basic::Surface>,
    /// Whether the stored net closes exactly in u and in v ([`closes_exactly`]): where the
    /// solve covers the whole knots' range too, its parameter space wraps there
    /// ([`LineNet::seams`]).
    closes: [bool; 2],
}

impl LineNet {
    /// The net of `surface` when its net is the surface (the kinds of
    /// [`Surface::certified_bounding_box`], B-splines in power-basis form apart) and the
    /// enclosures take it (positive weights, knots and points that agree); `None` for any
    /// other surface, which keeps the guesses alone. A net fitted to a definition
    /// ([`Surface::net_is_fitted`]: a swept wall's, a Setback or Mixed corner's, kept by the
    /// evaluation copy a surface cache holds though the copy drops the definition) is not
    /// the surface: a root certified on it may lie within the bound of a line that misses
    /// the definition, so it gets no pass.
    ///
    /// # Errors
    ///
    /// Those of making a missing NURB.
    pub(crate) fn of(k: &Kernel, surface: &Surface) -> Result<Option<Self>> {
        if surface.net_is_fitted() {
            return Ok(None);
        }
        let Some(base) = surface.certified_net(k)? else {
            return Ok(None);
        };
        if base.form() == BSplineSurfaceForm::Polynomial {
            return Ok(None);
        }
        let Ok(net) = Nurbs::of(&base) else {
            return Ok(None);
        };
        let ends = |knots: &[f64], p: usize, n: usize| match (knots.get(p), knots.get(n)) {
            (Some(&a), Some(&b)) if a < b && a.is_finite() && b.is_finite() => Some([a, b]),
            _ => None,
        };
        let (Some(u), Some(v)) = (ends(&net.u, net.p, net.nu), ends(&net.v, net.q, net.nv)) else {
            return Ok(None);
        };
        // (a break is a crease of multiplicity above the degree)
        let breaks = [
            creases(&net.u, net.p.saturating_add(1), u),
            creases(&net.v, net.q.saturating_add(1), v),
        ];
        let creases = [creases(&net.u, net.p, u), creases(&net.v, net.q, v)];
        let revolved = Revolved::of(surface, &net);
        let form = revolved.and_then(|_| base.gw_nurb().cloned());
        let closes = [closes_exactly(&net, true), closes_exactly(&net, false)];
        Ok(Some(Self {
            net,
            range: [u, v],
            creases,
            breaks,
            revolved,
            form,
            closes,
        }))
    }

    /// The certified bounds of [`side_bounds`] over the net's Bézier patches, for its
    /// surface of revolution; none for another surface, or where the decomposition fails.
    ///
    /// # Errors
    ///
    /// A cancel of `cancel`, polled before each patch.
    fn side_bounds(&self, cancel: Option<&CancelToken>) -> Result<Vec<SideBound>> {
        match (&self.form, &self.revolved) {
            (Some(form), Some(rev)) => side_bounds(form, &rev.revolution(), cancel),
            _ => Ok(Vec::new()),
        }
    }

    /// Where the solve's parameter space over `solve` closes on itself (module docs, "The
    /// seam"): in each direction in which the stored net closes exactly ([`closes_exactly`])
    /// and `solve` reaches both ends of the knots' range (the clamped domain is the whole
    /// range, so the seam lies inside the face). A partial turn does not close, and a solve
    /// over part of the range keeps its border a boundary.
    pub(crate) fn seams(&self, solve: &Extent2d) -> Seams {
        let [[u0, u1], [v0, v1]] = self.range;
        let (lo, hi) = (solve.min(), solve.max());
        let [cu, cv] = self.closes;
        Seams {
            range: self.range,
            wrap: [
                cu && lo.x <= u0 && hi.x >= u1,
                cv && lo.y <= v0 && hi.y >= v1,
            ],
        }
    }

    /// Whether the side `s` of the `(u, v)` box `b` (in the order of
    /// [`LineNet::side_kinds`]) lies on a break of the net ([`LineNet::breaks`]).
    fn breaks_at(&self, b: Scratch, s: usize) -> bool {
        let [bu, bv] = &self.breaks;
        match s {
            0 => bu.contains(&b.u.lo),
            1 => bu.contains(&b.u.hi),
            2 => bv.contains(&b.v.lo),
            _ => bv.contains(&b.v.hi),
        }
    }

    /// Whether the side `s` of the `(u, v)` box `b` (in the order of
    /// [`LineNet::side_kinds`]) lies on a seam of the solve over `solve` ([`LineNet::seams`]).
    fn on_a_seam(&self, solve: &Extent2d, b: Scratch, s: usize) -> bool {
        let seams = self.seams(solve);
        let [[u0, u1], [v0, v1]] = seams.range;
        let [wu, wv] = seams.wrap;
        match s {
            0 => wu && b.u.lo <= u0,
            1 => wu && b.u.hi >= u1,
            2 => wv && b.v.lo <= v0,
            _ => wv && b.v.hi >= v1,
        }
    }

    /// Whether a side of the `(u, v)` box `b` lies on a border where a path down the gap to
    /// the line can end without a root (module docs, "What the pass decided").
    fn on_a_border(&self, solve: &Extent2d, sides: &LeafSides, b: Scratch) -> bool {
        self.side_kinds(solve, sides, b)
            .iter()
            .any(|&k| k != SideKind::Plain)
    }

    /// What each side of the `(u, v)` box `b` lies on, in the order u = `b.u.lo`, u =
    /// `b.u.hi`, v = `b.v.lo`, v = `b.v.hi`: a boundary (the knots' range where the solve does
    /// not wrap; the solve's `(u, v)` domain `solve`, on it or past it, as a leaf is trimmed
    /// to it but a leaf within a rounding of it is kept whole; a stretch of the leaf's border
    /// that no searched leaf or strip covers, across a seam too), a crease (an interior knot
    /// line the net is below `C¹` across, which step 0 cuts boxes exactly at, or a seam the
    /// solve wraps across, [`LineNet::seams`]: a knot line of the face, not proved `C¹`), or
    /// neither.
    fn side_kinds(&self, solve: &Extent2d, sides: &LeafSides, b: Scratch) -> [SideKind; 4] {
        let [[u0, u1], [v0, v1]] = self.range;
        let [cu, cv] = &self.creases;
        let (lo, hi) = (solve.min(), solve.max());
        let unsearched = sides.faces_unsearched(b);
        let kind = |boundary: bool, crease: bool, unsearched: bool| {
            if boundary || unsearched {
                SideKind::Boundary
            } else if crease {
                SideKind::Crease
            } else {
                SideKind::Plain
            }
        };
        let on_u = |x: f64| x <= u0 || x >= u1 || x <= lo.x || x >= hi.x;
        let on_v = |y: f64| y <= v0 || y >= v1 || y <= lo.y || y >= hi.y;
        // (where the solve wraps, it holds the knots' range, and a box lies on the range: a
        // side on the border lies on the seam)
        let [wu, wv] = self.seams(solve).wrap;
        let [c0, c1, c2, c3] = unsearched;
        [
            kind(
                on_u(b.u.lo) && !wu,
                cu.contains(&b.u.lo) || on_u(b.u.lo),
                c0,
            ),
            kind(
                on_u(b.u.hi) && !wu,
                cu.contains(&b.u.hi) || on_u(b.u.hi),
                c1,
            ),
            kind(
                on_v(b.v.lo) && !wv,
                cv.contains(&b.v.lo) || on_v(b.v.lo),
                c2,
            ),
            kind(
                on_v(b.v.hi) && !wv,
                cv.contains(&b.v.hi) || on_v(b.v.hi),
                c3,
            ),
        ]
    }

    /// The first knot of the net strictly inside `u` (in u) or, with none there, strictly
    /// inside `v` (in v): whether it is in u, and the knot.
    fn knot_inside(&self, u: Ivl, v: Ivl) -> Option<(bool, f64)> {
        let inside = |knots: &[f64], x: Ivl| knots.iter().copied().find(|&k| k > x.lo && k < x.hi);
        inside(&self.net.u, u)
            .map(|k| (true, k))
            .or_else(|| inside(&self.net.v, v).map(|k| (false, k)))
    }

    /// Whether `[u.lo, u.hi] x [v.lo, v.hi]` lies on the knots' range.
    fn holds(&self, u: Ivl, v: Ivl) -> bool {
        let [[u0, u1], [v0, v1]] = self.range;
        u.lo >= u0 && u.hi <= u1 && v.lo >= v0 && v.hi <= v1 && u.lo <= u.hi && v.lo <= v.hi
    }

    /// `S − about`, `S_u` and `S_v` over the box, enclosed; `None` where the box leaves the
    /// knots' range or the enclosure is refused (a break of the surface inside the box,
    /// weights not proved positive over it).
    fn over(&self, u: Ivl, v: Ivl, about: [f64; 3]) -> Option<[I3; 3]> {
        if !self.holds(u, v) {
            return None;
        }
        // continuity 0: a box may cross a knot line the surface is continuous across; the
        // hull of the pieces' partials then holds each one-sided partial, which is what the
        // mean-value form needs (`geom::krawczyk`, module docs)
        let p =
            nurbs_partials_over_box_about(&self.net, u.lo, u.hi, v.lo, v.hi, 1, 0, about).ok()?;
        let out = [i3(p.at(0, 0)), i3(p.at(1, 0)), i3(p.at(0, 1))];
        out.iter().flatten().all(|x| x.is_finite()).then_some(out)
    }

    /// The second partials `S_uu`, `S_uv` and `S_vv` over the box, enclosed; `None` as
    /// [`LineNet::over`], and where the box crosses a knot line the surface is not `C¹`
    /// across (the fold's system reads the first partials as a continuous function).
    fn second(&self, u: Ivl, v: Ivl, about: [f64; 3]) -> Option<[I3; 3]> {
        if !self.holds(u, v) {
            return None;
        }
        let p =
            nurbs_partials_over_box_about(&self.net, u.lo, u.hi, v.lo, v.hi, 2, 1, about).ok()?;
        let out = [i3(p.at(2, 0)), i3(p.at(1, 1)), i3(p.at(0, 2))];
        out.iter().flatten().all(|x| x.is_finite()).then_some(out)
    }

    /// `S − about` over the box, enclosed, without its partials; `None` as
    /// [`LineNet::over`].
    fn value_over(&self, u: Ivl, v: Ivl, about: [f64; 3]) -> Option<I3> {
        if !self.holds(u, v) {
            return None;
        }
        let p =
            nurbs_partials_over_box_about(&self.net, u.lo, u.hi, v.lo, v.hi, 0, 0, about).ok()?;
        let out = i3(p.at(0, 0));
        out.iter().all(|x| x.is_finite()).then_some(out)
    }

    /// `S − about` at the point `(u, v)`, enclosed.
    fn at(&self, u: f64, v: f64, about: [f64; 3]) -> Option<I3> {
        let (u, v) = (Ivl::from(u), Ivl::from(v));
        if !self.holds(u, v) {
            return None;
        }
        let p =
            nurbs_partials_over_box_about(&self.net, u.lo, u.hi, v.lo, v.hi, 0, 0, about).ok()?;
        let out = i3(p.at(0, 0));
        out.iter().all(|x| x.is_finite()).then_some(out)
    }
}

/// Whether the net closes exactly in u (`in_u`, else in v): at least two rows of control
/// points in that direction, as many knots there as rows plus the degree plus one, each end
/// knot of multiplicity exactly the degree plus one (so its first and last rows are its
/// edges there: with a greater one the end row's basis function vanishes and the edge is the
/// next row), and those two rows one homogeneous row up to one common factor λ, weights
/// included. The edge at the first row is `Σ N_k a_k` and the one at the last `Σ N_k b_k`
/// (projected), one curve for every parameter when `b_k = λ a_k` for one `λ > 0` and every
/// `k` (sufficient, not necessary: rows sharing a polynomial factor make one curve too, and
/// are not shown); equal as Cartesian points place by place, each with its own weight ratio,
/// they are two curves (rows `(0, 0, 0)`, `(1, 1, 0)`, `(2, 0, 0)` of weights 1, 1, 1 and 1,
/// 2, 1 meet at the ends and part between). With `λ = b_0w / a_0w` taken implicitly, the
/// test is `b_kc a_0w = a_kc b_0w` for every `k` and `c ∈ {x, y, z, w}`, each product
/// compared exactly ([`equal_products`], no division), and every weight of both rows
/// positive.
fn closes_exactly(net: &Nurbs, in_u: bool) -> bool {
    let (knots, degree) = if in_u {
        (&net.u, net.p)
    } else {
        (&net.v, net.q)
    };
    let (count, across) = if in_u {
        (net.nv, net.nu)
    } else {
        (net.nu, net.nv)
    };
    let len = knots.len();
    let Some(order) = degree.checked_add(1) else {
        return false;
    };
    let counted = across.checked_add(order) == Some(len);
    // each end knot `order` times exactly: the first `order` knots one value and the next
    // greater, the last `order` one value and the one before them less
    let ends = || -> Option<bool> {
        let first = knots.first()?;
        let tail = len.checked_sub(order)?;
        let last = knots.last()?;
        let head = knots.get(..order)?.iter().all(|k| k == first);
        let back = knots.get(tail..)?.iter().all(|k| k == last);
        let after = knots.get(order)?;
        let before = knots.get(tail.checked_sub(1)?)?;
        Some(head && back && after > first && before < last)
    };
    let point = |i: usize, j: usize| -> Option<[f64; 4]> {
        let k = i.checked_mul(net.nv)?.checked_add(j)?.checked_mul(4)?;
        let x = net.pw.get(k..k.checked_add(4)?)?;
        Some([*x.first()?, *x.get(1)?, *x.get(2)?, *x.get(3)?])
    };
    // (with one row the first and last are one: no seam is shown)
    if count == 0 || across < 2 || !counted || ends() != Some(true) {
        return false;
    }
    let last = across - 1;
    let row = |k: usize| {
        if in_u {
            (point(0, k), point(last, k))
        } else {
            (point(k, 0), point(k, last))
        }
    };
    let (Some(a0), Some(b0)) = row(0) else {
        return false;
    };
    let [.., a0w] = a0;
    let [.., b0w] = b0;
    (0..count).all(|k| match row(k) {
        (Some(a), Some(b)) => {
            let [.., aw] = a;
            let [.., bw] = b;
            aw > 0.0
                && bw > 0.0
                && a.iter()
                    .zip(&b)
                    .all(|(&ac, &bc)| equal_products(bc, a0w, ac, b0w))
        }
        _ => false,
    })
}

/// Whether `x y = z w` exactly: each product as its rounded value and its error, the error
/// exact by a fused multiply-add (Ogita, Rump and Oishi, "Accurate sum and dot product",
/// SIAM J. Sci. Comput. 26 (2005), `TwoProduct`), so that equal pairs are equal products
/// and the converse also holds. The error of `x y` is representable when the product is at
/// least `2⁻⁹⁶⁸` (≈ 4·10⁻²⁹²), so a product of magnitude below `10⁻²⁸⁰` that is not
/// exactly 0 is not shown; a product rounding to 0 is exact only when a factor is 0 (an
/// underflow rounds both the product and its error to 0, as `5·10⁻³²⁴ × 0.4` does). A
/// product not finite is not shown (NaN fails every comparison), and one rounding down to a
/// finite `f64::MAX` keeps an exact remainder. `−0.0` and `0.0` are one value here.
fn equal_products(x: f64, y: f64, z: f64, w: f64) -> bool {
    let product = |a: f64, b: f64| {
        let p = a * b;
        let exact = p.is_finite()
            && if p == 0.0 {
                a == 0.0 || b == 0.0
            } else {
                p.abs() >= 1.0e-280
            };
        exact.then(|| (p, a.mul_add(b, -p)))
    };
    match (product(x, y), product(z, w)) {
        (Some(l), Some(r)) => l == r,
        _ => false,
    }
}

/// The knots of `knots` (degree `degree`) strictly inside `range` whose multiplicity is at
/// least the degree, each once: the net is `C^(degree − multiplicity)` across a knot, so
/// below `C¹` there.
fn creases(knots: &[f64], degree: usize, [lo, hi]: [f64; 2]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    let mut i = 0;
    while let Some(&k) = knots.get(i) {
        let run = knots
            .get(i..)
            .map_or(0, |rest| rest.iter().take_while(|&&x| x == k).count());
        if k > lo && k < hi && run >= degree {
            out.push(k);
        }
        i += run.max(1);
    }
    out
}

/// Where the solve's parameter space closes on itself ([`LineNet::seams`]): the knots'
/// range, and in each direction whether it wraps (module docs, "The seam").
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Seams {
    /// The knots' range in u and in v.
    range: [[f64; 2]; 2],
    /// Whether the parameter space wraps in u and in v.
    wrap: [bool; 2],
}

/// What a side of a box lies on ([`LineNet::side_kinds`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideKind {
    /// The surface's or the solve's boundary, or a stretch of the leaf's border that no
    /// searched leaf or strip covers: nothing across it was searched.
    Boundary,
    /// A crease knot line (the net below `C¹` in parameter across it), or a seam the solve
    /// wraps across (not proved `C¹`).
    Crease,
    /// Neither.
    Plain,
}

/// What lies across the four sides of one searched leaf's (or culled strip's) domain
/// (module docs, "What the pass decided"): for each side, the other searched leaves and
/// strips across it and the stretch of it each covers. A stretch of a side inside the
/// solve's domain that none covers faces a leaf outside the face, which gets no distance
/// verdict: a box side on it counts as a boundary.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LeafSides {
    /// The leaf's domain, `[[u0, u1], [v0, v1]]`.
    bounds: [[f64; 2]; 2],
    /// Whether each side (u = u0, u = u1, v = v0, v = v1) lies inside the solve's domain.
    inside: [bool; 4],
    /// For each side, the leaves across it with the stretch each covers along it (in v for
    /// the sides of constant u, in u for the others), by the stretch's start.
    across: [Vec<(usize, [f64; 2])>; 4],
    /// Whether each side lies on a seam the solve wraps across, its leaves across on the
    /// knots' range's other end.
    seam: [bool; 4],
}

impl LeafSides {
    /// A leaf that is the whole solve: every side lies on the solve's border.
    #[cfg(test)]
    pub(crate) fn whole_solve(domain: &Extent2d) -> Self {
        Self {
            bounds: bounds_of(domain),
            inside: [false; 4],
            across: Default::default(),
            seam: [false; 4],
        }
    }

    /// For each side of the box `b` (in the order of [`LineNet::side_kinds`]): whether it
    /// lies on the leaf's border inside the solve's domain and some of it faces no other
    /// searched leaf or strip.
    fn faces_unsearched(&self, b: Scratch) -> [bool; 4] {
        let ranges = side_ranges(b);
        core::array::from_fn(|s| {
            let on = self.on_side(b, s);
            match (on, self.inside.get(s), self.across.get(s), ranges.get(s)) {
                (true, Some(true), Some(across), Some(&range)) => !covered(across, range),
                _ => false,
            }
        })
    }

    /// Whether the side `s` of the box `b` lies on the same side of the leaf's border.
    fn on_side(&self, b: Scratch, s: usize) -> bool {
        let [[u0, u1], [v0, v1]] = self.bounds;
        match s {
            0 => b.u.lo <= u0,
            1 => b.u.hi >= u1,
            2 => b.v.lo <= v0,
            _ => b.v.hi >= v1,
        }
    }

    /// The leaves across the side `s` of the box `b` (on the leaf's border there) that
    /// meet it, ends included.
    fn leaves_across(&self, b: Scratch, s: usize) -> impl Iterator<Item = usize> + '_ {
        let range = side_ranges(b).get(s).copied();
        let on = self.on_side(b, s);
        self.across
            .get(s)
            .into_iter()
            .flatten()
            .filter(move |(_, [c0, c1])| on && range.is_some_and(|[a, z]| *c0 <= z && *c1 >= a))
            .map(|&(j, _)| j)
    }
}

/// `[[u0, u1], [v0, v1]]` of a domain.
fn bounds_of(d: &Extent2d) -> [[f64; 2]; 2] {
    let (lo, hi) = (d.min(), d.max());
    [[lo.x, hi.x], [lo.y, hi.y]]
}

/// The stretch each side of `b` spans along it (in v for the sides of constant u).
fn side_ranges(b: Scratch) -> [[f64; 2]; 4] {
    let (u, v) = ([b.u.lo, b.u.hi], [b.v.lo, b.v.hi]);
    [v, v, u, u]
}

/// Whether the stretches `across` (by their start) cover `[a, z]`.
fn covered(across: &[(usize, [f64; 2])], [a, z]: [f64; 2]) -> bool {
    let mut reach = a;
    for &(_, [c0, c1]) in across {
        if reach >= z {
            break;
        }
        if c0 > reach {
            return false;
        }
        if c1 > reach {
            reach = c1;
        }
    }
    reach >= z
}

/// One kind of side of every leaf (the sides u = u1, u = u0, v = v1 or v = v0), indexed for
/// [`leaf_sides`]: by the side's line, then by the start of its stretch along the line, then
/// by the leaf, each with its stretch, and the furthest a stretch reaches among those before
/// it on the same line (itself included). A query visits only the entries of its line from
/// the first whose stretches before it could reach the query's start to the last that
/// starts at or before the query's end, so a side meeting few others costs a binary search
/// and those few, not a scan of every side on its line.
struct SideIndex {
    /// `(line, stretch start, stretch end, leaf)`, by the line, the start (`total_cmp`) and
    /// the leaf.
    keys: Vec<(f64, f64, f64, usize)>,
    /// For each key, the greatest stretch end among the keys of its line up to it (from
    /// `−∞`: a NaN end is passed over, it meets nothing).
    reach: Vec<f64>,
}

impl SideIndex {
    /// The index of the sides `line` picks out of each domain of `all`, each with the
    /// stretch `stretch` picks along it.
    fn of(
        all: &[[[f64; 2]; 2]],
        line: fn(&[[f64; 2]; 2]) -> f64,
        stretch: fn(&[[f64; 2]; 2]) -> [f64; 2],
    ) -> Self {
        let mut keys: Vec<(f64, f64, f64, usize)> = all
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let [b0, b1] = stretch(b);
                (line(b), b0, b1, i)
            })
            .collect();
        keys.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then(a.1.total_cmp(&b.1))
                .then(a.3.cmp(&b.3))
        });
        let mut reach = Vec::new();
        let mut prev: Option<f64> = None;
        let mut most = f64::NEG_INFINITY;
        for &(x, _, b1, _) in &keys {
            if prev.is_none_or(|p| p.total_cmp(&x).is_ne()) {
                most = f64::NEG_INFINITY;
            }
            most = most.max(b1);
            reach.push(most);
            prev = Some(x);
        }
        Self { keys, reach }
    }

    /// The entries of the line `line` that may meet the stretch `[o0, o1]`: every entry on
    /// it whose stretch meets `[o0, o1]` (ends included) is among them, in the index's
    /// order. Those passed over before the first returned all end below `o0` (their reach
    /// does), and those after the last all start past `o1`.
    fn candidates(&self, line: f64, [o0, o1]: [f64; 2]) -> &[(f64, f64, f64, usize)] {
        let first = self.keys.partition_point(|k| k.0.total_cmp(&line).is_lt());
        let last = self.keys.partition_point(|k| k.0.total_cmp(&line).is_le());
        let (Some(keys), Some(reach)) = (self.keys.get(first..last), self.reach.get(first..last))
        else {
            return &[];
        };
        let from = reach.partition_point(|&r| r < o0);
        // (`total_cmp` puts −0 before +0, which `<=` holds equal: either order counts, so
        // the entries kept are a prefix of the line's that holds every start `<= o1`)
        let to = keys.partition_point(|k| k.1.total_cmp(&o1).is_le() || k.1 <= o1);
        keys.get(from..to.max(from)).unwrap_or_default()
    }
}

/// What lies across each searched leaf's sides, from the domains of every leaf the
/// traversal searched (`domains`) and the solve's `(u, v)` domain: two leaves are across a
/// side from each other when that side's line is the same number in both and their
/// stretches along it meet. Where the solve wraps (`seams`), a side on one end of the
/// knots' range is across from the sides on the other end (a leaf over the whole range from
/// itself), and lies inside the solve: a stretch of it that no searched leaf or strip
/// covers across the seam faces a trim cull. The sides on each line are found through a
/// [`SideIndex`], so the work is the sort and the sides that meet, not the square of the
/// leaves on one line (full strips along a seam all share the seam's line), wherever the
/// stretches on a line meet only at their ends, as the domains of distinct leaves of the
/// tree do; stretches that overlap on one line (a domain repeated, or one stretch along
/// the whole line) are still visited by each query that reaches past them.
pub(crate) fn leaf_sides(domains: &[Extent2d], solve: &Extent2d, seams: Seams) -> Vec<LeafSides> {
    let all: Vec<[[f64; 2]; 2]> = domains.iter().map(bounds_of).collect();
    // each leaf's sides by their line: u1, u0, v1, v0 (what a side u = u0 meets is a u1),
    // with their stretches along it (in v for the sides of constant u)
    let in_v = |&[_, v]: &[[f64; 2]; 2]| v;
    let in_u = |&[u, _]: &[[f64; 2]; 2]| u;
    let by: [SideIndex; 4] = [
        SideIndex::of(&all, |&[[_, u1], _]| u1, in_v),
        SideIndex::of(&all, |&[[u0, _], _]| u0, in_v),
        SideIndex::of(&all, |&[_, [_, v1]]| v1, in_u),
        SideIndex::of(&all, |&[_, [v0, _]]| v0, in_u),
    ];
    let (lo, hi) = (solve.min(), solve.max());
    let [[r0, r1], [q0, q1]] = seams.range;
    let [wu, wv] = seams.wrap;
    all.iter()
        .enumerate()
        .map(|(i, &bounds)| {
            let [[u0, u1], [v0, v1]] = bounds;
            let lines = [u0, u1, v0, v1];
            // each side on a seam, with the line of the other end it is glued to
            let glued = [
                (wu && u0 <= r0).then_some(r1),
                (wu && u1 >= r1).then_some(r0),
                (wv && v0 <= q0).then_some(q1),
                (wv && v1 >= q1).then_some(q0),
            ];
            let mut across: [Vec<(usize, [f64; 2])>; 4] = Default::default();
            for (s, slot) in across.iter_mut().enumerate() {
                let (Some(&own), Some(index)) = (lines.get(s), by.get(s)) else {
                    continue;
                };
                let seam = glued.get(s).copied().flatten();
                let line = seam.unwrap_or(own);
                let [o0, o1] = if s < 2 { [v0, v1] } else { [u0, u1] };
                for &(_, b0, b1, j) in index.candidates(line, [o0, o1]) {
                    let Some(&[[a0, a1], [c0, c1]]) = all.get(j) else {
                        continue;
                    };
                    // (a domain with no area covers no stretch: nothing was searched there;
                    // across a seam a leaf over the whole range faces itself)
                    let area = a0 < a1 && c0 < c1;
                    if (j != i || seam.is_some()) && area && b0 <= o1 && b1 >= o0 {
                        slot.push((j, [b0, b1]));
                    }
                }
                slot.sort_by(|(i, [a, _]), (j, [b, _])| a.total_cmp(b).then(i.cmp(j)));
            }
            let seam = glued.map(|g| g.is_some());
            let [g0, g1, g2, g3] = seam;
            LeafSides {
                bounds,
                inside: [
                    u0 > lo.x || g0,
                    u1 < hi.x || g1,
                    v0 > lo.y || g2,
                    v1 < hi.y || g3,
                ],
                across,
                seam,
            }
        })
        .collect()
}

/// The components of a vector of intervals.
fn i3(v: V3<Ivl>) -> I3 {
    [v.x, v.y, v.z]
}

/// The components of a vector.
fn f3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The vector of three components.
fn v3(a: [f64; 3]) -> Vec3 {
    let [x, y, z] = a;
    Vec3::new(x, y, z)
}

/// `a · b`, enclosed.
fn dot(a: [f64; 3], b: &I3) -> Ivl {
    a.iter()
        .zip(b)
        .fold(Ivl::from(0.0), |s, (&x, &y)| s + Ivl::from(x) * y)
}

/// `a × b`, enclosed.
fn cross(a: [f64; 3], b: &I3) -> I3 {
    let [ax, ay, az] = a.map(Ivl::from);
    let [bx, by, bz] = *b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

/// `a + b`, enclosed.
fn add(a: &I3, b: &I3) -> I3 {
    let [a0, a1, a2] = *a;
    let [b0, b1, b2] = *b;
    [a0 + b0, a1 + b1, a2 + b2]
}

/// `a − t d`, enclosed.
fn less(a: &I3, t: Ivl, d: [f64; 3]) -> I3 {
    let [a0, a1, a2] = *a;
    let [d0, d1, d2] = d.map(Ivl::from);
    [a0 - t * d0, a1 - t * d1, a2 - t * d2]
}

/// The squares of the least and of the greatest length a vector of `v` can have, each
/// rounded outward.
fn length_squared(v: &I3) -> Ivl {
    let sum = |f: fn(Ivl) -> f64| {
        v.iter().fold(Ivl::from(0.0), |s, &x| {
            let m = Ivl::from(f(x));
            s + m * m
        })
    };
    Ivl::new(sum(Ivl::mig).lo, sum(Ivl::mag).hi)
}

/// The middles of a vector of intervals.
fn mids(v: &I3) -> [f64; 3] {
    v.map(Ivl::mid)
}

/// `a` strictly inside `b`, component by component (a NaN end is never inside).
fn inside(a: &I3, b: &I3) -> bool {
    a.iter().zip(b).all(|(x, y)| x.lo > y.lo && x.hi < y.hi)
}

/// `a` apart from `b` in some component (a NaN end is never apart).
fn apart(a: &I3, b: &I3) -> bool {
    a.iter().zip(b).any(|(x, y)| x.hi < y.lo || x.lo > y.hi)
}

/// `a` within `b`, ends included.
fn within(a: Ivl, b: Ivl) -> bool {
    a.lo >= b.lo && a.hi <= b.hi
}

/// The common part of `a` and `b`, `None` where they are apart.
fn meet(a: Ivl, b: Ivl) -> Option<Ivl> {
    let (lo, hi) = (a.lo.max(b.lo), a.hi.min(b.hi));
    (lo <= hi).then_some(Ivl::new(lo, hi))
}

/// The line `P + t D` of a pass, with what its tests read.
#[derive(Debug, Clone, Copy)]
struct ScratchLine {
    p: [f64; 3],
    d: [f64; 3],
    /// `|D|²`, enclosed.
    d2: Ivl,
    /// The leaf's line interval: a root off it is unshown.
    interval: [f64; 2],
    /// The line's own interval ([`PassLine::span`]): a box off it is excluded by t.
    span: [f64; 2],
    /// The tolerance as a distance, and as a stretch of the line's parameter (rounded up).
    tol: f64,
    tol_t: f64,
    /// `(tol |D|)²`, rounded up.
    tol_cross2: f64,
    /// The solve's `(u, v)` domain ([`PassLine::solve_domain`]).
    solve_domain: Extent2d,
}

impl ScratchLine {
    /// The line of `line` with the leaf's `interval`; `None` where nothing can be proved of
    /// it (no tolerance, a line of no length, an empty interval).
    fn of(line: &PassLine, interval: &Extent1d) -> Option<Self> {
        let tol = line.tol;
        let d = f3(line.vector);
        let d2 = length_squared(&d.map(Ivl::from));
        let usable = tol > 0.0 && tol.is_finite() && d2.lo > 0.0 && d2.is_finite();
        let [s0, s1] = line.span;
        if !usable || !(interval.min() <= interval.max()) || !(s0 <= s1) {
            return None;
        }
        let tol_i = Ivl::from(tol);
        Some(Self {
            p: f3(line.point),
            d,
            d2,
            interval: [interval.min(), interval.max()],
            span: line.span,
            tol,
            tol_t: (tol_i / d2.sqrt()).hi,
            tol_cross2: (tol_i * tol_i * d2).hi,
            solve_domain: line.solve_domain,
        })
    }

    /// Whether the points `r + P` are all farther than the tolerance from the line.
    fn farther(&self, r: &I3) -> bool {
        length_squared(&cross(self.d, r)).lo > self.tol_cross2
    }

    /// The line parameters of the feet of the points `r + P` widened by the tolerance:
    /// as Krawczyk's steps start from them, and rounded outward (the enclosure the
    /// verdicts read).
    fn feet(&self, r: &I3) -> (Ivl, Ivl) {
        let foot = self.parameter(r);
        (
            Ivl::new(foot.lo - self.tol_t, foot.hi + self.tol_t),
            Ivl::new(
                round_down(foot.lo - self.tol_t),
                round_up(foot.hi + self.tol_t),
            ),
        )
    }

    /// `S − P` from an enclosure of `S − about`.
    fn off_the_start(&self, s: &I3, about: [f64; 3]) -> I3 {
        let [a0, a1, a2] = about;
        let [p0, p1, p2] = self.p;
        let shift = [
            Ivl::from(a0) - Ivl::from(p0),
            Ivl::from(a1) - Ivl::from(p1),
            Ivl::from(a2) - Ivl::from(p2),
        ];
        add(s, &shift)
    }

    /// The line parameters of the feet of the points `r + P`, enclosed.
    fn parameter(&self, r: &I3) -> Ivl {
        dot(self.d, r) / self.d2
    }
}

/// A crossing of the line and the surface, proved to exist and to be the only one in a box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CertifiedRoot {
    /// `(u, v, t)`: Newton's root, inside [`CertifiedRoot::enclosure`].
    pub point: [f64; 3],
    /// A box that holds the root.
    pub enclosure: [Ivl; 3],
    /// The `(u, v)` box in which the root is the only one, with any line parameter.
    pub unique: [Ivl; 2],
}

/// The line every leaf's pass of one solve searches, with what bounds it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PassLine {
    /// The line's point: the line is `point + t vector`.
    pub point: Point3,
    /// The line's vector.
    pub vector: Vec3,
    /// The line's own interval of `t` (module docs, step 1): the solve's line interval
    /// where it has one, from just behind the start for a ray; an end without a bound is
    /// infinite.
    pub span: [f64; 2],
    /// The solve's `(u, v)` domain, whose border is an edge's ([`FloorKind::Edge`]).
    pub solve_domain: Extent2d,
    /// The tolerance: a line passing within it of the surface is a hit.
    pub tol: f64,
}

/// How a leaf's pass ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Stop {
    /// Not run: nothing can be proved of the line or the domain (no tolerance, a line of no
    /// length, a domain off the net's knots, an empty interval). The guesses alone answer.
    #[default]
    NotRun,
    /// Every box was decided, excluded or left.
    Finished,
    /// Stopped at [`MAX_SCRATCH_BOXES`] with boxes still to visit ([`FloorKind::Unvisited`]).
    AtLimit,
}

/// What a box the pass did not exclude by distance is (module docs, "What the pass
/// decided").
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FloorKind {
    /// Its line parameters lie wholly outside the line's own interval (step 1).
    ExcludedByT,
    /// Left at the floor (step 3).
    Left,
    /// No root (step 2), with a side on a border where a path down the gap can end.
    Edge,
    /// No root (step 2), and no side on such a border.
    NoRoot,
    /// Its root, certified and reported, the only one in `unique` with any line parameter,
    /// and held by `enclosure` (in `(u, v, t)`).
    Root {
        unique: [Ivl; 2],
        enclosure: [Ivl; 3],
    },
    /// Its root, certified but not reported ([`Isolation::unshown`]), the only one in
    /// `unique`, and held by `enclosure` (in `(u, v, t)`).
    Unshown {
        unique: [Ivl; 2],
        enclosure: [Ivl; 3],
    },
    /// Not visited: the pass stopped at [`MAX_SCRATCH_BOXES`] first.
    Unvisited,
    /// A box of a culled strip ([`enclose_strip`]) that step 1 did not exclude: at the
    /// floor's size, not to be enclosed, or still to visit when the strip's budget ran out.
    Culled,
}

/// A box the pass did not exclude by distance, as it decided it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FloorBox {
    /// The box in u.
    pub u: Ivl,
    /// The box in v.
    pub v: Ivl,
    /// Its line parameters: the feet of its points on the line, widened by the tolerance;
    /// `None` for a box whose line parameters were never enclosed (unvisited, or left
    /// because its enclosure was refused or not finite).
    pub t: Option<Ivl>,
    /// What the pass decided of it.
    pub kind: FloorKind,
}

/// What one leaf's pass found.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Isolation {
    /// The certified crossings, each once, in the order found.
    pub roots: Vec<CertifiedRoot>,
    /// Boxes enclosed.
    pub boxes: usize,
    /// Boxes left at the floor (module docs, step 3).
    pub left: usize,
    /// Certified roots not reported because the gap at Newton's point was not enclosed
    /// within the tolerance, or the point lies off the leaf's line interval.
    pub unshown: usize,
    /// How the pass ended.
    pub stop: Stop,
    /// Every box not excluded by distance, in the order decided, then the boxes still to
    /// visit at the limit; after a finished pass, what its exclusion phase kept (the solve's
    /// drop, [`drop_fenced`], comes after every leaf's pass).
    pub floor: Vec<FloorBox>,
    /// What the exclusion phase did (module docs, "The exclusion phase").
    pub exclusion: Exclusion,
}

/// What a leaf's exclusion phase did (module docs, "The exclusion phase").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Exclusion {
    /// Whether it ran: only after a finished root pass.
    pub ran: bool,
    /// The enclosures it made: at most its budget.
    pub boxes: usize,
    /// Whether it stopped at its budget with work left. What it had not decided is kept;
    /// the root pass stays finished.
    pub out_of_budget: bool,
    /// Pieces of boxes with no root it proved farther than the tolerance.
    pub excluded: usize,
}

/// What Krawczyk's test says of a regular box.
enum Regular {
    /// No root in the box.
    Empty,
    /// Exactly one, with its enclosure and the `(u, v)` box it is unique in.
    Root(I3, [Ivl; 2]),
    /// Not decided.
    Open,
}

/// A parameter box of the work list.
#[derive(Clone, Copy)]
struct Scratch {
    u: Ivl,
    v: Ivl,
}

/// The work list of a leaf's pass (module docs, "The order of the boxes"): the box of least
/// line parameter first (its key: the least line parameter of the box it was cut from,
/// before it is enclosed itself), and of equal keys the last pushed, so that a pass stopped
/// at its limit has decided every box whose points meet the line before the boxes it left.
/// Each box carries its place in the cutting (the halves taken from the whole leaf, `false`
/// the first). The order changes which boxes a pass at its limit reaches, never what it
/// decides of a box ([`visit`] reads the box alone), so a finished pass decides the same
/// boxes in any order.
#[derive(Default)]
struct Queue {
    heap: std::collections::BinaryHeap<Queued>,
    pushed: u64,
}

/// A box of the [`Queue`] with its key, its place in the order pushed and in the cutting.
struct Queued {
    key: f64,
    seq: u64,
    b: Scratch,
    path: Vec<bool>,
}

impl Ord for Queued {
    /// The greater pops first: the lesser key, then the later pushed.
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        other
            .key
            .total_cmp(&self.key)
            .then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == core::cmp::Ordering::Equal
    }
}

impl Eq for Queued {}

impl Queue {
    fn push(&mut self, b: Scratch, key: f64, path: Vec<bool>) {
        self.pushed = self.pushed.wrapping_add(1);
        self.heap.push(Queued {
            key,
            seq: self.pushed,
            b,
            path,
        });
    }

    fn pop(&mut self) -> Option<(Scratch, f64, Vec<bool>)> {
        self.heap.pop().map(|q| (q.b, q.key, q.path))
    }

    /// The boxes still to visit, in the order they would have been visited.
    fn into_boxes(self) -> Vec<Scratch> {
        let mut left = self.heap.into_sorted_vec();
        left.reverse();
        left.into_iter().map(|q| q.b).collect()
    }
}

/// The certified crossings of `line` with the surface of `net` over the leaf's `domain`,
/// reported for `t` in the leaf's `interval` (module docs), with what the pass decided of
/// every box; `sides` is what lies across the leaf's sides.
///
/// # Errors
///
/// A cancel.
pub(crate) fn isolate(
    net: &LineNet,
    line: &PassLine,
    domain: &Extent2d,
    interval: &Extent1d,
    sides: &LeafSides,
    cancel: Option<&CancelToken>,
) -> Result<Isolation> {
    isolate_within(
        net,
        line,
        domain,
        interval,
        sides,
        cancel,
        MAX_EXCLUSION_BOXES,
    )
}

/// [`isolate`] with the exclusion phase's budget `exclusion_budget`.
fn isolate_within(
    net: &LineNet,
    line: &PassLine,
    domain: &Extent2d,
    interval: &Extent1d,
    sides: &LeafSides,
    cancel: Option<&CancelToken>,
    exclusion_budget: usize,
) -> Result<Isolation> {
    let mut out = Isolation::default();
    let Some(line) = ScratchLine::of(line, interval) else {
        // nothing can be proved of such a line: the guesses alone answer
        return Ok(out);
    };
    let (lo, hi) = (domain.min(), domain.max());
    let (u, v) = (Ivl::new(lo.x, hi.x), Ivl::new(lo.y, hi.y));
    if !net.holds(u, v) {
        // nor of a domain off the net's knots
        return Ok(out);
    }
    let mut work = Queue::default();
    work.push(Scratch { u, v }, f64::NEG_INFINITY, Vec::new());
    // the boxes decided and the roots certified, each with its place in the cutting (the
    // order a pass that visits the first half's boxes before the second's decides them in)
    let mut decided: Vec<(Vec<bool>, FloorBox)> = Vec::new();
    let mut certified: Vec<(Vec<bool>, CertifiedRoot)> = Vec::new();
    let mut left_to_visit: Vec<Scratch> = Vec::new();
    out.stop = Stop::Finished;
    while let Some((b, key, path)) = work.pop() {
        if out.boxes >= MAX_SCRATCH_BOXES {
            out.stop = Stop::AtLimit;
            work.push(b, key, path);
            left_to_visit = work.into_boxes();
            break;
        }
        if out.boxes % 64 == 0 {
            if let Some(token) = cancel {
                token.check(OP)?;
            }
        }
        out.boxes += 1;
        let (kind, t) = match visit(net, &line, b) {
            Visit::ExcludedByDistance => continue,
            Visit::ExcludedByT(t) => (FloorKind::ExcludedByT, Some(t)),
            Visit::NoRoot(t) => {
                let kind = if net.on_a_border(&line.solve_domain, sides, b) {
                    FloorKind::Edge
                } else {
                    FloorKind::NoRoot
                };
                (kind, Some(t))
            }
            Visit::Root(root, t) => {
                let (unique, enclosure) = (root.unique, root.enclosure);
                certified.push((path.clone(), root));
                (FloorKind::Root { unique, enclosure }, Some(t))
            }
            Visit::Unshown(unique, enclosure, t) => {
                out.unshown += 1;
                (FloorKind::Unshown { unique, enclosure }, Some(t))
            }
            Visit::Left(t) => {
                out.left += 1;
                (FloorKind::Left, t)
            }
            Visit::Halve(first, second, feet) => {
                // (halves inherit the least line parameter of the box they are cut from)
                let key = feet.map_or(key, |t| t.lo);
                let mut second_path = path.clone();
                second_path.push(true);
                let mut first_path = path;
                first_path.push(false);
                work.push(second, key, second_path);
                work.push(first, key, first_path);
                continue;
            }
        };
        decided.push((
            path,
            FloorBox {
                u: b.u,
                v: b.v,
                t,
                kind,
            },
        ));
    }
    // the records and the roots in the order of the cutting, first halves first: what a
    // finished pass records and reports is then the same in whatever order it visited (the
    // exclusion phase reads the records in this order within its budget, and of two boxes
    // holding one root the first reports it); no decided box's place is the start of
    // another's (a decided box is not cut), so the places are apart
    decided.sort_by(|a, b| a.0.cmp(&b.0));
    certified.sort_by(|a, b| a.0.cmp(&b.0));
    out.floor = decided.into_iter().map(|(_, f)| f).collect();
    // the boxes left to visit, each with its line parameters, so that a ray can tell whether
    // it lies before a hit (the whole line where they cannot be enclosed)
    out.floor.extend(left_to_visit.into_iter().map(|w| {
        FloorBox {
            u: w.u,
            v: w.v,
            t: Some(
                enclosed(net, &line, w.u, w.v)
                    .map(|e| line.feet(&e.r).1)
                    .filter(|feet| feet.is_finite())
                    .unwrap_or_else(Ivl::whole),
            ),
            kind: FloorKind::Unvisited,
        }
    }));
    for (_, root) in certified {
        // one root, however many boxes hold it on their borders: the same root when its
        // enclosure lies in a box another is the only root of (Rump, Thm 13.3), or when the
        // hull of the two enclosures is regular (it then holds one root at most); roots with
        // enclosures apart are two
        let known = out.roots.iter().any(|r| {
            let [ru, rv, _] = root.enclosure;
            let [qu, qv] = r.unique;
            let [su, sv, _] = r.enclosure;
            (within(ru, qu) && within(rv, qv))
                || (meet(ru, su).is_some()
                    && meet(rv, sv).is_some()
                    && regular_over(net, &line, ru.hull(su), rv.hull(sv)))
        });
        if !known {
            out.roots.push(root);
        }
    }
    if out.stop == Stop::Finished {
        let mut phase = Exclude {
            net,
            line: &line,
            sides,
            budget: exclusion_budget,
            cancel,
            done: Exclusion {
                ran: true,
                ..Exclusion::default()
            },
        };
        phase.run(&mut out.floor)?;
        out.exclusion = phase.done;
    }
    Ok(out)
}

/// Step 1 of the pass over a culled strip, the `(u, v)` box `domain` (module docs, "Culled
/// strips"): every box excluded by distance, recorded as excluded by t, or kept undecided
/// ([`FloorKind::Culled`]). The strip has no roots and no exclusion phase; it is
/// [`Stop::Finished`] when every box was decided or kept at the floor, [`Stop::AtLimit`]
/// when its budget ran out (the boxes left to visit kept undecided, with their line
/// parameters), and [`Stop::NotRun`] where nothing can be proved of the line or the domain.
///
/// # Errors
///
/// A cancel.
pub(crate) fn enclose_strip(
    net: &LineNet,
    line: &PassLine,
    domain: &Extent2d,
    cancel: Option<&CancelToken>,
) -> Result<Isolation> {
    enclose_strip_within(net, line, domain, cancel, MAX_STRIP_BOXES)
}

/// [`enclose_strip`] with the budget `budget`.
fn enclose_strip_within(
    net: &LineNet,
    line: &PassLine,
    domain: &Extent2d,
    cancel: Option<&CancelToken>,
    budget: usize,
) -> Result<Isolation> {
    let mut out = Isolation::default();
    let [s0, s1] = line.span;
    let Some(line) = ScratchLine::of(line, &Extent1d::from_min_max(s0, s1)) else {
        return Ok(out);
    };
    let (lo, hi) = (domain.min(), domain.max());
    let (u, v) = (Ivl::new(lo.x, hi.x), Ivl::new(lo.y, hi.y));
    if !net.holds(u, v) {
        return Ok(out);
    }
    let culled = |b: Scratch, t: Option<Ivl>| FloorBox {
        u: b.u,
        v: b.v,
        t,
        kind: FloorKind::Culled,
    };
    let mut work = vec![Scratch { u, v }];
    out.stop = Stop::Finished;
    while let Some(b) = work.pop() {
        if out.boxes >= budget {
            out.stop = Stop::AtLimit;
            work.push(b);
            // each with its line parameters (the whole line where they cannot be enclosed)
            out.floor.extend(work.iter().map(|&w| {
                let t = enclosed(net, &line, w.u, w.v)
                    .map(|e| line.feet(&e.r).1)
                    .filter(|feet| feet.is_finite())
                    .unwrap_or_else(Ivl::whole);
                culled(w, Some(t))
            }));
            break;
        }
        if out.boxes % 64 == 0 {
            if let Some(token) = cancel {
                token.check(OP)?;
            }
        }
        out.boxes += 1;
        // a box that cannot be enclosed is halved across its wider side
        let wider = |t: Option<Ivl>| {
            let split_u = b.u.hi - b.u.lo >= b.v.hi - b.v.lo;
            halves(b, split_u, if split_u { b.u.mid() } else { b.v.mid() }, t)
        };
        // (step 1 reads the surface's points alone: their partials only where it halves)
        let Some(r) = enclosed_value(net, &line, b.u, b.v) else {
            match wider(None) {
                Visit::Halve(first, second, _) => {
                    work.push(second);
                    work.push(first);
                }
                _ => out.floor.push(culled(b, None)),
            }
            continue;
        };
        if line.farther(&r) {
            continue;
        }
        let (_, feet) = line.feet(&r);
        if !feet.is_finite() {
            out.floor.push(culled(b, None));
            continue;
        }
        if feet.hi < s0 || feet.lo > s1 {
            out.floor.push(FloorBox {
                u: b.u,
                v: b.v,
                t: Some(feet),
                kind: FloorKind::ExcludedByT,
            });
            continue;
        }
        // halved as step 4 halves, down to the floor of step 3 (across its wider side where
        // its partials cannot be enclosed)
        let halved = match enclosed(net, &line, b.u, b.v) {
            Some(e) => cuts(&line, b, &e.s_u, &e.s_v)
                .filter(|c| c.cut_u || c.cut_v)
                .map(|c| {
                    halves(
                        b,
                        c.split_u,
                        if c.split_u { b.u.mid() } else { b.v.mid() },
                        Some(feet),
                    )
                }),
            None => Some(wider(Some(feet))),
        };
        match halved {
            Some(Visit::Halve(first, second, _)) => {
                work.push(second);
                work.push(first);
            }
            _ => out.floor.push(culled(b, Some(feet))),
        }
    }
    Ok(out)
}

/// The exclusion phase of one leaf (module docs, "The exclusion phase").
struct Exclude<'a> {
    net: &'a LineNet,
    line: &'a ScratchLine,
    sides: &'a LeafSides,
    /// The most enclosures it makes.
    budget: usize,
    cancel: Option<&'a CancelToken>,
    /// What it did so far.
    done: Exclusion,
}

impl Exclude<'_> {
    /// Takes one enclosure from the budget; false (and out of budget) when none is left.
    fn spend(&mut self) -> Result<bool> {
        if self.done.boxes >= self.budget {
            self.done.out_of_budget = true;
            return Ok(false);
        }
        if self.done.boxes % 64 == 0 {
            if let Some(token) = self.cancel {
                token.check(OP)?;
            }
        }
        self.done.boxes += 1;
        Ok(true)
    }

    /// The phase over the records `floor` of a finished root pass: the boxes with no root
    /// cut down to the floor, each piece proved farther than the tolerance dropped.
    fn run(&mut self, floor: &mut Vec<FloorBox>) -> Result<()> {
        let (todo, mut kept): (Vec<FloorBox>, Vec<FloorBox>) = std::mem::take(floor)
            .into_iter()
            .partition(|r| matches!(r.kind, FloorKind::NoRoot | FloorKind::Edge));
        let mut todo: Vec<FloorBox> = todo.into_iter().rev().collect();
        while let Some(r) = todo.pop() {
            if !self.spend()? {
                // out of budget: what is left is kept as it is
                kept.push(r);
                kept.extend(std::mem::take(&mut todo).into_iter().rev());
                break;
            }
            match self.cut(r) {
                Cut::Excluded => self.done.excluded += 1,
                Cut::Kept(piece) => kept.push(piece),
                Cut::Halves(first, second) => {
                    todo.push(second);
                    todo.push(first);
                }
            }
        }
        *floor = kept;
        Ok(())
    }

    /// One box with no root (a piece of one: it holds no root and is regular, as the box
    /// it was cut from is): excluded by distance, kept at the floor (or where it cannot be
    /// enclosed or cut), or cut in two.
    fn cut(&self, r: FloorBox) -> Cut {
        let (net, line) = (self.net, self.line);
        let b = Scratch { u: r.u, v: r.v };
        let Some(e) = enclosed(net, line, b.u, b.v) else {
            return Cut::Kept(r);
        };
        if line.farther(&e.r) {
            return Cut::Excluded;
        }
        let (_, feet) = line.feet(&e.r);
        if !feet.is_finite() {
            return Cut::Kept(r);
        }
        let [s0, s1] = line.span;
        let kind_of = |b: Scratch| {
            if net.on_a_border(&line.solve_domain, self.sides, b) {
                FloorKind::Edge
            } else {
                FloorKind::NoRoot
            }
        };
        let kind = if feet.hi < s0 || feet.lo > s1 {
            FloorKind::ExcludedByT
        } else {
            kind_of(b)
        };
        let piece = FloorBox {
            t: Some(feet),
            kind,
            ..r
        };
        if kind == FloorKind::ExcludedByT {
            return Cut::Kept(piece);
        }
        // the floor of step 3: a direction is not cut once its image is within half the
        // tolerance
        let halved = cuts(line, b, &e.s_u, &e.s_v)
            .filter(|c| c.cut_u || c.cut_v)
            .map(|c| {
                halves(
                    b,
                    c.split_u,
                    if c.split_u { b.u.mid() } else { b.v.mid() },
                    None,
                )
            });
        match halved {
            // (each half carries the line parameters of the box it is cut from until it is
            // enclosed: they hold its own)
            Some(Visit::Halve(first, second, _)) => Cut::Halves(
                FloorBox {
                    u: first.u,
                    v: first.v,
                    t: Some(feet),
                    kind: kind_of(first),
                },
                FloorBox {
                    u: second.u,
                    v: second.v,
                    t: Some(feet),
                    kind: kind_of(second),
                },
            ),
            _ => Cut::Kept(piece),
        }
    }
}

/// What the exclusion phase makes of one box with no root.
enum Cut {
    /// Proved farther than the tolerance.
    Excluded,
    /// Kept, as it now is.
    Kept(FloorBox),
    /// Cut in two, the first to visit first.
    Halves(FloorBox, FloorBox),
}

/// The side `s` of the box `b` (in the order of [`LineNet::side_kinds`]), as a box of no
/// width.
fn side_of(b: Scratch, s: usize) -> Scratch {
    match s {
        0 => Scratch {
            u: Ivl::from(b.u.lo),
            ..b
        },
        1 => Scratch {
            u: Ivl::from(b.u.hi),
            ..b
        },
        2 => Scratch {
            v: Ivl::from(b.v.lo),
            ..b
        },
        _ => Scratch {
            v: Ivl::from(b.v.hi),
            ..b
        },
    }
}

/// Whether the closed boxes `a` and `b` meet.
fn boxes_meet(a: Scratch, b: Scratch) -> bool {
    a.u.lo <= b.u.hi && a.u.hi >= b.u.lo && a.v.lo <= b.v.hi && a.v.hi >= b.v.lo
}

/// The most pairs of boxes [`meeting_pairs`] tests: about a thousand for each of the most
/// records a leaf keeps, the root pass's and its exclusion phase's together.
const MAX_MEETING_TESTS: usize = 1 << 23;

/// Calls `found(i, j)`, `i < j`, for every pair of the closed `(u, v)` boxes `rects` that
/// meet, by a sweep in u; false (with pairs left unreported) once it would test more than
/// [`MAX_MEETING_TESTS`] pairs.
fn meeting_pairs(rects: &[[Ivl; 2]], mut found: impl FnMut(usize, usize)) -> bool {
    let lo_u = |i: usize| rects.get(i).map_or(f64::NAN, |[u, _]| u.lo);
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by(|&a, &b| lo_u(a).total_cmp(&lo_u(b)).then(a.cmp(&b)));
    let mut active: Vec<usize> = Vec::new();
    let mut tests = 0_usize;
    for &i in &order {
        let Some(&[ui, vi]) = rects.get(i) else {
            continue;
        };
        tests = tests.saturating_add(active.len());
        if tests > MAX_MEETING_TESTS {
            return false;
        }
        // (sorted by u.lo: a box still active meets this one in u when it reaches it)
        active.retain(|&j| rects.get(j).is_some_and(|[uj, _]| uj.hi >= ui.lo));
        for &j in &active {
            if rects
                .get(j)
                .is_some_and(|[_, vj]| vj.lo <= vi.hi && vj.hi >= vi.lo)
            {
                found(j.min(i), j.max(i));
            }
        }
        active.push(i);
    }
    true
}

/// The least member of `i`'s set (every member points to a smaller one or itself, so the
/// walk ends), halving the path on the way.
fn root_of(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i) {
        if p >= i {
            break;
        }
        let up = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = up;
        }
        i = p;
    }
    i
}

/// Joins the sets of `i` and `j`, named by the lesser of their least members.
fn join(parent: &mut [usize], i: usize, j: usize) {
    let (a, b) = (root_of(parent, i), root_of(parent, j));
    let (keep, gone) = if a <= b { (a, b) } else { (b, a) };
    if let Some(slot) = parent.get_mut(gone) {
        *slot = keep;
    }
}

/// One searched leaf of a solve as the components read it (module docs, "Components").
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SearchedLeaf {
    /// The leaf's domain.
    pub domain: Extent2d,
    /// How its pass ended.
    pub stop: Stop,
    /// What its pass kept ([`Isolation::floor`]); `None` where the solve did not keep it
    /// (past [`MAX_SOLVE_BOXES`]): nothing is then known of the leaf's boxes, and no box is
    /// read as excluded there for having no record.
    pub floor: Option<Vec<FloorBox>>,
    /// Its certified roots ([`Isolation::roots`]).
    pub roots: Vec<CertifiedRoot>,
    /// Whether it is a culled strip ([`enclose_strip`]) rather than a leaf the traversal
    /// solved.
    pub strip: bool,
    /// Whether it lies wholly inside the face (its trim classification, or a solve with no
    /// trim to test): a component in such leaves alone cannot straddle the face's border.
    pub inside: bool,
}

impl SearchedLeaf {
    /// Whether a box of this leaf that no record holds is excluded by distance (module docs,
    /// "The drop"): its pass finished, or stopped at its limit with the boxes it had still
    /// to visit recorded, and its records are kept. A pass that did not run proves nothing.
    fn reads_absence(&self) -> bool {
        self.floor.is_some() && matches!(self.stop, Stop::Finished | Stop::AtLimit)
    }
}

/// The certified roots each searched leaf of one solve adds (module docs, "The order of the
/// boxes"), in the order of `leaves`: every root of a leaf whose pass finished; of a leaf
/// whose pass stopped at its limit, those whose line parameters lie wholly below `b`, the
/// least line parameter of the boxes left to visit by EVERY leaf of the solve at its limit
/// (each box records the feet of its points widened by the tolerance, the whole line where
/// they cannot be enclosed), lowered to the least line parameter of any of those leaves'
/// roots reaching it until none does; none from a leaf whose pass did not run, or from any
/// leaf at its limit where one such leaf's records were not kept (nothing is then known of
/// the boxes it left). A crossing's line parameter is the foot of its point, so a crossing in
/// a box left to visit by any leaf has it at `b` or past it: every crossing of the solve's
/// searched leaves below `b` lies in a box a pass decided: no root added from a leaf at its
/// limit comes after a crossing a box left to visit may hold. The culled strips and the
/// leaves whose pass did not run are no part of the bound: what they hold is the guesses'
/// answer, as before the pass. A finished leaf's roots are all added, as before this bound
/// existed, whatever boxes a leaf at its limit left: such a root can come after a crossing
/// left in a box a leaf at its limit did not visit, as it could before (where a leaf at its
/// limit added nothing).
pub(crate) fn publishable_roots(leaves: &[SearchedLeaf]) -> Vec<Vec<CertifiedRoot>> {
    let at_limit = || {
        leaves
            .iter()
            .filter(|l| !l.strip && l.stop == Stop::AtLimit)
    };
    let mut bound = f64::INFINITY;
    for leaf in at_limit() {
        let least = match &leaf.floor {
            Some(floor) => floor
                .iter()
                .filter(|f| f.kind == FloorKind::Unvisited)
                .map(|f| f.t.map_or(f64::NEG_INFINITY, |t| t.lo))
                .fold(f64::INFINITY, f64::min),
            None => f64::NEG_INFINITY,
        };
        bound = bound.min(least);
    }
    let line_parameters = |r: &CertifiedRoot| {
        let [_, _, t] = r.enclosure;
        t
    };
    // lowered until no root reaches it (each step lowers it to a root's start, or ends: at
    // most one step a root)
    loop {
        let lower = at_limit()
            .flat_map(|l| l.roots.iter())
            .map(line_parameters)
            .filter(|t| t.hi >= bound)
            .fold(bound, |b, t| b.min(t.lo));
        if lower.total_cmp(&bound).is_ge() {
            break;
        }
        bound = lower;
    }
    leaves
        .iter()
        .map(|leaf| match leaf.stop {
            Stop::Finished => leaf.roots.clone(),
            Stop::AtLimit => leaf
                .roots
                .iter()
                .filter(|r| line_parameters(r).hi < bound)
                .copied()
                .collect(),
            Stop::NotRun => Vec::new(),
        })
        .collect()
}

/// What a solve's drop did (module docs, "The drop").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Drops {
    /// The side enclosures it made: at most its budget.
    pub enclosures: usize,
    /// Whether it stopped at its budget (a set it had not shown farther is kept) or did not
    /// run past its bounds on boxes or on the pairs it tests.
    pub out_of_budget: bool,
    /// Plain boxes with no root it dropped.
    pub no_root: usize,
    /// Edge boxes it dropped.
    pub edge: usize,
    /// Of the boxes it dropped, those in sets that read another leaf's records: a set over
    /// more than one leaf, or with a member's side on its leaf's border read across it.
    pub across: usize,
}

/// The solve's drop (module docs, "The drop"): drops from the records of `leaves` (every
/// searched leaf and culled strip of one solve, `sides` what lies across each) every
/// connected set of regular boxes with no root, plain or edge, whose least gap to `line` is
/// shown farther than the tolerance by its boundary and crease sides and by the excluded
/// boxes around it. Decided on the records as they are, then applied at once; nothing runs
/// where a leaf's records are not kept and its boxes are past [`MAX_SOLVE_BOXES`], and at
/// most `budget` side enclosures are made.
///
/// # Errors
///
/// A cancel.
pub(crate) fn drop_fenced(
    net: &LineNet,
    line: &PassLine,
    leaves: &mut [SearchedLeaf],
    sides: &[LeafSides],
    cancel: Option<&CancelToken>,
    budget: usize,
) -> Result<Drops> {
    let mut done = Drops::default();
    let [s0, s1] = line.span;
    let Some(scratch) = ScratchLine::of(line, &Extent1d::from_min_max(s0, s1)) else {
        return Ok(done);
    };
    let gone = {
        let view: &[SearchedLeaf] = leaves;
        // every record of every leaf whose records are kept, in leaf order
        let mut recs: Vec<(usize, FloorBox)> = Vec::new();
        for (l, leaf) in view.iter().enumerate() {
            if let Some(floor) = &leaf.floor {
                recs.extend(floor.iter().map(|&r| (l, r)));
                if recs.len() > MAX_SOLVE_BOXES {
                    done.out_of_budget = true;
                    return Ok(done);
                }
            }
        }
        let n = recs.len();
        let member = |i: usize| {
            recs.get(i)
                .is_some_and(|(_, r)| matches!(r.kind, FloorKind::NoRoot | FloorKind::Edge))
        };
        let scratch_of = |i: usize| recs.get(i).map(|(_, r)| Scratch { u: r.u, v: r.v });
        let rects: Vec<[Ivl; 2]> = recs.iter().map(|(_, r)| [r.u, r.v]).collect();
        let mut near: Vec<Vec<usize>> = vec![Vec::new(); n];
        let within = meeting_pairs(&rects, |i, j| {
            if let Some(list) = near.get_mut(i) {
                list.push(j);
            }
            if let Some(list) = near.get_mut(j) {
                list.push(i);
            }
        });
        if !within {
            done.out_of_budget = true;
            return Ok(done);
        }
        // the connected sets of boxes with no root, each named by its least box
        let mut parent: Vec<usize> = (0..n).collect();
        for (i, list) in near.iter().enumerate() {
            for &j in list {
                if member(i) && member(j) {
                    join(&mut parent, i, j);
                }
            }
        }
        let set: Vec<usize> = (0..n).map(|i| root_of(&mut parent, i)).collect();
        let readable = |l: usize| view.get(l).is_some_and(SearchedLeaf::reads_absence);
        // what keeps each set (conditions 2 and 3 as far as the records tell them), whether
        // it reads across a leaf's border, and the sides it must show farther (1 and 2)
        let mut blocked = vec![false; n];
        let mut across = vec![false; n];
        let mut needs: Vec<Vec<Scratch>> = vec![Vec::new(); n];
        for i in 0..n {
            let (Some(&(l, _)), Some(b)) = (recs.get(i), scratch_of(i)) else {
                continue;
            };
            if !member(i) {
                continue;
            }
            let own = set.get(i).copied().unwrap_or(i);
            let list = near.get(i).map_or(&[][..], Vec::as_slice);
            let mut stop = !readable(l);
            let mut reads = false;
            let mut want: Vec<Scratch> = Vec::new();
            for &j in list {
                if !member(j) {
                    // a record that is not a member: never excluded (one excluded by t
                    // among them)
                    stop = true;
                } else if recs.get(j).is_some_and(|&(m, _)| m != l) {
                    reads = true;
                }
            }
            if let Some(own_sides) = sides.get(l) {
                let kinds = net.side_kinds(&line.solve_domain, own_sides, b);
                for (s, &kind) in kinds.iter().enumerate() {
                    let side = side_of(b, s);
                    let on_border = own_sides.on_side(b, s);
                    // (a side that is no boundary and lies on the leaf's border is covered by
                    // the leaves across it)
                    let across_read = || own_sides.leaves_across(b, s).all(readable);
                    match kind {
                        // (a seam keeps a boundary's test: its own enclosure, the first of
                        // a crease's two ways, so the drop accepts no more than across a
                        // crease; what lies across it is another leaf's end, which the
                        // drop's sets do not reach)
                        SideKind::Boundary => want.push(side),
                        // (nor does a break: the box excluded across it holds the other
                        // piece's edge, apart from this side)
                        SideKind::Crease
                            if net.on_a_seam(&line.solve_domain, b, s) || net.breaks_at(b, s) =>
                        {
                            want.push(side);
                        }
                        SideKind::Crease => {
                            let alone = !list.iter().any(|&j| {
                                member(j) && scratch_of(j).is_some_and(|o| boxes_meet(o, side))
                            });
                            if alone && (!on_border || across_read()) {
                                reads |= on_border;
                            } else {
                                want.push(side);
                            }
                        }
                        SideKind::Plain => {
                            if on_border {
                                if across_read() {
                                    reads = true;
                                } else {
                                    stop = true;
                                }
                            }
                        }
                    }
                }
            } else {
                stop = true;
            }
            if let Some(flag) = blocked.get_mut(own) {
                *flag |= stop;
            }
            if let Some(flag) = across.get_mut(own) {
                *flag |= reads;
            }
            if let Some(slot) = needs.get_mut(own) {
                slot.extend(want);
            }
        }
        // the sides of each set not kept by the above, shown farther by their own
        // enclosures, the sets in the order of their least box
        for i in 0..n {
            if !member(i) || set.get(i) != Some(&i) || blocked.get(i) != Some(&false) {
                continue;
            }
            let mut shown = true;
            for side in needs.get(i).map_or(&[][..], Vec::as_slice) {
                if done.enclosures >= budget {
                    done.out_of_budget = true;
                    shown = false;
                    break;
                }
                if done.enclosures % 64 == 0 {
                    if let Some(token) = cancel {
                        token.check(OP)?;
                    }
                }
                done.enclosures += 1;
                let farther = enclosed_value(net, &scratch, side.u, side.v)
                    .is_some_and(|r| scratch.farther(&r));
                if !farther {
                    shown = false;
                    break;
                }
            }
            if let Some(flag) = blocked.get_mut(i) {
                *flag = !shown;
            }
        }
        let gone: Vec<bool> = (0..n)
            .map(|i| {
                member(i)
                    && set
                        .get(i)
                        .is_some_and(|&own| blocked.get(own) == Some(&false))
            })
            .collect();
        for (i, &g) in gone.iter().enumerate() {
            if !g {
                continue;
            }
            match recs.get(i).map(|(_, r)| r.kind) {
                Some(FloorKind::Edge) => done.edge += 1,
                _ => done.no_root += 1,
            }
            if set
                .get(i)
                .is_some_and(|&own| across.get(own) == Some(&true))
            {
                done.across += 1;
            }
        }
        gone
    };
    let mut gone = gone.into_iter();
    for leaf in leaves.iter_mut() {
        if let Some(floor) = &mut leaf.floor {
            floor.retain(|_| !gone.next().unwrap_or(false));
        }
    }
    Ok(done)
}

/// What a box of a component is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Part {
    /// A box a leaf's pass kept.
    Floor(FloorKind),
    /// The whole domain of a leaf whose pass did not run: nothing is known of it.
    NotRunLeaf,
}

/// A box of a solve's components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlacedBox {
    /// The leaf it lies in (an index of the solve's searched leaves).
    pub leaf: usize,
    /// The box in u.
    pub u: Ivl,
    /// The box in v.
    pub v: Ivl,
    /// Its line parameters; the whole line where they were not enclosed.
    pub t: Ivl,
    /// What it is.
    pub part: Part,
}

/// How many boxes of each kind a component holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Holds {
    /// Boxes left at the floor.
    pub left: usize,
    /// Edge boxes.
    pub edge: usize,
    /// Plain boxes with no root the exclusion phase kept.
    pub no_root: usize,
    /// Boxes holding a certified root, reported.
    pub root: usize,
    /// Boxes holding a certified root not reported.
    pub unshown: usize,
    /// Boxes a pass stopped at its limit before visiting.
    pub unvisited: usize,
    /// Leaves whose pass did not run.
    pub not_run: usize,
    /// Boxes of a culled strip that step 1 did not exclude.
    pub culled: usize,
}

/// What a component lies in or touches that no pass decided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Touches {
    /// A leaf whose pass stopped at its limit.
    pub unfinished: bool,
    /// A leaf whose pass did not run.
    pub not_run: bool,
    /// A stretch of a leaf's border that no searched leaf or strip covers (a leaf outside the
    /// face: [`LeafSides`]).
    pub unsearched: bool,
}

/// A connected part of what the passes of a solve did not prove farther than the
/// tolerance (module docs, "Components").
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // independent facts of one component, each read alone
pub(crate) struct Component {
    /// Its boxes, indices of [`Components::boxes`], ascending.
    pub boxes: Vec<usize>,
    /// The hull of its boxes in u and in v, in the net's parameters.
    pub u: Ivl,
    /// The hull in v.
    pub v: Ivl,
    /// Its hull in u and in v as the typing reads it: [`Component::u`] and
    /// [`Component::v`], or, in a direction it is joined across a seam in, the two pieces
    /// glued there (module docs, "The seam").
    pub hull: [Span; 2],
    /// Whether it reaches a boundary side (answer10, Q2; set by [`type_components`] for
    /// every component, decided or not, false until then): a box of it has a side on the
    /// surface's or the solve's border where the solve does not wrap, or faces a stretch no
    /// searched leaf or strip covers (a trim cull), or a root's enclosure reaches that
    /// border; it lies in a leaf the trim crosses (not wholly inside the face); it touches a
    /// stretch nothing searched; or its hull is not shown covered by the searched leaves and
    /// strips (a trim cull inside it, or past the cover test's bound). An undecided
    /// component that reaches one may straddle the face's edge, which one face's solve
    /// cannot type (a consumer fires another ray); one that does not reaches no domain
    /// border or trim cull through its boxes or hull, and its cause is in its type (a gap of
    /// the typing: a seam not proved `C¹`, a foot box grown to a border, a chart or a
    /// budget).
    pub at_boundary: bool,
    /// Whether its boxes cover the whole knots' range in a direction the solve wraps in
    /// (joined across the seam or not): no lift of it is a rectangle less than a turn wide,
    /// so its sides and foot are not read ([`Why::WholeTurn`]; its count is).
    pub whole_turn: bool,
    /// The hull of its boxes' line parameters (each the feet widened by the tolerance).
    pub t: Ivl,
    /// Whether that hull reaches an end of the line's own interval.
    pub reaches_end: bool,
    /// The leaves it lies in or touches whose passes did not decide them.
    pub touches: Touches,
    /// Its boxes by kind.
    pub holds: Holds,
    /// The certified roots in it.
    pub roots: Vec<CertifiedRoot>,
    /// Its type ([`type_components`]): undecided until typed.
    pub contact: Contact,
    /// How its type was decided.
    pub typed: Typed,
    /// Why the foot rule along a frame ([`by_frame`]), tried after the closest-point foot
    /// rule failed at a stretch at the floor, did not type it either; `None` where it was
    /// not tried or typed it. A diagnosis only, beside [`Component::typed`], which keeps the
    /// closest-point rule's reason.
    pub frame_why: Option<Why>,
    /// Its distinct certified roots ([`root_classes`]), each the unique boxes of the root
    /// boxes that hold it; empty until typed.
    pub classes: Vec<Vec<[Ivl; 2]>>,
    /// Whether every leaf holding a box of it lies wholly inside the face.
    pub inside: bool,
    /// Its distinct certified roots whose line parameters lie wholly past the line's own
    /// interval (a component reaching its end), as [`Component::classes`]: not counted, and
    /// a point in one's unique box is undecided.
    pub beyond: Vec<Vec<[Ivl; 2]>>,
}

/// A component's hull in one direction (module docs, "The seam"): one interval of the
/// net's parameter, or, for a component joined across a seam in that direction, the piece
/// `first` at the knots' range's high end glued there to the piece `then` at its low end,
/// each in its own coordinates. The lifted interval runs from `first.lo` across the seam to
/// `then.hi`; it is never computed as one interval (a shift by the range's width rounds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Span {
    /// The interval, or the piece at the high end.
    pub first: Ivl,
    /// The piece at the low end, glued to `first` at the seam.
    pub then: Option<Ivl>,
}

impl Span {
    /// One interval.
    fn native(x: Ivl) -> Self {
        Self {
            first: x,
            then: None,
        }
    }

    /// Its pieces, in the order of the lifted interval.
    fn pieces(&self) -> impl Iterator<Item = Ivl> + '_ {
        core::iter::once(self.first).chain(self.then)
    }

    /// The parameter at the lifted interval's low end.
    fn low(&self) -> f64 {
        self.first.lo
    }

    /// The parameter at its high end.
    fn high(&self) -> f64 {
        self.then.map_or(self.first.hi, |t| t.hi)
    }

    /// A parameter about the middle of the lifted interval (a choice, rounded: for two
    /// pieces, half their lengths' sum along from `first.lo`, in whichever piece it falls).
    fn inner(&self) -> f64 {
        let Some(then) = self.then else {
            return self.first.mid();
        };
        let (a, b) = (self.first.hi - self.first.lo, then.hi - then.lo);
        let half = 0.5 * (a + b);
        if half <= a {
            (self.first.lo + half).min(self.first.hi)
        } else {
            (then.lo + (half - a)).min(then.hi)
        }
    }
}

/// The native rectangles of a component's hull ([`Component::hull`]): the pieces in u by
/// the pieces in v (one, two or four).
fn hull_pieces(hull: &[Span; 2]) -> Vec<[Ivl; 2]> {
    let [hu, hv] = hull;
    hu.pieces()
        .flat_map(|u| hv.pieces().map(move |v| [u, v]))
        .collect()
}

/// The components of one solve (module docs, "Components").
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Components {
    /// Every box of every component, sorted.
    pub boxes: Vec<PlacedBox>,
    /// The components, by their first box.
    pub list: Vec<Component>,
    /// Whether the solve's boxes or the tests joining them passed their bounds: nothing is
    /// then known of the components, and the whole line is undecided.
    pub over_budget: bool,
}

/// The most boxes a solve's drop reads and its components hold (sixteen leaves' worth at a
/// leaf's bounds); past it nothing is dropped and the components are not built
/// ([`Components::over_budget`]).
pub(crate) const MAX_SOLVE_BOXES: usize = 1 << 16;

impl Components {
    /// Components not built: past the bounds.
    pub(crate) fn over_budget() -> Self {
        Self {
            over_budget: true,
            ..Self::default()
        }
    }
}

/// The components of a solve over the whole surface: the kept boxes of every searched leaf
/// and culled strip (`leaves`, with `sides` what lies across each), and the whole domain of
/// each leaf whose pass did not run, joined where their closed `(u, v)` boxes meet and their
/// line parameters overlap, and across each seam the solve wraps (`seams`, module docs,
/// "The seam") where a box on one end of the knots' range and one on the other meet along
/// the seam and their line parameters overlap. Boxes excluded by t are not in them. `span`
/// is the line's own interval. Deterministic: the boxes are sorted, and a component is named
/// by its first. Not built ([`Components::over_budget`]) where a leaf's records were not
/// kept, or past the bounds on boxes and on the pairs tested.
pub(crate) fn components(
    leaves: &[SearchedLeaf],
    sides: &[LeafSides],
    span: [f64; 2],
    seams: Seams,
) -> Components {
    let mut out = Components::default();
    for (l, leaf) in leaves.iter().enumerate() {
        if leaf.stop == Stop::NotRun {
            let [[u0, u1], [v0, v1]] = bounds_of(&leaf.domain);
            out.boxes.push(PlacedBox {
                leaf: l,
                u: Ivl::new(u0, u1),
                v: Ivl::new(v0, v1),
                t: Ivl::whole(),
                part: Part::NotRunLeaf,
            });
        }
        let Some(floor) = &leaf.floor else {
            // (records not kept: nothing is known of the components)
            if leaf.stop == Stop::NotRun {
                continue;
            }
            return Components::over_budget();
        };
        for r in floor {
            if r.kind != FloorKind::ExcludedByT {
                out.boxes.push(PlacedBox {
                    leaf: l,
                    u: r.u,
                    v: r.v,
                    t: r.t.unwrap_or_else(Ivl::whole),
                    part: Part::Floor(r.kind),
                });
            }
        }
        if out.boxes.len() > MAX_SOLVE_BOXES {
            return Components::over_budget();
        }
    }
    out.boxes.sort_by(|a, b| {
        a.u.lo
            .total_cmp(&b.u.lo)
            .then(a.v.lo.total_cmp(&b.v.lo))
            .then(a.u.hi.total_cmp(&b.u.hi))
            .then(a.v.hi.total_cmp(&b.v.hi))
            .then(a.t.lo.total_cmp(&b.t.lo))
            .then(a.t.hi.total_cmp(&b.t.hi))
            .then(a.leaf.cmp(&b.leaf))
    });
    let n = out.boxes.len();
    let rects: Vec<[Ivl; 2]> = out.boxes.iter().map(|b| [b.u, b.v]).collect();
    let mut parent: Vec<usize> = (0..n).collect();
    let boxes = &out.boxes;
    let within = meeting_pairs(&rects, |i, j| {
        let overlap = match (boxes.get(i), boxes.get(j)) {
            (Some(a), Some(b)) => a.t.lo <= b.t.hi && b.t.lo <= a.t.hi,
            _ => false,
        };
        if overlap {
            join(&mut parent, i, j);
        }
    });
    if !within {
        return Components::over_budget();
    }
    // across each seam (module docs, "The seam"): the net closes exactly there, so a box on
    // one end and one on the other whose stretches along the seam meet share a point of the
    // net (at a torus's corner, across the other seam too), whose foot both boxes' line
    // parameters hold where they overlap; each such box is marked joined in that direction
    // (and in the other, met across its seam)
    let mut glued: [Vec<bool>; 2] = [vec![false; n], vec![false; n]];
    let ranges = seams.range;
    let along = |b: &PlacedBox, d: usize| if d == 0 { b.u } else { b.v };
    let mut tests = 0_usize;
    for d in 0..2 {
        let (Some(&true), Some(&[e0, e1])) = (seams.wrap.get(d), ranges.get(d)) else {
            continue;
        };
        let other = 1 - d;
        let wrap_other = seams.wrap.get(other).copied().unwrap_or(false);
        let [f0, f1] = ranges.get(other).copied().unwrap_or([f64::NAN; 2]);
        let low: Vec<usize> = (0..n)
            .filter(|&i| boxes.get(i).is_some_and(|b| along(b, d).lo <= e0))
            .collect();
        let high: Vec<usize> = (0..n)
            .filter(|&i| boxes.get(i).is_some_and(|b| along(b, d).hi >= e1))
            .collect();
        for &i in &low {
            for &j in &high {
                tests = tests.saturating_add(1);
                if tests > MAX_MEETING_TESTS {
                    return Components::over_budget();
                }
                let (Some(a), Some(b)) = (boxes.get(i), boxes.get(j)) else {
                    continue;
                };
                let (x, y) = (along(a, other), along(b, other));
                let direct = x.lo <= y.hi && y.lo <= x.hi;
                let corner =
                    wrap_other && ((x.lo <= f0 && y.hi >= f1) || (y.lo <= f0 && x.hi >= f1));
                let overlap = a.t.lo <= b.t.hi && b.t.lo <= a.t.hi;
                if !((direct || corner) && overlap) {
                    continue;
                }
                join(&mut parent, i, j);
                let mut mark = |k: usize| {
                    for x in [i, j] {
                        if let Some(slot) = glued.get_mut(k).and_then(|g| g.get_mut(x)) {
                            *slot = true;
                        }
                    }
                };
                mark(d);
                if !direct {
                    mark(other);
                }
            }
        }
    }
    let [s0, s1] = span;
    let mut named: Vec<Option<usize>> = vec![None; n];
    let mut of_box: Vec<usize> = Vec::new();
    // what each component's boxes touch across a seam ([`facing`])
    let mut seam_touches: Vec<Touches> = Vec::new();
    for i in 0..n {
        let Some(&b) = out.boxes.get(i) else {
            continue;
        };
        let first = root_of(&mut parent, i);
        let c = if let Some(c) = named.get(first).copied().flatten() {
            c
        } else {
            let c = out.list.len();
            if let Some(slot) = named.get_mut(first) {
                *slot = Some(c);
            }
            out.list.push(Component {
                boxes: Vec::new(),
                u: b.u,
                v: b.v,
                hull: [Span::native(b.u), Span::native(b.v)],
                at_boundary: false,
                whole_turn: false,
                t: b.t,
                reaches_end: false,
                touches: Touches::default(),
                holds: Holds::default(),
                roots: Vec::new(),
                contact: Contact::Undecided,
                typed: Typed::Untyped,
                frame_why: None,
                classes: Vec::new(),
                inside: true,
                beyond: Vec::new(),
            });
            seam_touches.push(Touches::default());
            c
        };
        of_box.push(c);
        let (flags, across) = facing(&b, leaves, sides);
        if let Some(t) = seam_touches.get_mut(c) {
            t.unfinished |= across.unfinished;
            t.not_run |= across.not_run;
            t.unsearched |= across.unsearched;
        }
        let Some(comp) = out.list.get_mut(c) else {
            continue;
        };
        comp.boxes.push(i);
        comp.u = comp.u.hull(b.u);
        comp.v = comp.v.hull(b.v);
        comp.t = comp.t.hull(b.t);
        comp.reaches_end = comp.t.lo <= s0 || comp.t.hi >= s1;
        comp.touches.unfinished |= flags.unfinished;
        comp.touches.not_run |= flags.not_run;
        comp.touches.unsearched |= flags.unsearched;
        comp.inside &= leaves.get(b.leaf).is_some_and(|l| l.inside);
        let h = &mut comp.holds;
        let count = match b.part {
            Part::NotRunLeaf => Some(&mut h.not_run),
            Part::Floor(FloorKind::Left) => Some(&mut h.left),
            Part::Floor(FloorKind::Edge) => Some(&mut h.edge),
            Part::Floor(FloorKind::NoRoot) => Some(&mut h.no_root),
            Part::Floor(FloorKind::Root { .. }) => Some(&mut h.root),
            Part::Floor(FloorKind::Unshown { .. }) => Some(&mut h.unshown),
            Part::Floor(FloorKind::Unvisited) => Some(&mut h.unvisited),
            Part::Floor(FloorKind::Culled) => Some(&mut h.culled),
            // (never placed)
            Part::Floor(FloorKind::ExcludedByT) => None,
        };
        if let Some(count) = count {
            *count += 1;
        }
    }
    // each component's hull: in a direction the solve wraps in, its boxes' stretches
    // merged; one covering the whole range is a whole turn, whether or not it is joined
    // across the seam. Joined across it, a connected component's stretches are a connected
    // arc of the turn (boxes joined meet in that direction, or across the seam), so two
    // pieces, one on each end of the range, glued at the seam (more than two cannot occur,
    // and are read as a whole turn); it then reads what its boxes touch across the seam
    for (ci, comp) in out.list.iter_mut().enumerate() {
        comp.hull = [Span::native(comp.u), Span::native(comp.v)];
        for (d, hull) in comp.hull.iter_mut().enumerate() {
            let (Some(&true), Some(&[e0, e1])) = (seams.wrap.get(d), ranges.get(d)) else {
                continue;
            };
            let joined = comp.boxes.iter().any(|&i| {
                glued
                    .get(d)
                    .and_then(|g| g.get(i))
                    .copied()
                    .unwrap_or(false)
            });
            let mut runs: Vec<Ivl> = comp
                .boxes
                .iter()
                .filter_map(|&i| out.boxes.get(i).map(|b| along(b, d)))
                .collect();
            runs.sort_by(|a, b| a.lo.total_cmp(&b.lo).then(a.hi.total_cmp(&b.hi)));
            let mut merged: Vec<Ivl> = Vec::new();
            for r in runs {
                match merged.last_mut() {
                    Some(m) if r.lo <= m.hi => m.hi = m.hi.max(r.hi),
                    _ => merged.push(r),
                }
            }
            match merged.as_slice() {
                [one] if one.lo <= e0 && one.hi >= e1 => comp.whole_turn = true,
                _ if !joined => {}
                [low, high] if low.lo <= e0 && high.hi >= e1 => {
                    *hull = Span {
                        first: *high,
                        then: Some(*low),
                    };
                }
                _ => comp.whole_turn = true,
            }
        }
        let lifted = comp.whole_turn || comp.hull.iter().any(|h| h.then.is_some());
        if let (true, Some(t)) = (lifted, seam_touches.get(ci)) {
            comp.touches.unfinished |= t.unfinished;
            comp.touches.not_run |= t.not_run;
            comp.touches.unsearched |= t.unsearched;
        }
    }
    // each certified root in the component of a box of its leaf holding it
    for (l, leaf) in leaves.iter().enumerate() {
        for root in &leaf.roots {
            let [u, v, t] = root.point;
            let holder = out.boxes.iter().position(|b| {
                b.leaf == l
                    && matches!(b.part, Part::Floor(FloorKind::Root { .. }))
                    && b.u.lo <= u
                    && u <= b.u.hi
                    && b.v.lo <= v
                    && v <= b.v.hi
                    && b.t.lo <= t
                    && t <= b.t.hi
            });
            let comp = holder
                .and_then(|i| of_box.get(i).copied())
                .and_then(|c| out.list.get_mut(c));
            if let Some(comp) = comp {
                comp.roots.push(*root);
            }
        }
    }
    out
}

/// What the box `b` of a component touches across its leaf's border: a leaf whose pass
/// stopped at its limit (or `b` lies in one), a leaf whose pass did not run (or `b` is
/// one), and a stretch that no searched leaf or strip covers; apart, what it touches across
/// a seam the solve wraps, read only for a component joined across one (or covering a whole
/// turn). One that is not joined has no box across the seam where its own boxes meet it
/// with overlapping line parameters, so it reaches no further than its seam sides: a side
/// facing a trim cull across the seam is a boundary in [`LineNet::side_kinds`] (the count
/// rule reads it as one), and what lies across is not read as a flag, which leaves every
/// component that does not reach across a seam with the flags it had before the seam
/// wrapped.
fn facing(b: &PlacedBox, leaves: &[SearchedLeaf], sides: &[LeafSides]) -> (Touches, Touches) {
    let own = leaves.get(b.leaf).map(|l| l.stop);
    let mut touches = Touches {
        unfinished: own == Some(Stop::AtLimit),
        not_run: b.part == Part::NotRunLeaf,
        unsearched: false,
    };
    let mut seam = Touches::default();
    let Some(leaf_sides) = sides.get(b.leaf) else {
        return (touches, seam);
    };
    let s = Scratch { u: b.u, v: b.v };
    for (side, &open) in leaf_sides.faces_unsearched(s).iter().enumerate() {
        let on_seam = leaf_sides.seam.get(side).copied().unwrap_or(false);
        let flags = if on_seam { &mut seam } else { &mut touches };
        flags.unsearched |= open;
    }
    for side in 0..4 {
        let on_seam = leaf_sides.seam.get(side).copied().unwrap_or(false);
        for j in leaf_sides.leaves_across(s, side) {
            let flags = if on_seam { &mut seam } else { &mut touches };
            match leaves.get(j).map(|l| l.stop) {
                Some(Stop::AtLimit) => flags.unfinished = true,
                Some(Stop::NotRun) => flags.not_run = true,
                _ => {}
            }
        }
    }
    (touches, seam)
}

/// The side record of a solve (module docs, "Components"): not run when the solve has a
/// leaf the traversal solved and no such leaf's pass ran, whatever its culled strips found
/// (the solutions are then the guesses' alone, and a strip's boxes undecided around a leaf
/// not run narrow nothing), or when it has none and no strip ran; otherwise undecided over
/// the line parameters of every component that is undecided, or decided but not stood for
/// by the solutions (`stood_for`, by the component's index; none where it is short), and
/// over the whole of the line's own interval `span` where a cull's strip could not be
/// placed (`unplaced`: nothing has enclosed it) or the components passed their bounds;
/// complete only with none of these. A strip's boxes it left undecided are in the
/// components, over their own line parameters. A leaf whose pass ran beside leaves not run
/// leaves the record undecided: its pass may have added certified roots, so the solutions
/// are no longer the guesses' alone.
pub(crate) fn side_record(
    leaves: &[SearchedLeaf],
    components: &Components,
    stood_for: &[bool],
    unplaced: bool,
    span: [f64; 2],
) -> SideRecord {
    if leaves_not_run(leaves) {
        return SideRecord::NotRun;
    }
    let [s0, s1] = span;
    let mut spans: Vec<[f64; 2]> = Vec::new();
    if unplaced || components.over_budget {
        spans.push(span);
    }
    for (i, c) in components.list.iter().enumerate() {
        // (a decided component the solutions do not stand for is not decided for a ray:
        // a touching one too, whose crossings may be the solutions' to carry)
        let decided = matches!(c.contact, Contact::Touching | Contact::Crossing)
            && stood_for.get(i).copied().unwrap_or(false);
        if decided {
            continue;
        }
        let (lo, hi) = (cmax(c.t.lo, s0), cmin(c.t.hi, s1));
        if c.t.lo.is_nan() || c.t.hi.is_nan() {
            // (nothing is known of where it lies)
            spans.push(span);
        } else if lo <= hi {
            spans.push([lo, hi]);
        }
    }
    if spans.is_empty() {
        SideRecord::Complete
    } else {
        SideRecord::undecided(spans)
    }
}

/// Whether the side record of a solve over `leaves` is not run ([`side_record`]): the
/// passes of the leaves the traversal solved (not the culled strips) all did not run, or,
/// with no such leaf (every leaf culled), there are strips and none of them ran. (`all` over
/// no leaves is true, hence the second case's own test.)
pub(crate) fn leaves_not_run(leaves: &[SearchedLeaf]) -> bool {
    let mut solved = leaves.iter().filter(|l| !l.strip).peekable();
    if solved.peek().is_some() {
        solved.all(|l| l.stop == Stop::NotRun)
    } else {
        !leaves.is_empty() && leaves.iter().all(|l| l.stop == Stop::NotRun)
    }
}

/// The complete surface of revolution a net is a patch of (module docs, "Typing"): a
/// sphere, a cylinder or one nappe of a cone, or a ring torus, about the axis of its
/// placement, as the analytic surface the net was made from defines it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Revolved {
    /// A point on the axis (the placement's origin).
    origin: [f64; 3],
    /// The placement's x and y axes (the plane the turn is measured in) and its axis.
    x: [f64; 3],
    y: [f64; 3],
    z: [f64; 3],
    /// The profile.
    shape: Shape,
    /// Whether the net's u is the turn about the axis (else its v).
    turn_in_u: bool,
}

/// How far each entry of the Gram matrix of a [`Revolved`]'s stored axes may be from the
/// identity's ([`Revolved::of`]).
const FRAME_DEFECT: f64 = 1.0e-12;

/// A lower bound of every eigenvalue of that Gram matrix, `1 − 3 FRAME_DEFECT`
/// (Gershgorin's discs), below by a margin for its rounding.
const GRAM_FLOOR: f64 = 1.0 - 4.0 * FRAME_DEFECT;

/// The profile of a [`Revolved`] in its half-plane through the axis (radius from the axis,
/// height along it).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    /// A circle about the origin.
    Sphere { radius: f64 },
    /// A line: the radius at height 0 and its change per unit height (0: a cylinder).
    Cone { radius: f64, slope: f64 },
    /// A circle of radius `minor` about the point at radius `major` and height 0, with
    /// `major > minor` (a ring torus: no point of it on the axis).
    Torus { major: f64, minor: f64 },
}

impl Revolved {
    /// The surface of revolution of `surface` when it is a sphere, a cone or cylinder, or a
    /// ring torus whose net turns about the axis in a direction of degree 2 (each of its
    /// knot spans a rational quadratic arc, less than half a turn) and, for the sphere and
    /// the torus, whose profile circle is of degree 2 too; `None` otherwise.
    fn of(surface: &Surface, net: &Nurbs) -> Option<Self> {
        use crate::surface::surface::SurfaceKind;
        let (rev, shape) = match surface.kind() {
            SurfaceKind::Sphere(s) => (&s.base, Shape::Sphere { radius: s.radius }),
            SurfaceKind::Cone(c) => (
                &c.rev,
                Shape::Cone {
                    radius: c.base_radius,
                    slope: if c.cylinder {
                        0.0
                    } else {
                        det::tan(c.semi_angle_deg.to_radians())
                    },
                },
            ),
            SurfaceKind::Torus(t) if t.major_radius > t.minor_radius => (
                &t.base,
                Shape::Torus {
                    major: t.major_radius,
                    minor: t.minor_radius,
                },
            ),
            _ => return None,
        };
        let finite = match shape {
            Shape::Sphere { radius } => radius > 0.0 && radius.is_finite(),
            Shape::Cone { radius, slope } => radius.is_finite() && slope.is_finite(),
            Shape::Torus { major, minor } => minor > 0.0 && major.is_finite(),
        };
        let turn_in_u = !rev.swap_uv;
        let (turn_degree, profile_degree) = if turn_in_u {
            (net.p, net.q)
        } else {
            (net.q, net.p)
        };
        let torus = matches!(shape, Shape::Torus { .. });
        let circle = torus || matches!(shape, Shape::Sphere { .. });
        if !finite || turn_degree != 2 || (circle && profile_degree != 2) {
            return None;
        }
        let place = &rev.position;
        let frame = [f3(place.x_axis()), f3(place.y_axis()), f3(place.z_axis())];
        // the stored axes orthonormal to within `FRAME_DEFECT` in every entry of their Gram
        // matrix, enclosed: the frame is then nonsingular, with every eigenvalue of the Gram
        // matrix at least `GRAM_FLOOR` (Gershgorin)
        let orthonormal = frame.iter().enumerate().all(|(i, a)| {
            frame.iter().enumerate().all(|(j, b)| {
                let g = dot(*a, &b.map(Ivl::from)) - Ivl::from(if i == j { 1.0 } else { 0.0 });
                g.mag() <= FRAME_DEFECT
            })
        });
        if !orthonormal {
            return None;
        }
        Some(Self {
            origin: f3(place.origin()),
            x: f3(place.x_axis()),
            y: f3(place.y_axis()),
            z: f3(place.z_axis()),
            shape,
            turn_in_u,
        })
    }

    /// The surface as [`side_bounds`] reads it: the same side function,
    /// from the same stored origin and axes.
    fn revolution(&self) -> Revolution {
        let origin = v3(self.origin);
        let frame = [v3(self.x), v3(self.y), v3(self.z)];
        match self.shape {
            Shape::Sphere { radius } => Revolution::Sphere {
                centre: origin,
                radius,
            },
            Shape::Cone { radius, slope } => Revolution::Cone {
                origin,
                frame,
                radius,
                slope,
            },
            Shape::Torus { major, minor } => Revolution::Torus {
                origin,
                frame,
                major,
                minor,
            },
        }
    }

    /// The point `w + origin` in the placement: its two coordinates in the plane of the
    /// turn, and its height along the axis, enclosed.
    fn coordinates(&self, w: &I3) -> ([Ivl; 2], Ivl) {
        ([dot(self.x, w), dot(self.y, w)], dot(self.z, w))
    }

    /// The side function at the points `w + origin`, enclosed: continuous on all of space,
    /// zero exactly on the complete surface (the whole sphere, the infinite cylinder or the
    /// infinite nappe of the cone, the whole torus), of one sign on each side of it, and in
    /// magnitude the distance to the surface (sphere, cylinder, torus) or to the line through
    /// the cone's profile in the half-plane through the axis (no more than the distance to
    /// the nappe).
    fn side(&self, w: &I3) -> Ivl {
        let ([a, b], h) = self.coordinates(w);
        let rho = || radius_of(a, b);
        match self.shape {
            Shape::Sphere { radius } => length_squared(w).sqrt() - Ivl::from(radius),
            // (the distance in the half-plane to the line through the profile, signed by
            // the radius that line has at height h, which no point reaches past the apex:
            // the zero set is one nappe)
            Shape::Cone { radius, slope } => {
                let s = Ivl::from(slope);
                (rho() - Ivl::from(radius) - s * h) / (Ivl::from(1.0) + s * s).sqrt()
            }
            Shape::Torus { major, minor } => {
                let across = rho() - Ivl::from(major);
                length_squared(&[across, h, Ivl::from(0.0)]).sqrt() - Ivl::from(minor)
            }
        }
    }

    /// The point in the profile's half-plane a circle profile's angle is read from: the
    /// radius past the major radius (the torus's; 0 for the sphere), and the height.
    fn in_the_tube(&self, w: &I3) -> [Ivl; 2] {
        let ([a, b], h) = self.coordinates(w);
        let rho = radius_of(a, b);
        match self.shape {
            Shape::Torus { major, .. } => [rho - Ivl::from(major), h],
            // (no subtraction of 0: its outward rounding would reach below the axis)
            _ => [rho, h],
        }
    }
}

/// The length of the plane vector `(a, b)`, enclosed, never below 0 (the square root's
/// outward rounding of 0 is not a length).
fn radius_of(a: Ivl, b: Ivl) -> Ivl {
    let r = length_squared(&[a, b, Ivl::from(0.0)]).sqrt();
    Ivl::new(r.lo.max(0.0), r.hi)
}

/// `a × b` of two plane vectors, enclosed.
fn cross2(a: [Ivl; 2], b: [Ivl; 2]) -> Ivl {
    let [a0, a1] = a;
    let [b0, b1] = b;
    a0 * b1 - a1 * b0
}

/// One piece of an edge of a component's `(u, v)` hull (one of its parameters a single
/// number), with the net over it less the axis's origin enclosed: over the piece (`w`), at
/// its middle (`mid`), and its partial along the parameter that runs (`dw`), with half that
/// parameter's width (`r`), for the centred form of the tests ([`centred`]).
#[derive(Debug, Clone, Copy)]
struct EdgePiece {
    u: Ivl,
    v: Ivl,
    w: I3,
    mid: I3,
    dw: I3,
    r: f64,
}

/// A test of an edge piece: from the net's point `X` (less the origin, enclosed) and its
/// partial `X'` along the edge, the tested quantity and its derivative along the edge, each
/// enclosed. The test passes where the quantity is proved positive.
type EdgeTest<'a> = &'a dyn Fn(&I3, &I3) -> (Ivl, Ivl);

/// The tested quantity over an edge piece, enclosed: the met of its value over the piece's
/// enclosure and its centred form, the value at the middle plus the derivative over the
/// piece times `[−r, r]` (the mean value theorem along the edge, on one polynomial piece of
/// the net, so the edge is smooth there). The centred form is what makes a quantity that
/// hardly changes along the edge (a direction about the axis along a meridian, the height
/// or the profile's direction along a circle of latitude) tight: the enclosure of the
/// points alone is as wide as the piece is long.
fn centred(p: &EdgePiece, g: EdgeTest<'_>) -> Ivl {
    let (at_mid, _) = g(&p.mid, &p.dw);
    let (naive, slope) = g(&p.w, &p.dw);
    let c = at_mid + slope * Ivl::new(-p.r, p.r);
    // (both hold the quantity; apart, a rounding has gone wrong, and nothing is shown)
    meet(naive, c).unwrap_or_else(|| Ivl::new(f64::NAN, f64::NAN))
}

/// The coverage certificate of a component's `(u, v)` hull `H` on a surface of revolution
/// (module docs, "Typing"): the net over `H`'s four edges in pieces (never hulled), in the
/// order turn low, turn high, profile low, profile high; a plane direction `d` every edge
/// piece's direction about the axis lies within a quarter turn of (and, for the sphere and
/// the torus, a direction `d_tube` every edge piece's direction about the profile circle's
/// centre does); and the orientation of each coordinate along the hull, read from the net's
/// points (a choice of which test to run, never assumed: a wrong one only fails the tests).
#[derive(Debug, Clone)]
struct Chart {
    edges: [Vec<EdgePiece>; 4],
    d: [f64; 2],
    d_tube: Option<[f64; 2]>,
    /// Whether the directions about the axis turn counterclockwise from the turn-low edge
    /// to the turn-high one.
    turn_ccw: bool,
    /// Whether the profile coordinate rises from the profile-low edge to the profile-high
    /// one (the height for the cone, the direction about the centre counterclockwise for the
    /// sphere and the torus).
    profile_up: bool,
}

/// `d · v` of a plane direction and a plane vector of intervals.
fn dot2(d: [f64; 2], v: [Ivl; 2]) -> Ivl {
    let [a, b] = v;
    Ivl::from(d[0]) * a + Ivl::from(d[1]) * b
}

/// The derivative of the point's vector in the profile's half-plane ([`Revolved::in_the_tube`])
/// along the edge, from the point `x` and its partial `dx`: `(ρ', h')` with
/// `ρ' = (a a' + b b') / ρ`; the whole line where `ρ` is not proved positive.
fn tube_slope(rev: &Revolved, x: &I3, dx: &I3) -> [Ivl; 2] {
    let ([a, b], _) = rev.coordinates(x);
    let ([da, db], dh) = rev.coordinates(dx);
    let rho = radius_of(a, b);
    let drho = if rho.lo > 0.0 {
        (a * da + b * db) / rho
    } else {
        Ivl::whole()
    };
    [drho, dh]
}

impl Chart {
    /// The certificate of the hull `[u, v]` (module docs, "Typing"; each a [`Span`], two
    /// pieces glued across a seam: the edges of the lifted rectangle, those along a glued
    /// direction made of both pieces' edges, module docs, "The seam"), within `budget`
    /// enclosures (spent from it); `None` where it is not shown: a hull reaching a pole or a
    /// whole turn (no half-plane holds its edges), an edge piece not enclosed, or the budget
    /// spent.
    fn of(net: &LineNet, rev: &Revolved, [u, v]: [Span; 2], budget: &mut usize) -> Option<Self> {
        let (turn, profile) = if rev.turn_in_u { (u, v) } else { (v, u) };
        let point = |t: f64, p: f64| -> Option<I3> {
            let (pu, pv) = if rev.turn_in_u { (t, p) } else { (p, t) };
            net.at(pu, pv, rev.origin)
        };
        let plane = |w: &I3| rev.coordinates(w).0;
        let tube = |w: &I3| rev.in_the_tube(w);
        let mid_of = |x: [Ivl; 2]| x.map(Ivl::mid);
        // the directions read from the net's points (choices, not assumptions)
        let centre = point(turn.inner(), profile.inner())?;
        let d = unit2(mid_of(plane(&centre)))?;
        let d_tube = match rev.shape {
            Shape::Cone { .. } => None,
            _ => Some(unit2(mid_of(tube(&centre)))?),
        };
        let (t_lo, t_hi) = (
            point(turn.low(), profile.inner())?,
            point(turn.high(), profile.inner())?,
        );
        let turn_ccw = cross2(plane(&t_lo), plane(&t_hi)).mid() > 0.0;
        let (p_lo, p_hi) = (
            point(turn.inner(), profile.low())?,
            point(turn.inner(), profile.high())?,
        );
        let profile_up = match d_tube {
            None => rev.coordinates(&p_hi).1.mid() > rev.coordinates(&p_lo).1.mid(),
            Some(_) => cross2(tube(&p_lo), tube(&p_hi)).mid() > 0.0,
        };
        // the four edges, each piece of the hull along them cut at the net's knots
        let edge = |fixed_turn: Option<f64>, fixed_profile: Option<f64>| -> Vec<(Ivl, Ivl)> {
            let (in_u, run) = if fixed_turn.is_some() {
                (!rev.turn_in_u, profile)
            } else {
                (rev.turn_in_u, turn)
            };
            run.pieces()
                .flat_map(|run| {
                    let mut at = vec![run.lo];
                    at.extend(knots_inside(net, in_u, [run.lo, run.hi]));
                    at.push(run.hi);
                    at.windows(2)
                        .filter_map(|w| match *w {
                            [a, z] => Some(Ivl::new(a, z)),
                            _ => None,
                        })
                        .collect::<Vec<Ivl>>()
                })
                .map(|piece| {
                    let (t, p) = match (fixed_turn, fixed_profile) {
                        (Some(t), _) => (Ivl::from(t), piece),
                        (_, Some(p)) => (piece, Ivl::from(p)),
                        _ => (piece, piece),
                    };
                    if rev.turn_in_u {
                        (t, p)
                    } else {
                        (p, t)
                    }
                })
                .collect()
        };
        let params = [
            edge(Some(turn.low()), None),
            edge(Some(turn.high()), None),
            edge(None, Some(profile.low())),
            edge(None, Some(profile.high())),
        ];
        let mut edges: [Vec<EdgePiece>; 4] = Default::default();
        for (slot, list) in edges.iter_mut().zip(params) {
            for (pu, pv) in list {
                slot.push(enclose_edge(net, rev, pu, pv, budget)?);
            }
        }
        let mut chart = Self {
            edges,
            d,
            d_tube,
            turn_ccw,
            profile_up,
        };
        // every edge piece within a quarter turn of `d` (and of `d_tube`)
        let in_d = |x: &I3, dx: &I3| (dot2(d, plane(x)), dot2(d, plane(dx)));
        for e in 0..4 {
            if !chart.clear(net, rev, e, budget, &in_d) {
                return None;
            }
            if let Some(dt) = d_tube {
                let in_dt = |x: &I3, dx: &I3| (dot2(dt, tube(x)), dot2(dt, tube_slope(rev, x, dx)));
                if !chart.clear(net, rev, e, budget, &in_dt) {
                    return None;
                }
            }
        }
        Some(chart)
    }

    /// Whether every piece of the edge `e` passes `test` ([`centred`]), halving the pieces
    /// that do not (each half enclosed, within `budget`); `false` where one cannot be made
    /// to, and at once where the net's point at a failing piece's middle fails it too (no
    /// halving of the edge passes that; a test against a piece of the line may still pass
    /// once the line's piece is halved, which the caller does).
    fn clear(
        &mut self,
        net: &LineNet,
        rev: &Revolved,
        e: usize,
        budget: &mut usize,
        test: EdgeTest<'_>,
    ) -> bool {
        let Some(list) = self.edges.get_mut(e) else {
            return false;
        };
        let mut done: Vec<EdgePiece> = Vec::new();
        let mut work: Vec<EdgePiece> = std::mem::take(list);
        work.reverse();
        let mut ok = true;
        while let Some(piece) = work.pop() {
            if !ok || centred(&piece, test).lo > 0.0 {
                done.push(piece);
                continue;
            }
            if !(test(&piece.mid, &piece.dw).0.lo > 0.0) {
                ok = false;
                done.push(piece);
                continue;
            }
            // halve along the parameter that runs
            let split_u = piece.u.lo < piece.u.hi;
            let run = if split_u { piece.u } else { piece.v };
            let mid = run.mid();
            let halves = (mid > run.lo && mid < run.hi).then(|| {
                let (a, b) = (Ivl::new(run.lo, mid), Ivl::new(mid, run.hi));
                if split_u {
                    [(a, piece.v), (b, piece.v)]
                } else {
                    [(piece.u, a), (piece.u, b)]
                }
            });
            let enclosed = halves.and_then(|[(u0, v0), (u1, v1)]| {
                Some([
                    enclose_edge(net, rev, u0, v0, budget)?,
                    enclose_edge(net, rev, u1, v1, budget)?,
                ])
            });
            if let Some([first, second]) = enclosed {
                work.push(second);
                work.push(first);
            } else {
                ok = false;
                done.push(piece);
            }
        }
        if let Some(list) = self.edges.get_mut(e) {
            *list = done;
        }
        ok
    }

    /// Whether every point of `w + origin` lies strictly inside the chart (module docs,
    /// "Typing"): within a quarter turn of `d`, its direction about the axis strictly past
    /// every turn-low edge piece's and before every turn-high one's, and its height
    /// (cone) or its direction about the profile circle's centre (within a quarter turn of
    /// `d_tube`) strictly past every profile-low edge piece's and before every profile-high
    /// one's, each in the chart's orientation. Halves edge pieces within `budget`.
    fn holds(&mut self, net: &LineNet, rev: &Revolved, w: &I3, budget: &mut usize) -> bool {
        let (pw, hw) = rev.coordinates(w);
        if !(dot2(self.d, pw).lo > 0.0) {
            return false;
        }
        let plane = |x: &I3| rev.coordinates(x).0;
        // (the turn: `cross2(edge, w) > 0` past the low edge, `cross2(w, edge) > 0` before
        // the high one, counterclockwise; the other way round clockwise)
        let sign = Ivl::from(if self.turn_ccw { 1.0 } else { -1.0 });
        let past_low =
            |x: &I3, dx: &I3| (sign * cross2(plane(x), pw), sign * cross2(plane(dx), pw));
        let before_high =
            |x: &I3, dx: &I3| (sign * cross2(pw, plane(x)), sign * cross2(pw, plane(dx)));
        if !self.clear(net, rev, 0, budget, &past_low)
            || !self.clear(net, rev, 1, budget, &before_high)
        {
            return false;
        }
        let up = Ivl::from(if self.profile_up { 1.0 } else { -1.0 });
        match self.d_tube {
            None => {
                let height = |x: &I3| rev.coordinates(x).1;
                let above_low =
                    |x: &I3, dx: &I3| (up * (hw - height(x)), Ivl::from(0.0) - up * height(dx));
                let below_high = |x: &I3, dx: &I3| (up * (height(x) - hw), up * height(dx));
                self.clear(net, rev, 2, budget, &above_low)
                    && self.clear(net, rev, 3, budget, &below_high)
            }
            Some(dt) => {
                let tw = rev.in_the_tube(w);
                if !(dot2(dt, tw).lo > 0.0) {
                    return false;
                }
                let tube = |x: &I3| rev.in_the_tube(x);
                let past_low = |x: &I3, dx: &I3| {
                    (
                        up * cross2(tube(x), tw),
                        up * cross2(tube_slope(rev, x, dx), tw),
                    )
                };
                let before_high = |x: &I3, dx: &I3| {
                    (
                        up * cross2(tw, tube(x)),
                        up * cross2(tw, tube_slope(rev, x, dx)),
                    )
                };
                self.clear(net, rev, 2, budget, &past_low)
                    && self.clear(net, rev, 3, budget, &before_high)
            }
        }
    }
}

/// The unit vector along `v`, `None` where it is not finite or of no length.
fn unit2(v: [f64; 2]) -> Option<[f64; 2]> {
    let [a, b] = v;
    let len = det::hypot(a, b);
    (len > 0.0 && len.is_finite()).then(|| [a / len, b / len])
}

/// The edge piece `[u, v]` (one of them a single number) enclosed ([`EdgePiece`]), one
/// enclosure spent from `budget`; `None` where it cannot be (or the budget is spent).
fn enclose_edge(
    net: &LineNet,
    rev: &Revolved,
    u: Ivl,
    v: Ivl,
    budget: &mut usize,
) -> Option<EdgePiece> {
    if *budget == 0 {
        return None;
    }
    *budget = budget.saturating_sub(1);
    let [w, s_u, s_v] = net.over(u, v, rev.origin)?;
    let mid = net.at(u.mid(), v.mid(), rev.origin)?;
    let in_u = u.lo < u.hi;
    let run = if in_u { u } else { v };
    let m = run.mid();
    let r = round_up(cmax(m - run.lo, run.hi - m));
    Some(EdgePiece {
        u,
        v,
        w,
        mid,
        dw: if in_u { s_u } else { s_v },
        r,
    })
}

/// The most piece enclosures one component's side test makes (module docs, "Typing"): a
/// leaf's own bound, as for the strips.
pub(crate) const MAX_SIDE_PIECES: usize = MAX_SCRATCH_BOXES;

/// How a component's type was decided (module docs, "Typing").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Typed {
    /// Not typed (the components were not built, or not yet typed).
    #[default]
    Untyped,
    /// Every box regular: its distinct certified roots, each a transversal crossing.
    Count(usize),
    /// The side function at its two ends.
    Sides,
    /// The side of the foot at its two ends (no side function: the foot rule).
    Foot,
    /// The side of the foot along a frame at its two ends (the closest-point foot not
    /// certified at a stretch at the floor: [`by_frame`]).
    FrameFoot,
    /// Undecided, and why.
    Undecided(Why),
}

/// Why a component is undecided (module docs, "Typing").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Why {
    /// Its line parameters reach an end of the line's own interval, and that end is not
    /// certified off the net (or not finite), or a certified root reaches across it.
    ReachesEnd,
    /// It holds or touches a leaf whose pass stopped at its limit.
    Unfinished,
    /// It holds or touches a leaf whose pass did not run.
    NotRun,
    /// It touches a stretch no searched leaf or strip covers (a trim cull).
    Unsearched,
    /// It holds a box of a culled strip that step 1 did not exclude.
    Culled,
    /// It holds a box with a certified root not reported.
    Unshown,
    /// Every box regular, but one has a side on a boundary (the surface's, the solve's): a
    /// root there may be another patch's or face's too.
    OnABoundary,
    /// Every box regular, but two of its certified roots are neither shown one nor apart.
    RootsNotTold,
    /// No side function for the kind (not reached: a surface without one takes the foot
    /// rule).
    NoSideFunction,
    /// The net over its hull is not shown within a quarter of the tolerance of the closed
    /// form (a patch without a certified bound, or one too large).
    NotTheNet,
    /// The side function at an end is not proved apart from 0.
    EndNotShown,
    /// Another component's line parameters meet its own, and what is known of the two.
    Overlapped(Overlap),
    /// The cone's radius at the net's heights over its hull is not shown positive (the
    /// apex, or the other nappe).
    Apex,
    /// Its hull is not covered by the searched leaves and strips (a trim cull inside it).
    Uncovered,
    /// A point of the line between its ends that may lie on the complete surface is not
    /// shown strictly inside the chart its hull's edges certify, or no chart is certified
    /// (a pole or a whole turn on its edge, the net near the axis).
    OutsideHull,
    /// The side test ran out of its budget.
    Budget,
    /// The foot's branch is not certified over its line parameters (the foot rule), and
    /// where it failed.
    FootNotShown(FootWhy),
    /// It holds a box left at the floor and its boxes cover the whole knots' range in a
    /// direction the solve wraps in: no lift of it is a rectangle less than a turn wide, which
    /// the side chart (its degree and winding) and the foot chain read (module docs, "The
    /// seam"). A count reads no lift and is not refused (answer11, Q3).
    WholeTurn,
}

/// What is known of a component another's line parameters meet ([`Why::Overlapped`]):
/// each other component whose clipped line parameters meet its own is told by the first of
/// these that holds for the two, and the component takes the last told of them (so
/// [`Overlap::Seam`] means every one lies across a seam from it). A diagnosis only: none of
/// them decides anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Overlap {
    /// A box of each lies on an opposite end of the knots' range in a direction in which
    /// the stored net closes exactly ([`LineNet::closes`]), their stretches along those ends
    /// meeting: one contact across the seam that the join did not join (their boxes'
    /// line parameters apart there, or the solve not wrapping: over part of the range).
    Seam,
    /// The same on the ends of a direction in which the net is not shown to close (a
    /// sphere's two poles).
    Ends,
    /// The boxes of one are all left at the floor, and the line parameters of the two
    /// without the tolerance's widening (each box's widened by `tol / |D|`, rounded
    /// outward) are apart.
    Widening,
    /// None of these, or past the tests' bound.
    Other,
}

/// Where the foot rule ([`by_foot`]) failed ([`Why::FootNotShown`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FootWhy {
    /// Two neighbouring stretches are not chained.
    NotChained,
    /// A stretch halved to the floor of the line parameters has no foot box: why its last
    /// test failed.
    Floor(Stretch),
    /// The foot's side at `t₀` cannot be formed (no stretch there, or its tight foot box is
    /// not shown).
    NoSideAtStart,
    /// The foot's side at `t₁` cannot be formed.
    NoSideAtEnd,
    /// The component has no frame ([`Frame::of`]: its middle box is not enclosed, or `S_u ×
    /// S_v` has no length there). The foot rule along a frame only.
    NoFrame,
}

/// Why a stretch of the line parameters has no foot box ([`foot_box`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Stretch {
    /// The foot rule's budget ran out in its widenings.
    Budget,
    /// Newton's steps toward the foot at the stretch's middle do not settle (a singular
    /// Jacobian, a step not finite, or the net refusing a point on its knots' range).
    Newton,
    /// Newton's steps toward the foot at the stretch's middle leave the knots' range, or
    /// settle off it, past an end the solve does not wrap at (a border of the net) or at a
    /// point with a coordinate that is no number.
    NewtonOff,
    /// The box reaches the net's knots' range or the solve's domain.
    LeavesDomain,
    /// A crease stops the rule: Krawczyk's operator cannot be formed because the box crosses
    /// a crease knot line (the net below `C¹` across it), or it reaches only across a seam
    /// the solve wraps, which it would straddle; or, before any box, Newton's steps toward
    /// the foot leave the knots' range only past ends the solve wraps at (the same seam, one
    /// step earlier: answer12, Q1).
    Crease,
    /// Krawczyk's operator cannot be formed: the Jacobian at the box's middle is not
    /// inverted.
    Singular,
    /// Krawczyk's operator cannot be formed: the net refuses the box, or a value is not
    /// finite.
    NotFinite,
    /// The operator does not lie strictly inside the box within [`FOOT_WIDENINGS`], or
    /// (the foot rule along a frame) its contraction is not below 1: no matrix of the
    /// Jacobian over the box is shown nonsingular, as where a crease folds the surface over
    /// relative to the frame.
    NoContraction,
    /// It does, but a box of the component whose line parameters meet the stretch does not
    /// lie in the box (not reached by the closest-point rule: the box is grown from them;
    /// the foot rule along a frame, whose box is pieces made from a rounded stretch, tests
    /// it on the pieces, and gives it too where Newton's foot lies on no piece).
    HeldOutside,
    /// It does, but the box is not covered by the searched leaves and strips.
    Uncovered,
    /// It does, but `S_u × S_v` is not shown apart from 0 over the box.
    Degenerate,
    /// The box crosses a knot line whose multiplicity is above the degree, a break of the
    /// net (the foot rule along a frame, which crosses a knot line the net is continuous
    /// across).
    Break,
    /// The box would be a whole turn wide in a direction the solve wraps in (the foot rule
    /// along a frame, whose boxes may reach across a seam).
    Turn,
}

/// Types every component of `comps` (module docs, "Typing"): the solve's `leaves`, `sides`
/// what lies across each, the line `line` and the surface's net `net`. Components not built
/// are left as they are.
///
/// # Errors
///
/// A cancel.
pub(crate) fn type_components(
    net: &LineNet,
    line: &PassLine,
    leaves: &[SearchedLeaf],
    sides: &[LeafSides],
    comps: &mut Components,
    cancel: Option<&CancelToken>,
) -> Result<()> {
    if comps.over_budget {
        return Ok(());
    }
    let [s0, s1] = line.span;
    let scratch = ScratchLine::of(line, &Extent1d::from_min_max(s0, s1));
    // each component's line parameters clipped to the line's own interval (answer8, Q2):
    // `None` where nothing finite is left
    let clip = |t: Ivl| {
        meet(t, Ivl::new(s0, s1))
            .filter(|x| !(t.lo.is_nan() || t.hi.is_nan()) && x.lo.is_finite() && x.hi.is_finite())
    };
    let clipped: Vec<Option<Ivl>> = comps.list.iter().map(|c| clip(c.t)).collect();
    let overlapped = overlapped(
        &clipped
            .iter()
            .map(|t| t.unwrap_or_else(|| Ivl::new(f64::NAN, f64::NAN)))
            .collect::<Vec<Ivl>>(),
    );
    // what is known of each overlapped component a side test or the foot rule may read
    // (one holding a box left at the floor, not undecided by its flags)
    let wanted: Vec<bool> = comps
        .list
        .iter()
        .zip(&overlapped)
        .zip(&clipped)
        .map(|((c, &o), t)| o && t.is_some() && c.holds.left > 0 && undecided_by_flags(c).is_none())
        .collect();
    let overlaps = match &scratch {
        Some(scratch) if wanted.contains(&true) => {
            overlap_reasons(net, scratch, comps, &clipped, &wanted)
        }
        _ => vec![Overlap::Other; comps.list.len()],
    };
    // the net's side bounds, made once for the solve when a side test first reads them (a
    // decomposition that fails gives none: every side test is then undecided)
    let mut bounds: Option<Vec<SideBound>> = None;
    for (ci, c) in comps.list.iter_mut().enumerate() {
        if let Some(token) = cancel {
            token.check(OP)?;
        }
        let boxes: Vec<PlacedBox> = c
            .boxes
            .iter()
            .filter_map(|&i| comps.boxes.get(i).copied())
            .collect();
        let t = clipped.get(ci).copied().flatten();
        c.at_boundary = reaches_a_boundary(net, line, (leaves, sides), c, &boxes);
        let typed = match (undecided_by_flags(c), &scratch, t) {
            (Some(why), _, _) => Typed::Undecided(why),
            (None, None, _) => Typed::Undecided(Why::NotRun),
            (None, Some(_), None) => Typed::Undecided(Why::ReachesEnd),
            (None, Some(scratch), Some(t)) => {
                let others = overlapped
                    .get(ci)
                    .copied()
                    .unwrap_or(true)
                    .then(|| overlaps.get(ci).copied().unwrap_or(Overlap::Other));
                type_one(
                    net,
                    scratch,
                    (leaves, sides),
                    c,
                    &boxes,
                    (t, others),
                    &mut bounds,
                    cancel,
                )?
            }
        };
        c.contact = match typed {
            Typed::Count(n) if n % 2 == 1 => Contact::Crossing,
            Typed::Count(_) => Contact::Touching,
            Typed::Sides | Typed::Foot | Typed::FrameFoot => c.contact,
            _ => Contact::Undecided,
        };
        c.typed = typed;
    }
    Ok(())
}

/// Types one component not undecided by its flags (module docs, "Typing"), its line
/// parameters `t` clipped to the line's own interval and `others` what is known of it where
/// another component's (clipped) meet them: its certified roots are split by the interval (those
/// wholly past it are the component's [`Component::beyond`], one reaching across its end
/// undecided), a certified root not reported and not wholly past it makes it undecided, and
/// it is typed by its count, its sides or its foot. A component reaching an end is decided
/// only with that end certified off the net ([`end_off_the_net`]: by its count; the side
/// test asks more of the side function there, the foot rules read the foot's side there).
/// Where the closest-point foot rule fails at a stretch at the floor, the foot rule along a
/// frame is the next rule ([`by_frame`]).
#[allow(clippy::too_many_arguments)]
fn type_one(
    net: &LineNet,
    line: &ScratchLine,
    (leaves, sides): (&[SearchedLeaf], &[LeafSides]),
    c: &mut Component,
    boxes: &[PlacedBox],
    (t, others): (Ivl, Option<Overlap>),
    bounds: &mut Option<Vec<SideBound>>,
    cancel: Option<&CancelToken>,
) -> Result<Typed> {
    // (a whole turn has no lift less than a turn wide, which only the side chart and the
    // foot chain read; the count reads no lift, its boxes all regular, each root unique in
    // its native box and off the seam: answer11, Q3)
    if c.whole_turn && c.holds.left > 0 {
        return Ok(Typed::Undecided(Why::WholeTurn));
    }
    // a certified root whose enclosure reaches a seam the solve wraps is not counted, by any
    // rule: its unique box and one on the seam's other end cannot be shown to hold one root
    // without `C¹` across the seam (module docs, "The seam")
    let seams = net.seams(&line.solve_domain);
    let [[u0, u1], [v0, v1]] = seams.range;
    let [wu, wv] = seams.wrap;
    let on_seam = boxes.iter().any(|b| match b.part {
        Part::Floor(FloorKind::Root { enclosure, .. } | FloorKind::Unshown { enclosure, .. }) => {
            let [eu, ev, _] = enclosure;
            (wu && (eu.lo <= u0 || eu.hi >= u1)) || (wv && (ev.lo <= v0 || ev.hi >= v1))
        }
        _ => false,
    });
    if on_seam {
        return Ok(Typed::Undecided(Why::OnABoundary));
    }
    let [s0, s1] = line.span;
    let past = |e: &I3| {
        let [_, _, et] = *e;
        et.hi < s0 || et.lo > s1
    };
    let within_span = |e: &I3| {
        let [_, _, et] = *e;
        et.lo >= s0 && et.hi <= s1
    };
    let mut counted: Vec<PlacedBox> = Vec::new();
    let mut beyond: Vec<PlacedBox> = Vec::new();
    let mut past_unshown: Vec<Vec<[Ivl; 2]>> = Vec::new();
    for b in boxes {
        match b.part {
            Part::Floor(FloorKind::Root { enclosure, unique }) => {
                // (an enclosure reaching across the end is narrowed about its root first)
                let tight = if within_span(&enclosure) || past(&enclosure) {
                    Some(enclosure)
                } else {
                    let point = c
                        .roots
                        .iter()
                        .find(|r| r.unique == unique && r.enclosure == enclosure)
                        .map_or_else(|| mids(&enclosure), |r| r.point);
                    tight_root(net, line, point, unique)
                };
                match tight {
                    Some(e) if within_span(&e) => counted.push(*b),
                    Some(e) if past(&e) => beyond.push(*b),
                    _ => return Ok(Typed::Undecided(Why::ReachesEnd)),
                }
            }
            Part::Floor(FloorKind::Unshown { enclosure, .. }) if !past(&enclosure) => {
                return Ok(Typed::Undecided(Why::Unshown));
            }
            // (a root past the interval not reported: its unique box, like a reported one's)
            Part::Floor(FloorKind::Unshown { unique, .. }) => {
                past_unshown.push(vec![unique]);
            }
            _ => counted.push(*b),
        }
    }
    let (Some(classes), Some(far)) = (root_classes(&counted), root_classes(&beyond)) else {
        return Ok(Typed::Undecided(Why::RootsNotTold));
    };
    let n = classes.len();
    c.classes = classes;
    c.beyond = far;
    c.beyond.extend(past_unshown);
    let ends = [c.t.lo <= s0, c.t.hi >= s1];
    let typed = if c.holds.left == 0 {
        by_count(net, line, sides, boxes, n)
    } else if net.revolved.is_none() {
        match by_foot(net, line, leaves, boxes, c, (t, others)) {
            // the closest-point foot not certified at a stretch at the floor: the foot
            // along a frame (answer12, Q2), the next rule and only here, so it never moves a
            // component the closest-point rule decides; failing too, that rule's reason
            // stays
            closest @ Typed::Undecided(Why::FootNotShown(FootWhy::Floor(_))) => {
                match by_frame(net, line, leaves, boxes, c, t) {
                    Ok(contact) => {
                        c.contact = contact;
                        Typed::FrameFoot
                    }
                    Err(why) => {
                        c.frame_why = Some(why);
                        closest
                    }
                }
            }
            typed => typed,
        }
    } else {
        let bounds = match bounds {
            Some(b) => b,
            // (made with the solve's cancel token: a stop while the patches are composed is
            // the solve's stop, never a missing bound)
            None => bounds.insert(net.side_bounds(cancel)?),
        };
        by_sides(net, line, leaves, c, (t, ends, others), bounds)
    };
    // every clipped end of a decided component certified farther than the tolerance from
    // the net over it (a box at the floor straddling the end enclosed whole): an end within
    // the tolerance of the net stays undecided
    if matches!(
        typed,
        Typed::Count(_) | Typed::Sides | Typed::Foot | Typed::FrameFoot
    ) {
        let mut budget = MAX_SIDE_PIECES;
        let [lo_end, hi_end] = ends;
        let certified = [(lo_end, t.lo), (hi_end, t.hi)]
            .iter()
            .all(|&(clipped, at)| !clipped || end_off_the_net(net, line, boxes, at, &mut budget));
        if !certified {
            c.contact = Contact::Undecided;
            return Ok(Typed::Undecided(Why::ReachesEnd));
        }
    }
    Ok(typed)
}

/// A box about the certified root at `point` (Newton's, of the root the `(u, v)` box
/// `unique` holds alone, with any line parameter) proved to hold a root by Krawczyk's
/// operator about `point` ([`krawczyk_region`], epsilon-inflation): that root, its box
/// lying in `unique`. `None` where none is shown.
fn tight_root(net: &LineNet, line: &ScratchLine, point: [f64; 3], unique: [Ivl; 2]) -> Option<I3> {
    let [pu, pv, _] = point;
    let [_, s_u, s_v] = net.over(Ivl::from(pu), Ivl::from(pv), line.p)?;
    let y = inverse_of_columns3(&[mids(&s_u), mids(&s_v), line.d.map(|e| -e)])?;
    let value = |x: [Ivl; 3]| -> Result<Option<[Ivl; 3]>> {
        let [u, v, t] = x;
        Ok(net.over(u, v, line.p).map(|[s, _, _]| less(&s, t, line.d)))
    };
    let jacobian = |x: [Ivl; 3]| -> Result<Option<[[Ivl; 3]; 3]>> {
        let [u, v, _] = x;
        Ok(net
            .over(u, v, line.p)
            .map(|[_, s_u, s_v]| [s_u, s_v, line.d.map(|e| Ivl::from(-e))]))
    };
    let (root, _) = krawczyk_region(point, y, &value, &jacobian).ok()??;
    let [ru, rv, _] = root;
    let [qu, qv] = unique;
    (within(ru, qu) && within(rv, qv)).then_some(root)
}

/// Whether the line's point `E = L(at)` at a clipped end of a component is certified
/// farther than the tolerance from the net over the component (answer8, Q2): each box of
/// the component whose line parameters hold `at` has `S − E` enclosed longer than the
/// tolerance, a regular box halved within `budget` (a box at the floor, not visited or of a
/// culled strip is enclosed whole). A box whose line parameters do not hold `at` needs no
/// test: its line parameters are its points' feet widened by the tolerance as a stretch of
/// the line (`tol / |D|`, rounded up), so every point of it has its foot more than that
/// from `at`, and lies more than the tolerance from `E` along the line; a point in no box
/// was excluded by distance, farther than the tolerance from the whole line.
fn end_off_the_net(
    net: &LineNet,
    line: &ScratchLine,
    boxes: &[PlacedBox],
    at: f64,
    budget: &mut usize,
) -> bool {
    let [p0, p1, p2] = line.p;
    let [d0, d1, d2] = line.d;
    let e = [
        Ivl::from(p0) + Ivl::from(at) * Ivl::from(d0),
        Ivl::from(p1) + Ivl::from(at) * Ivl::from(d1),
        Ivl::from(p2) + Ivl::from(at) * Ivl::from(d2),
    ];
    let about = mids(&e);
    let [a0, a1, a2] = about;
    let [e0, e1, e2] = e;
    // `E − about`, enclosed
    let off = [e0 - Ivl::from(a0), e1 - Ivl::from(a1), e2 - Ivl::from(a2)];
    let tol2 = (Ivl::from(line.tol) * Ivl::from(line.tol)).hi;
    for b in boxes.iter().filter(|b| b.t.lo <= at && at <= b.t.hi) {
        let regular = matches!(
            b.part,
            Part::Floor(FloorKind::Edge | FloorKind::NoRoot | FloorKind::Root { .. })
        );
        let mut work = vec![(b.u, b.v)];
        while let Some((u, v)) = work.pop() {
            let shown = net.value_over(u, v, about).is_some_and(|w| {
                let [w0, w1, w2] = w;
                let [o0, o1, o2] = off;
                length_squared(&[w0 - o0, w1 - o1, w2 - o2]).lo > tol2
            });
            if shown {
                continue;
            }
            if !regular || *budget == 0 {
                return false;
            }
            *budget = budget.saturating_sub(1);
            let split_u = u.hi - u.lo >= v.hi - v.lo;
            let (x, mid) = if split_u { (u, u.mid()) } else { (v, v.mid()) };
            if !(mid > x.lo && mid < x.hi) {
                return false;
            }
            if split_u {
                work.push((Ivl::new(u.lo, mid), v));
                work.push((Ivl::new(mid, u.hi), v));
            } else {
                work.push((u, Ivl::new(v.lo, mid)));
                work.push((u, Ivl::new(mid, v.hi)));
            }
        }
    }
    true
}

/// For each of the closed intervals `hulls`, whether another of them meets it: sorted by
/// their lower ends, an interval meets an earlier one when the greatest upper end before it
/// reaches its lower end, and a later one when the next lower end lies within it (every
/// later one starts no lower). A NaN end meets everything.
fn overlapped(hulls: &[Ivl]) -> Vec<bool> {
    if hulls.iter().any(|h| h.lo.is_nan() || h.hi.is_nan()) {
        return vec![hulls.len() > 1; hulls.len()];
    }
    let mut order: Vec<usize> = (0..hulls.len()).collect();
    let lo = |i: usize| hulls.get(i).map_or(f64::NAN, |h| h.lo);
    order.sort_by(|&a, &b| lo(a).total_cmp(&lo(b)).then(a.cmp(&b)));
    let mut out = vec![false; hulls.len()];
    let mut reach = f64::NEG_INFINITY;
    for (k, &i) in order.iter().enumerate() {
        let Some(h) = hulls.get(i) else {
            continue;
        };
        let before = reach >= h.lo;
        let after = order
            .get(k + 1)
            .and_then(|&j| hulls.get(j))
            .is_some_and(|n| n.lo <= h.hi);
        if let Some(slot) = out.get_mut(i) {
            *slot = before || after;
        }
        reach = reach.max(h.hi);
    }
    out
}

/// What is known of each component of `comps` flagged in `wanted` where another's line
/// parameters meet its own ([`Overlap`]; `clipped` each one's line parameters clipped to
/// the line's own interval, `None` where nothing finite is left); [`Overlap::Other`] for
/// the components not wanted. The other components are tested within [`MAX_MEETING_TESTS`]
/// pairs of components and of boxes (a component past it is [`Overlap::Other`]). (No two
/// components share a leaf's border: boxes meeting in `(u, v)` share a point, whose feet
/// both boxes' line parameters hold, so [`components`] joined them.)
fn overlap_reasons(
    net: &LineNet,
    line: &ScratchLine,
    comps: &Components,
    clipped: &[Option<Ivl>],
    wanted: &[bool],
) -> Vec<Overlap> {
    let n = comps.list.len();
    let want = |c: usize| wanted.get(c).copied().unwrap_or(false);
    // each component's boxes on each end of the knots' range: u0, u1, v0, v1
    let [[u0, u1], [v0, v1]] = net.range;
    let ends: Vec<[Vec<PlacedBox>; 4]> = comps
        .list
        .iter()
        .map(|c| {
            let mut e: [Vec<PlacedBox>; 4] = Default::default();
            for b in c.boxes.iter().filter_map(|&i| comps.boxes.get(i)) {
                let on = [b.u.lo <= u0, b.u.hi >= u1, b.v.lo <= v0, b.v.hi >= v1];
                for (slot, _) in e.iter_mut().zip(on).filter(|(_, o)| *o) {
                    slot.push(*b);
                }
            }
            e
        })
        .collect();
    // the direction (0 for u, 1 for v) in which a box of `a` and one of `b` lie on opposite
    // ends with their stretches along them meeting, counting each pair tested in `tests`
    let opposite = |a: usize, b: usize, tests: &mut usize| -> Option<usize> {
        let (ea, eb) = (ends.get(a)?, ends.get(b)?);
        for (s, o) in [(0, 1), (1, 0), (2, 3), (3, 2)] {
            let (Some(xa), Some(xb)) = (ea.get(s), eb.get(o)) else {
                continue;
            };
            for p in xa {
                for q in xb {
                    *tests = tests.saturating_add(1);
                    if *tests > MAX_MEETING_TESTS {
                        return None;
                    }
                    let (x, y) = if s < 2 { (p.v, q.v) } else { (p.u, q.u) };
                    if x.lo <= y.hi && y.lo <= x.hi {
                        return Some(s / 2);
                    }
                }
            }
        }
        None
    };
    // each one's line parameters without the widening: its points' feet (not clipped)
    let feet: Vec<Ivl> = comps
        .list
        .iter()
        .map(|c| {
            let lo = round_down(c.t.lo + line.tol_t);
            Ivl::new(lo, round_up(c.t.hi - line.tol_t).max(lo))
        })
        .collect();
    let at_floor = |c: usize| {
        comps
            .list
            .get(c)
            .is_some_and(|x| x.holds.left == x.boxes.len())
    };
    let mut tests = 0_usize;
    let mut out = vec![Overlap::Other; n];
    for a in (0..n).filter(|&a| want(a)) {
        let Some(Some(ta)) = clipped.get(a).copied() else {
            continue;
        };
        let mut told: Option<Overlap> = None;
        for b in (0..n).filter(|&b| b != a) {
            tests = tests.saturating_add(1);
            if tests > MAX_MEETING_TESTS {
                told = Some(Overlap::Other);
                break;
            }
            let tb = clipped.get(b).copied().flatten();
            if tb.is_some_and(|tb| !(ta.lo <= tb.hi && tb.lo <= ta.hi)) {
                continue;
            }
            let apart = match (feet.get(a), feet.get(b)) {
                (Some(x), Some(y)) => x.hi < y.lo || y.hi < x.lo,
                _ => false,
            };
            let pair = match opposite(a, b, &mut tests) {
                Some(dir) if net.closes.get(dir).copied().unwrap_or(false) => Overlap::Seam,
                Some(_) => Overlap::Ends,
                None if apart && (at_floor(a) || at_floor(b)) => Overlap::Widening,
                None => Overlap::Other,
            };
            told = Some(told.map_or(pair, |t| t.max(pair)));
        }
        if let Some(slot) = out.get_mut(a) {
            *slot = told.unwrap_or(Overlap::Other);
        }
    }
    out
}

/// Whether the component `c` (its boxes `boxes`) reaches a boundary side
/// ([`Component::at_boundary`]): a box with a side of [`SideKind::Boundary`] (a box of a
/// leaf with no record of what lies across it counts), a root's (or root not reported)
/// enclosure reaching the knots' range or the solve's border where the solve does not wrap
/// (as the count rule reads it), a box in a leaf the trim crosses (a culled strip is a
/// distance cull, not one), a stretch nothing searched it touches, or its hull not shown
/// covered.
fn reaches_a_boundary(
    net: &LineNet,
    line: &PassLine,
    (leaves, sides): (&[SearchedLeaf], &[LeafSides]),
    c: &Component,
    boxes: &[PlacedBox],
) -> bool {
    let seams = net.seams(&line.solve_domain);
    let [[u0, u1], [v0, v1]] = seams.range;
    let [wu, wv] = seams.wrap;
    let (lo, hi) = (line.solve_domain.min(), line.solve_domain.max());
    let on_one = boxes.iter().any(|b| {
        let side = sides.get(b.leaf).is_none_or(|own| {
            net.side_kinds(&line.solve_domain, own, Scratch { u: b.u, v: b.v })
                .contains(&SideKind::Boundary)
        });
        let straddled = leaves.get(b.leaf).is_none_or(|l| !l.inside && !l.strip);
        let root = match b.part {
            Part::Floor(
                FloorKind::Root { enclosure, .. } | FloorKind::Unshown { enclosure, .. },
            ) => {
                let [eu, ev, _] = enclosure;
                (!wu && (eu.lo <= u0.max(lo.x) || eu.hi >= u1.min(hi.x)))
                    || (!wv && (ev.lo <= v0.max(lo.y) || ev.hi >= v1.min(hi.y)))
            }
            _ => false,
        };
        side || straddled || root
    });
    on_one
        || c.touches.unsearched
        || !hull_pieces(&c.hull)
            .iter()
            .all(|&[u, v]| covered_by_leaves(u, v, leaves))
}

/// What makes a component undecided whatever its sides (answer to the typing's design: a
/// leaf not decided, a trim cull, a strip box not excluded).
fn undecided_by_flags(c: &Component) -> Option<Why> {
    if c.touches.unfinished || c.holds.unvisited > 0 {
        Some(Why::Unfinished)
    } else if c.touches.not_run || c.holds.not_run > 0 {
        Some(Why::NotRun)
    } else if c.touches.unsearched {
        Some(Why::Unsearched)
    } else if c.holds.culled > 0 {
        Some(Why::Culled)
    } else {
        None
    }
}

/// The most root boxes [`root_classes`] tells apart in one component (pairs tested: their
/// square); past it the roots are not told.
const MAX_ROOT_BOXES: usize = 256;

/// The distinct certified roots of the root boxes among `boxes` (a component's): two root
/// boxes hold one root where the enclosure of either lies in the other's unique box (it
/// holds one root), two where their enclosures are apart; `None` where two are neither.
/// Each root is given as the unique boxes of the root boxes holding it, by its first box.
fn root_classes(boxes: &[PlacedBox]) -> Option<Vec<Vec<[Ivl; 2]>>> {
    let roots: Vec<([Ivl; 2], I3)> = boxes
        .iter()
        .filter_map(|b| match b.part {
            Part::Floor(FloorKind::Root { unique, enclosure }) => Some((unique, enclosure)),
            _ => None,
        })
        .collect();
    if roots.len() > MAX_ROOT_BOXES {
        return None;
    }
    let in_unique = |e: &I3, [qu, qv]: [Ivl; 2]| {
        let [eu, ev, _] = *e;
        within(eu, qu) && within(ev, qv)
    };
    let mut parent: Vec<usize> = (0..roots.len()).collect();
    for (i, (ui, ei)) in roots.iter().enumerate() {
        for (j, (uj, ej)) in roots.iter().enumerate().skip(i + 1) {
            if in_unique(ei, *uj) || in_unique(ej, *ui) {
                join(&mut parent, i, j);
            } else if !apart(ei, ej) {
                return None;
            }
        }
    }
    let mut classes: Vec<Vec<[Ivl; 2]>> = Vec::new();
    let mut slot_of: Vec<Option<usize>> = vec![None; roots.len()];
    for (i, (unique, _)) in roots.iter().enumerate() {
        let first = root_of(&mut parent, i);
        let k = if let Some(k) = slot_of.get(first).copied().flatten() {
            k
        } else {
            let k = classes.len();
            classes.push(Vec::new());
            if let Some(slot) = slot_of.get_mut(first) {
                *slot = Some(k);
            }
            k
        };
        if let Some(class) = classes.get_mut(k) {
            class.push(*unique);
        }
    }
    Some(classes)
}

/// A component every box of which is regular (no box left at the floor; with the flags
/// clear, its `boxes` are roots, boxes with no root, edge boxes and roots not reported
/// wholly past the line's own interval): every exact root of the line on the surface over
/// it within the interval lies in a root box, each a transversal crossing, so its parity is
/// the count `roots` of its distinct roots within the interval. Undecided when an edge box has a side
/// on a boundary (the surface's or the solve's, not a seam the solve wraps, which is a
/// crease: the contact may lie past it) or a root's enclosure reaches the knots' range or
/// the solve's border (the patch or face across may count it too; at a seam, the root's
/// unique box and one on the other end cannot be shown to hold one root).
fn by_count(
    net: &LineNet,
    line: &ScratchLine,
    sides: &[LeafSides],
    boxes: &[PlacedBox],
    roots: usize,
) -> Typed {
    let [[u0, u1], [v0, v1]] = net.range;
    let (lo, hi) = (line.solve_domain.min(), line.solve_domain.max());
    let on_a_boundary = boxes.iter().any(|b| match b.part {
        Part::Floor(FloorKind::Edge) => sides.get(b.leaf).is_none_or(|own| {
            net.side_kinds(&line.solve_domain, own, Scratch { u: b.u, v: b.v })
                .contains(&SideKind::Boundary)
        }),
        Part::Floor(FloorKind::Root { enclosure, .. }) => {
            let [eu, ev, _] = enclosure;
            eu.lo <= u0.max(lo.x)
                || eu.hi >= u1.min(hi.x)
                || ev.lo <= v0.max(lo.y)
                || ev.hi >= v1.min(hi.y)
        }
        _ => false,
    });
    if on_a_boundary {
        return Typed::Undecided(Why::OnABoundary);
    }
    Typed::Count(roots)
}

/// The side test of a component holding a box left at the floor (module docs, "Typing"),
/// for a surface of revolution: the side function at the line's points at the two ends of
/// its line parameters, each proved more than half the tolerance from 0; no other
/// component's line parameters meet its own (`others`, what is known of it where one
/// does); its hull covered by searched leaves
/// and strips; the net over its hull within a certified `ε` below a quarter of the
/// tolerance of the closed form (`bounds`, [`side_bounds`]), on a cone's one nappe; and
/// every point of the line between the ends that may lie within `ε` of the complete
/// surface strictly inside the chart its hull's edges certify ([`Chart`]). Sets the
/// component's contact (crossing for opposite sides, touching for one) and returns
/// [`Typed::Sides`], or why it is undecided.
fn by_sides(
    net: &LineNet,
    line: &ScratchLine,
    leaves: &[SearchedLeaf],
    c: &mut Component,
    (t, clipped, others): (Ivl, [bool; 2], Option<Overlap>),
    bounds: &[SideBound],
) -> Typed {
    let Some(rev) = net.revolved else {
        return Typed::Undecided(Why::NoSideFunction);
    };
    let origin_off = line.off_the_start(&[Ivl::from(0.0); 3], rev.origin);
    // the points `P + t D − origin` for t in `t`, enclosed
    let at = |t: Ivl| -> I3 {
        let [o0, o1, o2] = origin_off;
        let [d0, d1, d2] = line.d.map(Ivl::from);
        [t * d0 - o0, t * d1 - o1, t * d2 - o2]
    };
    let ends = [t.lo, t.hi].map(|x| rev.side(&at(Ivl::from(x))));
    // (half the tolerance: the ends are at least the tolerance from the net over the
    // component, and the net is within a quarter of it of the closed form)
    let half = 0.5 * line.tol;
    let shown = |f: Ivl| f.lo > half || f.hi < -half;
    let [lo, hi] = ends;
    if !(shown(lo) && shown(hi)) {
        return Typed::Undecided(Why::EndNotShown);
    }
    if let Some(why) = others {
        return Typed::Undecided(Why::Overlapped(why));
    }
    // (the hull's native pieces: one, or, glued across a seam, two or four)
    let pieces = hull_pieces(&c.hull);
    if !pieces.iter().all(|&[u, v]| covered_by_leaves(u, v, leaves)) {
        return Typed::Undecided(Why::Uncovered);
    }
    let mut eps = 0.0_f64;
    for &[u, v] in &pieces {
        match side_eps(net, &rev, bounds, u, v) {
            Ok(e) => eps = eps.max(e),
            Err(why) => return Typed::Undecided(why),
        }
    }
    if !(eps < 0.25 * line.tol) {
        return Typed::Undecided(Why::NotTheNet);
    }
    // an end clipped to the line's own interval is certified off the net: the side function
    // there more than half the tolerance and `ε` from 0 (the net over the hull within `ε` of
    // the complete surface, `|f|` a distance to it, or to the cone's profile line, no more)
    let margin = (Ivl::from(half) + Ivl::from(eps)).hi;
    let [lo_clipped, hi_clipped] = clipped;
    let off = |f: Ivl| f.lo > margin || f.hi < -margin;
    if (lo_clipped && !off(lo)) || (hi_clipped && !off(hi)) {
        return Typed::Undecided(Why::ReachesEnd);
    }
    let mut budget = MAX_SIDE_PIECES;
    if !pieces
        .iter()
        .all(|&[u, v]| in_the_chart_domain(net, &rev, u, v, eps))
    {
        return Typed::Undecided(Why::OutsideHull);
    }
    let Some(mut chart) = Chart::of(net, &rev, c.hull, &mut budget) else {
        return Typed::Undecided(if budget == 0 {
            Why::Budget
        } else {
            Why::OutsideHull
        });
    };
    // every piece of the line between the ends that may meet the net lies strictly in the
    // chart, halving pieces within the budget
    let mut work = vec![t];
    while let Some(piece) = work.pop() {
        if budget == 0 {
            return Typed::Undecided(Why::Budget);
        }
        budget = budget.saturating_sub(1);
        let w = at(piece);
        let f = rev.side(&w);
        if shown(f) {
            continue;
        }
        if chart.holds(net, &rev, &w, &mut budget) {
            continue;
        }
        let mid = piece.mid();
        if !(mid > piece.lo && mid < piece.hi) {
            return Typed::Undecided(Why::OutsideHull);
        }
        work.push(Ivl::new(mid, piece.hi));
        work.push(Ivl::new(piece.lo, mid));
    }
    c.contact = if (lo.lo > 0.0) == (hi.lo > 0.0) {
        Contact::Touching
    } else {
        Contact::Crossing
    };
    Typed::Sides
}

/// `a × b` of two vectors of intervals, enclosed.
fn cross_i(a: &I3, b: &I3) -> I3 {
    let [ax, ay, az] = *a;
    let [bx, by, bz] = *b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

/// `a · b` of two vectors of intervals, enclosed.
fn dot_i(a: &I3, b: &I3) -> Ivl {
    a.iter()
        .zip(b)
        .fold(Ivl::from(0.0), |s, (&x, &y)| s + x * y)
}

/// The most Krawczyk tests one component's foot rule makes (module docs, "Typing"): the
/// side test's own bound.
pub(crate) const MAX_FOOT_BOXES: usize = MAX_SIDE_PIECES;

/// The most widenings of one foot box's Krawczyk test (epsilon-inflation).
const FOOT_WIDENINGS: usize = 6;

/// One stretch `T` of a component's line parameters with its foot box `U` (module docs,
/// "Typing", the foot rule): for every `t` in `T` exactly one foot in `U`, held in `k`.
#[derive(Debug, Clone, Copy)]
struct FootBox {
    t: Ivl,
    u: [Ivl; 2],
    k: [Ivl; 2],
}

/// The foot system `G(u, v, t) = ((S − L(t)) · S_u, (S − L(t)) · S_v)` and its Jacobian in
/// `(u, v)` over the box `u` (`[u, v]`) and the line parameters `t`, enclosed: `G` at the
/// box's middle, the Jacobian over the box (rows), and `S_u × S_v` over it; why not where
/// the net refuses the box (it leaves the knots' range, or crosses a knot line the net is
/// not `C¹` across: the second partials' mean-value form needs `S` to be `C¹`).
#[allow(clippy::type_complexity)]
fn foot_system(
    net: &LineNet,
    line: &ScratchLine,
    u: [Ivl; 2],
    t: Ivl,
) -> core::result::Result<([Ivl; 2], [[Ivl; 2]; 2], I3), Stretch> {
    let [bu, bv] = u;
    let (mu, mv) = (bu.mid(), bv.mid());
    let refused = Stretch::NotFinite;
    let [s_m, su_m, sv_m] = net
        .over(Ivl::from(mu), Ivl::from(mv), line.p)
        .ok_or(refused)?;
    let gap_m = less(&s_m, t, line.d);
    let g = [dot_i(&gap_m, &su_m), dot_i(&gap_m, &sv_m)];
    let [s, s_u, s_v] = net.over(bu, bv, line.p).ok_or(refused)?;
    let [s_uu, s_uv, s_vv] = net.second(bu, bv, line.p).ok_or_else(|| {
        let [cu, cv] = &net.creases;
        let across = |knots: &[f64], x: Ivl| knots.iter().any(|&k| k > x.lo && k < x.hi);
        if across(cu, bu) || across(cv, bv) {
            Stretch::Crease
        } else {
            refused
        }
    })?;
    let gap = less(&s, t, line.d);
    let j_uv = dot_i(&s_u, &s_v) + dot_i(&gap, &s_uv);
    let jac = [
        [dot_i(&s_u, &s_u) + dot_i(&gap, &s_uu), j_uv],
        [j_uv, dot_i(&s_v, &s_v) + dot_i(&gap, &s_vv)],
    ];
    let normal = cross_i(&s_u, &s_v);
    let finite = g.iter().chain(jac.iter().flatten()).all(|x| x.is_finite());
    if finite {
        Ok((g, jac, normal))
    } else {
        Err(refused)
    }
}

/// Krawczyk's operator of the foot system over the box `u` with the line parameters `t`
/// (`t` an interval parameter): why not where it cannot be formed.
fn foot_operator(
    net: &LineNet,
    line: &ScratchLine,
    u: [Ivl; 2],
    t: Ivl,
) -> core::result::Result<([Ivl; 2], I3), Stretch> {
    let (g, jac, normal) = foot_system(net, line, u, t)?;
    let mids = u.map(Ivl::mid);
    let [[a, b], [c, d]] = jac;
    let y =
        inverse_of_columns2(&[[a.mid(), c.mid()], [b.mid(), d.mid()]]).ok_or(Stretch::Singular)?;
    let (cm, _) = contraction(&y, &jac);
    Ok((k_operator(&mids, &y, &g, &cm, &u), normal))
}

/// The foot at the single line parameter `t` in the foot box `u` (whose operator over a
/// stretch holding `t` lies strictly inside it), enclosed tightly: the operator at `t` alone
/// strictly inside `u`, then taken again on its meet with the box a few times (each box
/// holding the one foot holds it in its operator). `None` where it cannot be formed.
fn tight_foot(net: &LineNet, line: &ScratchLine, u: [Ivl; 2], t: f64) -> Option<[Ivl; 2]> {
    let (k, _) = foot_operator(net, line, u, Ivl::from(t)).ok()?;
    if !inside2(&k, &u) {
        return None;
    }
    let mut x = k;
    for _ in 0..FOOT_WIDENINGS {
        let Ok((k, _)) = foot_operator(net, line, x, Ivl::from(t)) else {
            break;
        };
        let [ku, kv] = k;
        let [xu, xv] = x;
        let (Some(a), Some(b)) = (meet(ku, xu), meet(kv, xv)) else {
            break;
        };
        if [a, b] == x {
            break;
        }
        x = [a, b];
    }
    Some(x)
}

/// Whether `a` lies strictly inside `b` in both directions.
fn inside2(a: &[Ivl; 2], b: &[Ivl; 2]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.lo > y.lo && x.hi < y.hi)
}

/// Whether `a` lies in `b` in both directions, ends included.
fn within2(a: &[Ivl; 2], b: &[Ivl; 2]) -> bool {
    a.iter().zip(b).all(|(&x, &y)| within(x, y))
}

/// The foot box of the stretch `t` of the component whose boxes are `boxes` (module docs,
/// "Typing", the foot rule): a box `U` strictly inside the net's knots and the solve's
/// domain, holding every box of the component whose line parameters meet `t` and Newton's
/// foot at `t`'s middle, covered by the searched leaves and strips, with `S_u × S_v` apart
/// from 0 over it and Krawczyk's operator of the foot system over `U × t` strictly inside
/// it (each `t` then has exactly one foot in `U`). Why not where none is found within the
/// widenings and the budget (the last test's failure).
#[allow(clippy::too_many_arguments)]
fn foot_box(
    net: &LineNet,
    line: &ScratchLine,
    leaves: &[SearchedLeaf],
    boxes: &[PlacedBox],
    t: Ivl,
    start: [f64; 2],
    budget: &mut usize,
) -> core::result::Result<FootBox, Stretch> {
    let [[u0, u1], [v0, v1]] = net.range;
    let (lo, hi) = (line.solve_domain.min(), line.solve_domain.max());
    let (du, dv) = ([u0.max(lo.x), u1.min(hi.x)], [v0.max(lo.y), v1.min(hi.y)]);
    // the component's boxes meeting `t`, and Newton's foot at its middle
    let held: Option<[Ivl; 2]> = boxes
        .iter()
        .filter(|b| b.t.lo <= t.hi && t.lo <= b.t.hi)
        .map(|b| [b.u, b.v])
        .reduce(|[a, b], [c, d]| [a.hull(c), b.hull(d)]);
    let foot = newton_foot(net, line, t.mid(), start)?;
    let [fu, fv] = foot;
    let mut u = match held {
        Some([a, b]) => [a.hull(Ivl::from(fu)), b.hull(Ivl::from(fv))],
        None => [Ivl::from(fu), Ivl::from(fv)],
    };
    let grow = |x: Ivl| {
        let r = 0.1 * (x.hi - x.lo) + 1.0e-9 * (1.0 + x.lo.abs().max(x.hi.abs()));
        Ivl::new(round_down(x.lo - r), round_up(x.hi + r))
    };
    u = u.map(grow);
    for _ in 0..FOOT_WIDENINGS {
        if *budget == 0 {
            return Err(Stretch::Budget);
        }
        *budget = budget.saturating_sub(1);
        let [bu, bv] = u;
        let out = [
            bu.lo <= du[0],
            bu.hi >= du[1],
            bv.lo <= dv[0],
            bv.hi >= dv[1],
        ];
        if out.contains(&true) {
            // (a box reaching only across a seam the solve wraps would straddle it: a
            // crease, module docs, "The seam")
            let [wu, wv] = net.seams(&line.solve_domain).wrap;
            let seam = [wu, wu, wv, wv];
            let across = out.iter().zip(seam).all(|(&o, w)| !o || w);
            return Err(if across {
                Stretch::Crease
            } else {
                Stretch::LeavesDomain
            });
        }
        let (k, normal) = foot_operator(net, line, u, t)?;
        if inside2(&k, &u) {
            return if !held.is_none_or(|h| within2(&h, &u)) {
                Err(Stretch::HeldOutside)
            } else if !covered_by_leaves(bu, bv, leaves) {
                Err(Stretch::Uncovered)
            } else if !(length_squared(&normal).lo > 0.0) {
                Err(Stretch::Degenerate)
            } else {
                Ok(FootBox { t, u, k })
            };
        }
        let [ku, kv] = k;
        if !(ku.is_finite() && kv.is_finite()) {
            return Err(Stretch::NotFinite);
        }
        u = [bu.hull(ku), bv.hull(kv)].map(grow);
    }
    Err(Stretch::NoContraction)
}

/// Newton's foot of `L(t)` on the net from `start`: why not where the steps do not settle,
/// or leave the knots' range or settle off it. Leaving it is told by where (answer12, Q1):
/// with every point asked for off the range, and the settled point where it is off it, past
/// ends of directions the solve wraps in only ([`LineNet::seams`]), the steps stopped at a
/// seam, where a foot box would have to straddle it: [`Stretch::Crease`], the reason
/// [`foot_box`] gives a box that reaches only across such a seam, one step earlier (it names
/// the border that stopped the rule, not where the foot is: the steps may be diverging too).
/// Past any other end, or at a point with a coordinate that is no number,
/// [`Stretch::NewtonOff`]. A diagnosis only: either way the stretch has no foot box.
fn newton_foot(
    net: &LineNet,
    line: &ScratchLine,
    t: f64,
    start: [f64; 2],
) -> core::result::Result<[f64; 2], Stretch> {
    let [[u0, u1], [v0, v1]] = net.range;
    let [wu, wv] = net.seams(&line.solve_domain).wrap;
    // whether a point off the knots' range lies past ends of wrapped directions only: each
    // end it lies past (u low, u high, v low, v high, as `out` in [`foot_box`]) one the solve
    // wraps at; a point with a NaN coordinate is not
    let only_wrapped = |[u, v]: [f64; 2]| {
        let out = [u < u0, u > u1, v < v0, v > v1];
        !(u.is_nan() || v.is_nan())
            && out.contains(&true)
            && out.iter().zip([wu, wu, wv, wv]).all(|(&o, w)| !o || w)
    };
    // (whether a step asked for a point off the knots' range, and whether every such point
    // lay past wrapped ends only)
    let left = core::cell::Cell::new(false);
    let wrapped = core::cell::Cell::new(true);
    let off = |p: [f64; 2]| {
        left.set(true);
        if !only_wrapped(p) {
            wrapped.set(false);
        }
    };
    let leaving = || {
        if wrapped.get() {
            Stretch::Crease
        } else {
            Stretch::NewtonOff
        }
    };
    let at = |p: [f64; 2]| -> Result<Linearised<2>> {
        let [u, v] = p;
        let (bu, bv) = (Ivl::from(u), Ivl::from(v));
        if !net.holds(bu, bv) {
            off(p);
        }
        let (Some([s, s_u, s_v]), Some([s_uu, s_uv, s_vv])) =
            (net.over(bu, bv, line.p), net.second(bu, bv, line.p))
        else {
            return Ok(None);
        };
        let gap = less(&s, Ivl::from(t), line.d);
        let j_uv = dot_i(&s_u, &s_v) + dot_i(&gap, &s_uv);
        let g = [dot_i(&gap, &s_u).mid(), dot_i(&gap, &s_v).mid()];
        // (columns)
        let cols = [
            [(dot_i(&s_u, &s_u) + dot_i(&gap, &s_uu)).mid(), j_uv.mid()],
            [j_uv.mid(), (dot_i(&s_v, &s_v) + dot_i(&gap, &s_vv)).mid()],
        ];
        Ok(Some((g, cols)))
    };
    let p = newton(start, &at, &inverse_of_columns2)
        .ok()
        .flatten()
        .ok_or_else(|| {
            if left.get() {
                leaving()
            } else {
                Stretch::Newton
            }
        })?;
    let [pu, pv] = p;
    if net.holds(Ivl::from(pu), Ivl::from(pv)) {
        Ok(p)
    } else {
        off(p);
        Err(leaving())
    }
}

/// The side of the foot at the line parameter `t` in the foot box `f` (`t` in `f.t`):
/// `s = (S_u × S_v) · (L(t) − S)` over the tight box Krawczyk's operator gives for that `t`
/// alone, enclosed; `None` where it cannot be formed or does not lie in `f.u`.
fn foot_side(net: &LineNet, line: &ScratchLine, f: &FootBox, t: f64) -> Option<Ivl> {
    let [ku, kv] = tight_foot(net, line, f.u, t)?;
    let [s, s_u, s_v] = net.over(ku, kv, line.p)?;
    let gap = less(&s, Ivl::from(t), line.d);
    // (L − S = −gap)
    Some(Ivl::from(0.0) - dot_i(&cross_i(&s_u, &s_v), &gap))
}

/// The foot rule of a component holding a box left at the floor on a surface with no side
/// function (a B-spline, an extrusion, a general surface of revolution; module docs,
/// "Typing"): `[t₀, t₁]` covered by stretches, each with a foot box ([`foot_box`]), each two
/// neighbours chained (at their common `t`, the tight foot box of one lies in the other's
/// box), and the side of the foot ([`foot_side`]) proved apart from 0 at `t₀` and `t₁`.
/// With no other component's line parameters meeting its own (`others`, what is known of
/// it where one does) and its hull covered by searched leaves and strips. Sets the
/// component's contact (crossing for opposite sides, touching for one) and returns
/// [`Typed::Foot`], or why it is undecided (for a stretch at the floor, why its last foot
/// box failed: [`Stretch`]).
fn by_foot(
    net: &LineNet,
    line: &ScratchLine,
    leaves: &[SearchedLeaf],
    boxes: &[PlacedBox],
    c: &mut Component,
    (t, others): (Ivl, Option<Overlap>),
) -> Typed {
    if let Some(why) = others {
        return Typed::Undecided(Why::Overlapped(why));
    }
    // (the hull's native pieces; joined across a seam, a stretch whose boxes lie on both of
    // its ends has a foot box straddling it, which [`foot_box`] refuses as a crease: the
    // seam is not proved `C¹`, module docs, "The seam")
    if !hull_pieces(&c.hull)
        .iter()
        .all(|&[u, v]| covered_by_leaves(u, v, leaves))
    {
        return Typed::Undecided(Why::Uncovered);
    }
    let mut budget = MAX_FOOT_BOXES;
    let mut start = [c.u.mid(), c.v.mid()];
    let mut work = vec![t];
    let mut chain: Vec<FootBox> = Vec::new();
    let whole = t;
    let on_border = held_on_a_border(net, line, boxes);
    while let Some(t) = work.pop() {
        if budget == 0 {
            return Typed::Undecided(Why::Budget);
        }
        // (a stretch costs one test, whatever its Newton step does)
        budget = budget.saturating_sub(1);
        // a stretch whose line parameters meet a box on a border has no foot box: halved
        // untested while it can be, the stretch at the floor tested for its reason
        let mid = t.mid();
        if mid > t.lo && mid < t.hi && meets_a_held_border(&on_border, t) {
            work.push(Ivl::new(mid, t.hi));
            work.push(Ivl::new(t.lo, mid));
            continue;
        }
        let failed = match foot_box(net, line, leaves, boxes, t, start, &mut budget) {
            Ok(f) => {
                if let Some(prev) = chain.last() {
                    // (the stretches tile the line parameters left to right)
                    let at = prev.t.hi;
                    // (the two feet at the common `t` are one: each box holds one foot)
                    let joined = tight_foot(net, line, prev.u, at)
                        .is_some_and(|k| within2(&k, &f.u))
                        || tight_foot(net, line, f.u, at).is_some_and(|k| within2(&k, &prev.u));
                    if !(joined && at == t.lo) {
                        return Typed::Undecided(Why::FootNotShown(FootWhy::NotChained));
                    }
                }
                start = f.k.map(Ivl::mid);
                chain.push(f);
                continue;
            }
            Err(why) => why,
        };
        let mid = t.mid();
        if !(mid > t.lo && mid < t.hi) {
            return Typed::Undecided(Why::FootNotShown(FootWhy::Floor(failed)));
        }
        work.push(Ivl::new(mid, t.hi));
        work.push(Ivl::new(t.lo, mid));
    }
    let lo = chain
        .first()
        .and_then(|first| foot_side(net, line, first, whole.lo));
    let hi = chain
        .last()
        .and_then(|last| foot_side(net, line, last, whole.hi));
    let (lo, hi) = match (lo, hi) {
        (Some(lo), Some(hi)) => (lo, hi),
        (None, _) => return Typed::Undecided(Why::FootNotShown(FootWhy::NoSideAtStart)),
        (Some(_), None) => return Typed::Undecided(Why::FootNotShown(FootWhy::NoSideAtEnd)),
    };
    let apart = |s: Ivl| s.lo > 0.0 || s.hi < 0.0;
    if !(apart(lo) && apart(hi)) {
        return Typed::Undecided(Why::EndNotShown);
    }
    c.contact = if (lo.lo > 0.0) == (hi.lo > 0.0) {
        Contact::Touching
    } else {
        Contact::Crossing
    };
    Typed::Foot
}

/// The line parameters of every box among `boxes` (a component's) with a side on the
/// border where neither foot rule's box may reach (module docs, "Typing"): the knots' range
/// met with the solve's `(u, v)` domain, in a direction the solve does not wrap in. A foot
/// box ([`foot_box`]) and a frame box ([`frame_box`]) of a stretch must hold every box
/// whose line parameters meet the stretch and lie strictly inside that border, so a stretch
/// meeting one of these has neither: both tests fail it before any Krawczyk test.
fn held_on_a_border(net: &LineNet, line: &ScratchLine, boxes: &[PlacedBox]) -> Vec<Ivl> {
    let [[u0, u1], [v0, v1]] = net.range;
    let (lo, hi) = (line.solve_domain.min(), line.solve_domain.max());
    let (bu, bv) = ([u0.max(lo.x), u1.min(hi.x)], [v0.max(lo.y), v1.min(hi.y)]);
    let [wu, wv] = net.seams(&line.solve_domain).wrap;
    boxes
        .iter()
        .filter(|b| {
            (!wu && (b.u.lo <= bu[0] || b.u.hi >= bu[1]))
                || (!wv && (b.v.lo <= bv[0] || b.v.hi >= bv[1]))
        })
        .map(|b| b.t)
        .collect()
}

/// Whether the stretch `t` meets the line parameters of a box of [`held_on_a_border`].
fn meets_a_held_border(on_border: &[Ivl], t: Ivl) -> bool {
    on_border.iter().any(|b| b.lo <= t.hi && t.lo <= b.hi)
}

/// The frame of a component for the foot rule along a fixed direction (module docs,
/// "Typing"; answer12, Q2): three vectors fixed before any test and the same for every
/// stretch of the component, `e₁`, `e₂` and `m`, with `det [e₁ e₂ m]` enclosed apart from
/// 0. How well they fit the surface decides only how often the rule succeeds.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    /// `e₁` and `e₂`: the foot of `L(t)` is where the line through it along `e₁ × e₂`
    /// meets the net.
    e: [[f64; 3]; 2],
    /// `m`: the side of the foot is `m · (L(t) − S)`.
    m: [f64; 3],
}

impl Frame {
    /// The frame of the component whose boxes are `boxes` and whose line parameters are
    /// `t`: `m` the rounded middle of the enclosure of `S_u × S_v` over its middle box (the
    /// first whose line parameters' middle is nearest `t`'s), made a unit vector in
    /// rounded arithmetic, `e₁` the axis `m` is least along crossed with `m` and `e₂ = m ×
    /// e₁`, each rounded. It is not taken from a closest-point foot (the step that fails
    /// where this rule runs). `None` where the box is not enclosed, a vector has no length,
    /// or `det [e₁ e₂ m] = (e₁ × e₂) · m` is not enclosed apart from 0.
    fn of(net: &LineNet, line: &ScratchLine, boxes: &[PlacedBox], t: Ivl) -> Option<Self> {
        let at = t.mid();
        let far = |b: &PlacedBox| {
            let d = (b.t.mid() - at).abs();
            if d.is_nan() {
                f64::INFINITY
            } else {
                d
            }
        };
        let middle = boxes.iter().min_by(|a, b| far(a).total_cmp(&far(b)))?;
        let [_, s_u, s_v] = net.over(middle.u, middle.v, line.p)?;
        let unit = |v: [f64; 3]| {
            let [x, y, z] = v;
            let length = crate::geom::vector3d::sqrt(x * x + y * y + z * z);
            (length > 0.0 && length.is_finite()).then(|| v.map(|c| c / length))
        };
        let times = |[ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3]| {
            [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
        };
        let m = unit(mids(&cross_i(&s_u, &s_v)))?;
        let [mx, my, mz] = m.map(f64::abs);
        let axis = if mx <= my && mx <= mz {
            [1.0, 0.0, 0.0]
        } else if my <= mz {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        let e1 = unit(times(axis, m))?;
        let e2 = unit(times(m, e1))?;
        let det = dot(m, &cross(e1, &e2.map(Ivl::from)));
        (det.lo > 0.0 || det.hi < 0.0).then_some(Self { e: [e1, e2], m })
    }
}

/// The offsets `x − at` of the points `x` of the span `x` from its point `at`, measured
/// along the lifted interval (module docs, "The seam"; answer12, Q2, condition 5): an
/// enclosure rounded outward, and its low and high ends rounded inward (the lifted
/// interval's interior holds every offset strictly between them). `at` is a parameter of
/// the net on one piece; on two pieces glued at the seam of the range `[r0, r1]`, a point
/// of the other piece is offset through the seam, `(x − r0) + (r1 − at)` or
/// `−((r1 − x) + (at − r0))`, each difference an interval rounded outward: no parameter is
/// shifted by the range's width. `None` where `at` is on no piece, or the pieces are not a
/// stretch less than a turn wide glued at that seam.
fn offsets_of(x: &Span, at: f64, [r0, r1]: [f64; 2]) -> Option<(Ivl, [f64; 2])> {
    let i = Ivl::from;
    let (lo, hi) = match x.then {
        None => {
            if !(x.first.lo <= at && at <= x.first.hi) {
                return None;
            }
            (i(x.first.lo) - i(at), i(x.first.hi) - i(at))
        }
        Some(then) => {
            if !(x.first.hi == r1 && then.lo == r0 && then.hi < x.first.lo) {
                return None;
            }
            if x.first.lo <= at && at <= r1 {
                (
                    i(x.first.lo) - i(at),
                    (i(then.hi) - i(r0)) + (i(r1) - i(at)),
                )
            } else if r0 <= at && at <= then.hi {
                (
                    i(0.0) - ((i(r1) - i(x.first.lo)) + (i(at) - i(r0))),
                    i(then.hi) - i(at),
                )
            } else {
                return None;
            }
        }
    };
    let outer = Ivl::new(lo.lo, hi.hi);
    (outer.is_finite() && lo.hi <= hi.lo).then_some((outer, [lo.hi, hi.lo]))
}

/// The points of the span `x` at the offsets `off` from its point `at` ([`offsets_of`]'s
/// measure), as pieces in the net's parameters rounded outward and cut to `x`'s pieces: a
/// set holding every such point of `x`. `None` where there is none or `at` is on no piece.
fn at_offsets(x: &Span, at: f64, off: Ivl, [r0, r1]: [f64; 2]) -> Option<Span> {
    let i = Ivl::from;
    let Some(then) = x.then else {
        return meet(i(at) + off, x.first).map(Span::native);
    };
    let (high, low) = if x.first.lo <= at && at <= r1 {
        (i(at) + off, i(r0) + (off - (i(r1) - i(at))))
    } else if r0 <= at && at <= then.hi {
        (i(r1) + (off + (i(at) - i(r0))), i(at) + off)
    } else {
        return None;
    };
    match (meet(high, x.first), meet(low, then)) {
        // (on both pieces the set reaches the seam from each: kept up to it)
        (Some(h), Some(l)) => Some(Span {
            first: Ivl::new(h.lo, x.first.hi),
            then: Some(Ivl::new(then.lo, l.hi)),
        }),
        (Some(h), None) => Some(Span::native(h)),
        (None, Some(l)) => Some(Span::native(l)),
        (None, None) => None,
    }
}

/// Whether every piece of `a` lies in a piece of `b`, ends included, in both directions.
fn spans_within(a: &[Span; 2], b: &[Span; 2]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| x.pieces().all(|p| y.pieces().any(|q| within(p, q))))
}

/// `S − about`, `S_u` and `S_v` over the box `x`, enclosed: the hull over its pieces in
/// the net's parameters ([`LineNet::over`] on each, which crosses a knot line only where the
/// net is continuous). Across a seam the solve wraps, `S` is continuous (the net closes
/// exactly) and the translation between the two ends keeps the partials, so the hull holds,
/// at every point of the lifted box, each one-sided partial there. `None` as
/// [`LineNet::over`] on a piece.
fn over_spans(net: &LineNet, x: &[Span; 2], about: [f64; 3]) -> Option<[I3; 3]> {
    let mut out: Option<[I3; 3]> = None;
    for [u, v] in hull_pieces(x) {
        let p = net.over(u, v, about)?;
        out = Some(match out {
            None => p,
            Some(h) => {
                let mut both = h;
                for (a, b) in both.iter_mut().flatten().zip(p.iter().flatten()) {
                    *a = a.hull(*b);
                }
                both
            }
        });
    }
    out
}

/// Whether a piece of the box `x` crosses a knot line whose multiplicity is above the
/// degree: a break of the net, which no box of the foot rule along a frame crosses.
fn crosses_a_break(net: &LineNet, x: &[Span; 2]) -> bool {
    let broken = |knots: &[f64], degree: usize, x: Ivl| {
        knots.iter().any(|&k| {
            k > x.lo && k < x.hi && knots.iter().filter(|&&other| other == k).count() > degree
        })
    };
    hull_pieces(x)
        .iter()
        .any(|&[u, v]| broken(&net.net.u, net.net.p, u) || broken(&net.net.v, net.net.q, v))
}

/// One stretch `T` of a component's line parameters with its box `U` of the foot rule
/// along a frame (module docs, "Typing"): for every `t` in `T` exactly one foot along the
/// frame in `U`.
#[derive(Debug, Clone, Copy)]
struct FrameBox {
    t: Ivl,
    /// `U`, each direction one interval or two pieces glued at a seam the solve wraps.
    u: [Span; 2],
    /// The point Krawczyk's operator is taken about: Newton's foot at `T`'s middle, a
    /// parameter of the net on one piece of `U` in each direction.
    y: [f64; 2],
}

/// Krawczyk's operator of the foot system along `frame`, `G̃(x, t) = Eᵀ (S(x) − L(t))`,
/// about the point `y` over the points of `u` at the offsets `delta` from `y`, with the
/// line parameters `t` an interval parameter: `K − y = −Y G̃(y, t) + (I − Y J) delta`, the
/// offsets of its points from `y`, and the contraction `q = ‖I − Y J‖`. `J = Eᵀ [S_u S_v]`
/// is enclosed over `u` ([`over_spans`]: first partials only, the hull of both sides'
/// across a knot line or a wrapped seam inside it), and does not depend on `t`; `Y` is
/// the inverse of its middle. `u` must hold `y` and every point at the offsets `delta`.
/// Why not where it cannot be formed.
fn frame_operator(
    net: &LineNet,
    line: &ScratchLine,
    frame: &Frame,
    u: &[Span; 2],
    y: [f64; 2],
    delta: &[Ivl; 2],
    t: Ivl,
) -> core::result::Result<([Ivl; 2], f64), Stretch> {
    let [yu, yv] = y;
    let [e1, e2] = frame.e;
    let [s_y, _, _] = net
        .over(Ivl::from(yu), Ivl::from(yv), line.p)
        .ok_or(Stretch::NotFinite)?;
    let gap = less(&s_y, t, line.d);
    let g = [dot(e1, &gap), dot(e2, &gap)];
    let [_, s_u, s_v] = over_spans(net, u, line.p).ok_or_else(|| {
        if crosses_a_break(net, u) {
            Stretch::Break
        } else {
            Stretch::NotFinite
        }
    })?;
    let jac = [
        [dot(e1, &s_u), dot(e1, &s_v)],
        [dot(e2, &s_u), dot(e2, &s_v)],
    ];
    if !g.iter().chain(jac.iter().flatten()).all(|x| x.is_finite()) {
        return Err(Stretch::NotFinite);
    }
    let [[a, b], [c, d]] = jac;
    let y_inverse =
        inverse_of_columns2(&[[a.mid(), c.mid()], [b.mid(), d.mid()]]).ok_or(Stretch::Singular)?;
    let (cm, q) = contraction(&y_inverse, &jac);
    let zero = [Ivl::from(0.0); 2];
    Ok((
        plus_c_times(start_minus_y(zero, &y_inverse, &g), &cm, delta),
        q,
    ))
}

/// Newton's foot of `L(t)` along `frame` on the net from `start`: the zero of `G̃(x, t) =
/// Eᵀ (S(x) − L(t))`, by first partials only (at a point on a knot line, the middle of the
/// hull of both sides'). A step past an end the solve wraps at (`seams`) goes on from the
/// other end, by a rounded shift: the steps only propose a point, which Krawczyk's operator
/// certifies. Why not where the steps do not settle, or leave the knots' range at another
/// end or settle off it.
fn frame_newton(
    net: &LineNet,
    line: &ScratchLine,
    frame: &Frame,
    seams: &Seams,
    t: f64,
    start: [f64; 2],
) -> core::result::Result<[f64; 2], Stretch> {
    let [ru, rv] = seams.range;
    let [wu, wv] = seams.wrap;
    let turn = |x: f64, [a, z]: [f64; 2], wraps: bool| {
        if wraps && x < a {
            x + (z - a)
        } else if wraps && x > z {
            x - (z - a)
        } else {
            x
        }
    };
    let native = |[u, v]: [f64; 2]| [turn(u, ru, wu), turn(v, rv, wv)];
    let [e1, e2] = frame.e;
    let left = core::cell::Cell::new(false);
    let at = |p: [f64; 2]| -> Result<Linearised<2>> {
        let [u, v] = native(p);
        let (bu, bv) = (Ivl::from(u), Ivl::from(v));
        let Some([s, s_u, s_v]) = net.over(bu, bv, line.p) else {
            if !net.holds(bu, bv) {
                left.set(true);
            }
            return Ok(None);
        };
        let gap = less(&s, Ivl::from(t), line.d);
        let g = [dot(e1, &gap).mid(), dot(e2, &gap).mid()];
        // (columns)
        let cols = [
            [dot(e1, &s_u).mid(), dot(e2, &s_u).mid()],
            [dot(e1, &s_v).mid(), dot(e2, &s_v).mid()],
        ];
        Ok(Some((g, cols)))
    };
    let p = newton(start, &at, &inverse_of_columns2)
        .ok()
        .flatten()
        .ok_or_else(|| {
            if left.get() {
                Stretch::NewtonOff
            } else {
                Stretch::Newton
            }
        })?;
    let [pu, pv] = native(p);
    if net.holds(Ivl::from(pu), Ivl::from(pv)) {
        Ok([pu, pv])
    } else {
        Err(Stretch::NewtonOff)
    }
}

/// The offsets of the box `u` from its point `y` in both directions ([`offsets_of`]): the
/// enclosures, and their ends rounded inward.
fn box_offsets(u: &[Span; 2], y: [f64; 2], seams: &Seams) -> Option<([Ivl; 2], [[f64; 2]; 2])> {
    let [su, sv] = u;
    let [yu, yv] = y;
    let [ru, rv] = seams.range;
    let (du, iu) = offsets_of(su, yu, ru)?;
    let (dv, iv) = offsets_of(sv, yv, rv)?;
    Some(([du, dv], [iu, iv]))
}

/// The points of the box `u` at the offsets `off` from its point `y` in both directions
/// ([`at_offsets`]).
fn box_at_offsets(u: &[Span; 2], y: [f64; 2], off: &[Ivl; 2], seams: &Seams) -> Option<[Span; 2]> {
    let [su, sv] = u;
    let [yu, yv] = y;
    let [ou, ov] = *off;
    let [ru, rv] = seams.range;
    Some([at_offsets(su, yu, ou, ru)?, at_offsets(sv, yv, ov, rv)?])
}

/// The box of the foot rule along `frame` for the stretch `t` of the component whose boxes
/// are `boxes` (module docs, "Typing"; answer12, Q2): a box `U` holding every box of the
/// component whose line parameters meet `t` and Newton's foot `y` at `t`'s middle
/// ([`frame_newton`]), covered by the searched leaves and strips, on which Krawczyk's
/// operator of `G̃` about `y` over `U × t` lies strictly inside `U` with a contraction
/// below 1: each `t` then has exactly one foot along the frame in `U`, and every matrix of
/// `J(U)` is nonsingular. In a direction the solve wraps in ([`LineNet::seams`]), `U` may
/// reach across the seam, as two pieces in the net's parameters glued there, less than a
/// turn wide together; in any other it lies strictly inside the knots' range and the
/// solve's domain. The inclusion is tested on the offsets from `y` ([`offsets_of`]):
/// the operator's, rounded outward, strictly between `U`'s, rounded inward. Why not where
/// none is found within the widenings and the budget (the last test's failure; a
/// contraction not below 1 is not widened on).
#[allow(clippy::too_many_arguments)]
fn frame_box(
    net: &LineNet,
    line: &ScratchLine,
    frame: &Frame,
    seams: &Seams,
    leaves: &[SearchedLeaf],
    boxes: &[PlacedBox],
    t: Ivl,
    start: [f64; 2],
    budget: &mut usize,
) -> core::result::Result<FrameBox, Stretch> {
    let (lo, hi) = (line.solve_domain.min(), line.solve_domain.max());
    let [[u0, u1], [v0, v1]] = seams.range;
    // (where the solve does not wrap: the knots' range met with the solve's domain)
    let border = [[u0.max(lo.x), u1.min(hi.x)], [v0.max(lo.y), v1.min(hi.y)]];
    let y = frame_newton(net, line, frame, seams, t.mid(), start)?;
    let held: Vec<[Ivl; 2]> = boxes
        .iter()
        .filter(|b| b.t.lo <= t.hi && t.lo <= b.t.hi)
        .map(|b| [b.u, b.v])
        .collect();
    let [yu, yv] = y;
    let [ru, rv] = seams.range;
    let [wu, wv] = seams.wrap;
    let [border_u, border_v] = border;
    // a stretch about the foot holding the held boxes in one direction, as an interval of
    // rounded lifted parameters (a held box across a wrapped seam from the foot is taken a
    // range's width nearer: a proposal, the tests below read the pieces it gives)
    let about = |at: f64, [r0, r1]: [f64; 2], wraps: bool, pick: fn(&[Ivl; 2]) -> Ivl| {
        let width = r1 - r0;
        let (mut a, mut z) = (at, at);
        for h in &held {
            let x = pick(h);
            let gap = |shift: f64| (x.lo + shift - at).max(at - (x.hi + shift)).max(0.0);
            let mut shift = 0.0;
            if wraps {
                for other in [width, -width] {
                    if gap(other) < gap(shift) {
                        shift = other;
                    }
                }
            }
            a = a.min(x.lo + shift);
            z = z.max(x.hi + shift);
        }
        [a, z]
    };
    let grow = |[a, z]: [f64; 2]| {
        let r = 0.1 * (z - a) + 1.0e-9 * (1.0 + a.abs().max(z.abs()));
        [round_down(a - r), round_up(z + r)]
    };
    // the pieces of a stretch of rounded lifted parameters: in a direction the solve wraps
    // in, the part past an end of the range comes back from the other end
    let pieces = |[a, z]: [f64; 2],
                  [r0, r1]: [f64; 2],
                  [b0, b1]: [f64; 2],
                  wraps: bool|
     -> core::result::Result<Span, Stretch> {
        if !(a <= z && a.is_finite() && z.is_finite()) {
            return Err(Stretch::NotFinite);
        }
        if !wraps {
            return if a <= b0 || z >= b1 {
                Err(Stretch::LeavesDomain)
            } else {
                Ok(Span::native(Ivl::new(a, z)))
            };
        }
        let span = if a <= r0 && z >= r1 {
            return Err(Stretch::Turn);
        } else if a >= r0 && z <= r1 {
            Span::native(Ivl::new(a, z))
        } else if a < r0 && z <= r1 && z >= r0 {
            Span {
                first: Ivl::new(round_down(r1 - (r0 - a)), r1),
                then: Some(Ivl::new(r0, z)),
            }
        } else if z > r1 && a >= r0 && a <= r1 {
            Span {
                first: Ivl::new(a, r1),
                then: Some(Ivl::new(r0, round_up(r0 + (z - r1)))),
            }
        } else {
            return Err(Stretch::Turn);
        };
        match span.then {
            Some(then) if !(then.hi < span.first.lo) => Err(Stretch::Turn),
            _ => Ok(span),
        }
    };
    let mut lifted = [
        grow(about(yu, ru, wu, |&[u, _]| u)),
        grow(about(yv, rv, wv, |&[_, v]| v)),
    ];
    for _ in 0..FOOT_WIDENINGS {
        if *budget == 0 {
            return Err(Stretch::Budget);
        }
        *budget = budget.saturating_sub(1);
        let [lu, lv] = lifted;
        let u = [pieces(lu, ru, border_u, wu)?, pieces(lv, rv, border_v, wv)?];
        let (delta, inner) = box_offsets(&u, y, seams).ok_or(Stretch::HeldOutside)?;
        let (k, q) = frame_operator(net, line, frame, &u, y, &delta, t)?;
        if !(q < 1.0) {
            return Err(Stretch::NoContraction);
        }
        let inside = k
            .iter()
            .zip(&inner)
            .all(|(x, &[a, z])| x.lo > a && x.hi < z);
        if inside {
            let holds = held.iter().all(|h| {
                h.iter()
                    .zip(&u)
                    .all(|(&x, s)| s.pieces().any(|p| within(x, p)))
            });
            return if !holds {
                Err(Stretch::HeldOutside)
            } else if !hull_pieces(&u)
                .iter()
                .all(|&[pu, pv]| covered_by_leaves(pu, pv, leaves))
            {
                Err(Stretch::Uncovered)
            } else {
                Ok(FrameBox { t, u, y })
            };
        }
        let [ku, kv] = k;
        if !(ku.is_finite() && kv.is_finite()) {
            return Err(Stretch::NotFinite);
        }
        let wider = |[a, z]: [f64; 2], at: f64, k: Ivl| grow([a.min(at + k.lo), z.max(at + k.hi)]);
        lifted = [wider(lu, yu, ku), wider(lv, yv, kv)];
    }
    Err(Stretch::NoContraction)
}

/// The most times the tight box of a foot along a frame is taken again
/// ([`tight_frame_foot`]): across a crease the Jacobian's hull keeps the width of the
/// partials' jump however small the box, so each step narrows the box by the contraction
/// only (a half where one side's partial is three times the other's), not by its square.
const FRAME_TIGHTENINGS: usize = 64;

/// The foot along `frame` at the single line parameter `t` in the box `f` (`t` in `f.t`,
/// so `f.u` holds exactly one), enclosed tightly, as pieces in the net's parameters.
/// Krawczyk's operator is taken about Newton's foot at `t` (`y`, a point of `f.u`; `f.y`
/// where the steps give none in it: any point of the box serves), first over all of `f.u`,
/// then again (at most [`FRAME_TIGHTENINGS`] times, until it no longer narrows) on its meet
/// with the offsets from `y` it was taken on. A set of offsets that holds the foot's has
/// it in its operator, whatever the operator's size: `x* − y = −Y G̃(y, t) + (I − Y M) (x*
/// − y)` for a matrix `M` of the Jacobian's enclosure over the segment from `y` to `x*`,
/// so the Jacobian is enclosed over the points at the offsets and `y` itself (the hull of
/// the offsets and 0). About `y` at the foot the box narrows to rounding even where a
/// crease runs through the foot; about the stretch's own point it would stay as wide as
/// the foot's distance from it. `None` where it cannot be formed.
fn tight_frame_foot(
    net: &LineNet,
    line: &ScratchLine,
    frame: &Frame,
    seams: &Seams,
    f: &FrameBox,
    t: f64,
) -> Option<[Span; 2]> {
    let y = frame_newton(net, line, frame, seams, t, f.y)
        .ok()
        .filter(|y| box_offsets(&f.u, *y, seams).is_some())
        .unwrap_or(f.y);
    let (mut off, _) = box_offsets(&f.u, y, seams)?;
    for step in 0..FRAME_TIGHTENINGS {
        let with_y = off.map(|o| o.hull(Ivl::from(0.0)));
        let narrowed = box_at_offsets(&f.u, y, &with_y, seams).and_then(|over| {
            let (k, _) = frame_operator(net, line, frame, &over, y, &off, Ivl::from(t)).ok()?;
            let [ku, kv] = k;
            let [ou, ov] = off;
            Some([meet(ku, ou)?, meet(kv, ov)?])
        });
        match narrowed {
            Some(next) if next != off => off = next,
            // (not formed over the whole box: no tight box)
            None if step == 0 => return None,
            _ => break,
        }
    }
    box_at_offsets(&f.u, y, &off, seams)
}

/// The side of the foot along `frame` at the line parameter `t` in the box `f` (`t` in
/// `f.t`): `h = m · (L(t) − S)` over the tight box of the foot at `t` alone
/// ([`tight_frame_foot`]), enclosed; `None` where it cannot be formed.
fn frame_side(
    net: &LineNet,
    line: &ScratchLine,
    frame: &Frame,
    seams: &Seams,
    f: &FrameBox,
    t: f64,
) -> Option<Ivl> {
    let x = tight_frame_foot(net, line, frame, seams, f, t)?;
    let [s, _, _] = over_spans(net, &x, line.p)?;
    let gap = less(&s, Ivl::from(t), line.d);
    // (L − S = −gap)
    Some(Ivl::from(0.0) - dot(frame.m, &gap))
}

/// The foot rule along a frame (module docs, "Typing"; answer12, Q2), for a component
/// holding a box left at the floor whose closest-point foot rule ([`by_foot`]) failed at a
/// stretch at the floor, so with no other component's line parameters meeting its own `t`
/// and its hull covered by searched leaves and strips: one frame for the component
/// ([`Frame::of`]), `t` covered by stretches, each with its box ([`frame_box`]), each two
/// neighbours chained (at their common `t`, the tight box of one's foot lies in the other's
/// box), and the side of the foot ([`frame_side`]) proved apart from 0 at `t`'s two ends.
/// The contact (crossing for opposite sides, touching for one), or why not (for a stretch
/// at the floor, why its last box failed).
fn by_frame(
    net: &LineNet,
    line: &ScratchLine,
    leaves: &[SearchedLeaf],
    boxes: &[PlacedBox],
    c: &Component,
    t: Ivl,
) -> core::result::Result<Contact, Why> {
    let seams = net.seams(&line.solve_domain);
    let Some(frame) = Frame::of(net, line, boxes, t) else {
        return Err(Why::FootNotShown(FootWhy::NoFrame));
    };
    let mut budget = MAX_FOOT_BOXES;
    let [hu, hv] = c.hull;
    let mut start = [hu.inner(), hv.inner()];
    let mut work = vec![t];
    let mut chain: Vec<FrameBox> = Vec::new();
    let whole = t;
    let on_border = held_on_a_border(net, line, boxes);
    while let Some(t) = work.pop() {
        if budget == 0 {
            return Err(Why::Budget);
        }
        // (a stretch costs one test, whatever its Newton step does)
        budget = budget.saturating_sub(1);
        // (as in [`by_foot`]: no frame box holds a box on a border it does not wrap at)
        let mid = t.mid();
        if mid > t.lo && mid < t.hi && meets_a_held_border(&on_border, t) {
            work.push(Ivl::new(mid, t.hi));
            work.push(Ivl::new(t.lo, mid));
            continue;
        }
        let found = frame_box(
            net,
            line,
            &frame,
            &seams,
            leaves,
            boxes,
            t,
            start,
            &mut budget,
        );
        let failed = match found {
            Ok(f) => {
                if let Some(prev) = chain.last() {
                    // (the stretches tile the line parameters left to right)
                    let at = prev.t.hi;
                    // (the two feet at the common `t` are one: each box holds one foot)
                    let tight = |of: &FrameBox, within: &FrameBox| {
                        tight_frame_foot(net, line, &frame, &seams, of, at)
                            .is_some_and(|k| spans_within(&k, &within.u))
                    };
                    if !((tight(prev, &f) || tight(&f, prev)) && at == t.lo) {
                        return Err(Why::FootNotShown(FootWhy::NotChained));
                    }
                }
                start = f.y;
                chain.push(f);
                continue;
            }
            Err(why) => why,
        };
        let mid = t.mid();
        if !(mid > t.lo && mid < t.hi) {
            return Err(Why::FootNotShown(FootWhy::Floor(failed)));
        }
        work.push(Ivl::new(mid, t.hi));
        work.push(Ivl::new(t.lo, mid));
    }
    let lo = chain
        .first()
        .and_then(|first| frame_side(net, line, &frame, &seams, first, whole.lo));
    let hi = chain
        .last()
        .and_then(|last| frame_side(net, line, &frame, &seams, last, whole.hi));
    let (lo, hi) = match (lo, hi) {
        (Some(lo), Some(hi)) => (lo, hi),
        (None, _) => return Err(Why::FootNotShown(FootWhy::NoSideAtStart)),
        (Some(_), None) => return Err(Why::FootNotShown(FootWhy::NoSideAtEnd)),
    };
    let apart = |s: Ivl| s.lo > 0.0 || s.hi < 0.0;
    if !(apart(lo) && apart(hi)) {
        return Err(Why::EndNotShown);
    }
    Ok(if (lo.lo > 0.0) == (hi.lo > 0.0) {
        Contact::Touching
    } else {
        Contact::Crossing
    })
}

/// The certified bound `ε` on the side function over the net above the `(u, v)` hull
/// `[u, v]` (module docs, "Typing"): the largest bound on `|F|` of the Bézier patches the
/// hull meets ([`side_bounds`]; a hull on a patch's edge reads the patch it lies in) over
/// a lower bound of `F`'s other factor there: the radius for the sphere, `r (R² − r²)` for
/// the torus, and for the cone `R √(1 + s²)` with `R` the radius of the closed form at the
/// heights of the net over the hull, enclosed, which must be positive (the nappe the side
/// function describes; otherwise [`Why::Apex`]). [`Why::NotTheNet`] where a patch the hull
/// meets has no bound, or it meets none.
fn side_eps(
    net: &LineNet,
    rev: &Revolved,
    bounds: &[SideBound],
    u: Ivl,
    v: Ivl,
) -> core::result::Result<f64, Why> {
    let meets = |x: Ivl, [a, z]: [f64; 2]| {
        if x.lo < x.hi {
            x.lo < z && a < x.hi
        } else {
            x.lo <= z && a <= x.hi
        }
    };
    let mut residual: Option<f64> = None;
    for b in bounds {
        let [su, sv] = b.span;
        if meets(u, su) && meets(v, sv) {
            let r = b.residual.ok_or(Why::NotTheNet)?;
            residual = Some(residual.map_or(r, |m: f64| m.max(r)));
        }
    }
    let residual = residual.ok_or(Why::NotTheNet)?;
    let factor = match rev.shape {
        Shape::Sphere { radius } => Ivl::from(radius),
        Shape::Torus { major, minor } => {
            let (big, small) = (Ivl::from(major), Ivl::from(minor));
            small * (big * big - small * small)
        }
        Shape::Cone { radius, slope } => {
            let w = net.value_over(u, v, rev.origin).ok_or(Why::NotTheNet)?;
            let s = Ivl::from(slope);
            let big_r = Ivl::from(radius) + s * rev.coordinates(&w).1;
            if !(big_r.lo > 0.0) {
                return Err(Why::Apex);
            }
            Ivl::new(big_r.lo, big_r.lo) * (Ivl::from(1.0) + s * s).sqrt()
        }
    };
    if !(factor.lo > 0.0 && factor.is_finite()) {
        return Err(Why::NotTheNet);
    }
    let eps = (Ivl::from(residual) / Ivl::new(factor.lo, factor.lo)).hi;
    if eps.is_finite() {
        Ok(eps)
    } else {
        Err(Why::NotTheNet)
    }
}

/// Whether the net over the `(u, v)` hull `[u, v]`, within `eps` of the complete surface,
/// lies off its axis and off its profile circle's centre, where the chart's coordinates are
/// smooth (module docs, "Typing"): for the cone, its radius `ρ ≥ R − ε √(1 + s²) > 0` with
/// `R` enclosed over the hull; for the torus, `R − r − ε > 0` and `r − ε > 0`; for the
/// sphere, `r − ε > 0` and `ρ² ≥ λ (r − ε)² − h² > 0`, `h` enclosed over the hull and `λ`
/// the least eigenvalue bound of the stored frame's Gram matrix ([`Revolved::of`]).
fn in_the_chart_domain(net: &LineNet, rev: &Revolved, u: Ivl, v: Ivl, eps: f64) -> bool {
    let e = Ivl::from(eps);
    match rev.shape {
        Shape::Torus { major, minor } => {
            let (big, small) = (Ivl::from(major), Ivl::from(minor));
            (big - small - e).lo > 0.0 && (small - e).lo > 0.0
        }
        Shape::Cone { radius, slope } => net.value_over(u, v, rev.origin).is_some_and(|w| {
            let s = Ivl::from(slope);
            let big_r = Ivl::from(radius) + s * rev.coordinates(&w).1;
            (big_r - e * (Ivl::from(1.0) + s * s).sqrt()).lo > 0.0
        }),
        Shape::Sphere { radius } => net.value_over(u, v, rev.origin).is_some_and(|w| {
            let off = Ivl::from(radius) - e;
            let h = rev.coordinates(&w).1;
            off.lo > 0.0 && (Ivl::from(GRAM_FLOOR) * off * off - h * h).lo > 0.0
        }),
    }
}

/// The net's knots strictly inside `[a, z]` in u (`in_u`) or v, each once.
fn knots_inside(net: &LineNet, in_u: bool, [a, z]: [f64; 2]) -> Vec<f64> {
    let knots = if in_u { &net.net.u } else { &net.net.v };
    let mut out: Vec<f64> = knots.iter().copied().filter(|&k| k > a && k < z).collect();
    out.dedup();
    out
}

/// The most leaves and strips the coverage test of a hull reads; past it the hull is not
/// shown covered.
const MAX_COVER_RECTS: usize = 64;

/// Whether the `(u, v)` box `[u, v]` is covered by the domains of the searched leaves and
/// strips `leaves` (no trim cull inside it): each cell of the grid of their sides inside
/// the box lies in one of them.
fn covered_by_leaves(u: Ivl, v: Ivl, leaves: &[SearchedLeaf]) -> bool {
    let rects: Vec<[[f64; 2]; 2]> = leaves
        .iter()
        .map(|l| bounds_of(&l.domain))
        .filter_map(|[[a0, a1], [c0, c1]]| {
            let (x0, x1, y0, y1) = (a0.max(u.lo), a1.min(u.hi), c0.max(v.lo), c1.min(v.hi));
            (x0 < x1 && y0 < y1).then_some([[x0, x1], [y0, y1]])
        })
        .collect();
    if rects.len() > MAX_COVER_RECTS || !(u.lo < u.hi && v.lo < v.hi) {
        return false;
    }
    let grid = |pick: fn(&[[f64; 2]; 2]) -> [f64; 2], lo: f64, hi: f64| {
        let mut xs: Vec<f64> = vec![lo, hi];
        xs.extend(rects.iter().flat_map(pick));
        xs.sort_by(f64::total_cmp);
        xs.dedup();
        xs
    };
    let xs = grid(|r| r[0], u.lo, u.hi);
    let ys = grid(|r| r[1], v.lo, v.hi);
    // each cell by both its bounds, never a point sampled in it: a cell between two
    // adjacent doubles has a middle that rounds onto one of them, which a rectangle ending
    // there holds although the open cell between them was not searched. Every rectangle's
    // sides are grid lines, so one holds the whole closed cell exactly when it holds both
    // its corners.
    let cells = |zs: &[f64]| -> Vec<[f64; 2]> {
        zs.windows(2)
            .filter_map(|w| match *w {
                [a, b] => Some([a, b]),
                _ => None,
            })
            .collect()
    };
    let (cxs, cys) = (cells(&xs), cells(&ys));
    cxs.iter().all(|&[x0, x1]| {
        cys.iter().all(|&[y0, y1]| {
            rects
                .iter()
                .any(|&[[a0, a1], [c0, c1]]| a0 <= x0 && x1 <= a1 && c0 <= y0 && y1 <= c1)
        })
    })
}

/// What [`type_solutions`] gives a solution: its contact, the unique box of the certified
/// root it stands for, and the id of the one contact it and another solution both stand for
/// (module docs, "The seam pair": the two points of a pair, numbered from 1 in the order
/// the components are named).
pub(crate) type Typing = (Contact, Option<[Ivl; 2]>, Option<ContactId>);

/// A solution of the solve as [`type_solutions`] reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SolutionAt {
    /// A point, `(t, u, v)` in the solve's line parameter.
    Point([f64; 3]),
    /// A range of values: undecided once typing runs.
    Range,
    /// Any other kind: not typed.
    Other,
}

/// How [`type_solutions`] placed a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placing {
    /// Undecided whatever its component (its gap not shown, in no component, or in the
    /// unique boxes of two roots not shown one, or of a root of another component).
    Undecided,
    /// In the unique box of the root `class` of the component `comp`.
    Carrier { comp: usize, class: usize },
    /// In a box of the component `comp`, in no root's unique box.
    Other { comp: usize },
}

/// The contacts of the solve's solutions `at` (module docs, "Typing"), from its typed
/// components `comps`, with `unplaced` a cull whose strip could not be placed: for each
/// solution, its contact and the unique box of the certified root it stands for (`None`
/// to leave it as it is: a kind not typed); and for each component whether its solutions
/// stand for it.
///
/// Every point and range is undecided where the components were not built or a cull was
/// not placed. A range is undecided. A point is undecided unless its gap to the line is
/// enclosed below the tolerance and it lies in a box of a decided component (the boxes of
/// two components never both hold one point). In it, a point in the unique box of one of
/// its distinct roots carries that root, a transversal crossing (a crossing, when it is the
/// root's only carrier; two carriers of one root are undecided). The component's parity
/// less its roots' leaves what its boxes left at the floor hold: with every root carried
/// once, the one other point is touching when that is even and a crossing when it is odd;
/// two other points that are one seam location (module docs, "The seam pair") are one
/// slot, both crossing or both touching, and share an id; otherwise (two slots or more,
/// whose parity cannot tell two crossings from two touching points) they are undecided.
/// A component typed by its sides
/// whose leaves are not all inside the face may straddle its border, so its other points
/// are undecided. A component stands for itself when every root is carried once and its
/// parity is accounted for (a touching one with no point does).
pub(crate) fn type_solutions(
    net: &LineNet,
    line: &PassLine,
    comps: &Components,
    at: &[SolutionAt],
    unplaced: bool,
) -> (Vec<Option<Typing>>, Vec<bool>) {
    let mut stood = vec![false; comps.list.len()];
    let undecided = Some((Contact::Undecided, None, None));
    let mut pairs = 0_u32;
    if comps.over_budget || unplaced {
        let out = at
            .iter()
            .map(|a| (!matches!(a, SolutionAt::Other)).then_some((Contact::Undecided, None, None)))
            .collect();
        return (out, stood);
    }
    let holds =
        |[qu, qv]: [Ivl; 2], u: f64, v: f64| qu.lo <= u && u <= qu.hi && qv.lo <= v && v <= qv.hi;
    let placing: Vec<Option<Placing>> = at
        .iter()
        .map(|a| match *a {
            SolutionAt::Other => None,
            SolutionAt::Range => Some(Placing::Undecided),
            SolutionAt::Point(p) => Some(place(net, line, comps, p, holds)),
        })
        .collect();
    let mut out: Vec<Option<Typing>> = placing
        .iter()
        .map(|p| p.map(|_| (Contact::Undecided, None, None)))
        .collect();
    for (c, comp) in comps.list.iter().enumerate() {
        let decided = matches!(comp.contact, Contact::Crossing | Contact::Touching)
            && matches!(
                comp.typed,
                Typed::Count(_) | Typed::Sides | Typed::Foot | Typed::FrameFoot
            );
        if !decided {
            continue;
        }
        let mut carriers: Vec<Vec<usize>> = vec![Vec::new(); comp.classes.len()];
        let mut others: Vec<usize> = Vec::new();
        for (i, p) in placing.iter().enumerate() {
            match *p {
                Some(Placing::Carrier { comp: pc, class }) if pc == c => {
                    if let Some(list) = carriers.get_mut(class) {
                        list.push(i);
                    }
                }
                Some(Placing::Other { comp: pc }) if pc == c => others.push(i),
                _ => {}
            }
        }
        for (class, list) in carriers.iter().enumerate() {
            if let ([one], Some(boxes)) = (list.as_slice(), comp.classes.get(class)) {
                let unique = match at.get(*one) {
                    Some(&SolutionAt::Point([_, u, v])) => {
                        boxes.iter().copied().find(|&q| holds(q, u, v))
                    }
                    _ => None,
                };
                if let Some(slot) = out.get_mut(*one) {
                    *slot = Some((Contact::Crossing, unique, None));
                }
            }
        }
        let carried = carriers.iter().all(|l| l.len() == 1);
        let straddles =
            matches!(comp.typed, Typed::Sides | Typed::Foot | Typed::FrameFoot) && !comp.inside;
        // (what the boxes left at the floor hold, beyond the roots, as a parity)
        let parity = usize::from(comp.contact == Contact::Crossing);
        let rest = (parity + comp.classes.len()) % 2;
        // the seam pair (module docs, "The seam pair"): exactly two other points, one seam
        // location reported on each end of the range, one slot of the rest
        // (its id made here: past `u32::MAX` pairs there is none, and the two points stay
        // two slots, never two crossings that lost their link)
        let pair = match (carried && !straddles, others.as_slice()) {
            (true, &[i, j]) => match (at.get(i), at.get(j)) {
                (Some(&SolutionAt::Point(p)), Some(&SolutionAt::Point(q)))
                    if seam_pair(net, line, comps, comp, [p, q]) =>
                {
                    pairs
                        .checked_add(1)
                        .and_then(core::num::NonZeroU32::new)
                        .map(ContactId)
                }
                _ => None,
            },
            _ => None,
        };
        let slots = if pair.is_some() { 1 } else { others.len() };
        // (one slot at most: the parity of two says nothing of each, two crossings and two
        // touching points both even; with none the component stands for itself)
        let other = match (carried && !straddles, rest, slots) {
            (true, 0, 0 | 1) => Some(Contact::Touching),
            (true, 1, 1) => Some(Contact::Crossing),
            _ => None,
        };
        if let Some(contact) = other {
            if let Some(ContactId(id)) = pair {
                pairs = id.get();
            }
            for &i in &others {
                if let Some(slot) = out.get_mut(i) {
                    *slot = Some((contact, None, pair));
                }
            }
            if let Some(slot) = stood.get_mut(c) {
                *slot = true;
            }
        } else {
            for &i in &others {
                if let Some(slot) = out.get_mut(i) {
                    *slot = undecided;
                }
            }
        }
    }
    (out, stood)
}

/// Where the point `(t, u, v)` lies among the typed components `comps` ([`type_solutions`]).
fn place(
    net: &LineNet,
    line: &PassLine,
    comps: &Components,
    [t, u, v]: [f64; 3],
    holds: impl Fn([Ivl; 2], f64, f64) -> bool,
) -> Placing {
    let shown = ScratchLine::of(line, &Extent1d::from_min_max(t, t)).is_some_and(|scratch| {
        net.at(u, v, scratch.p).is_some_and(|s| {
            let gap2 = length_squared(&less(&s, Ivl::from(t), scratch.d));
            gap2.hi < (Ivl::from(scratch.tol) * Ivl::from(scratch.tol)).lo
        })
    });
    let [s0, s1] = line.span;
    let in_span = t >= s0 && t <= s1;
    if !(shown && in_span) {
        return Placing::Undecided;
    }
    let past_root = comps
        .list
        .iter()
        .flat_map(|x| x.beyond.iter().flatten())
        .any(|&q| holds(q, u, v));
    if past_root {
        return Placing::Undecided;
    }
    let held = comps.boxes.iter().position(|b| {
        b.u.lo <= u && u <= b.u.hi && b.v.lo <= v && v <= b.v.hi && b.t.lo <= t && t <= b.t.hi
    });
    let Some(comp) = held.and_then(|i| {
        comps
            .list
            .iter()
            .position(|c| c.boxes.binary_search(&i).is_ok())
    }) else {
        return Placing::Undecided;
    };
    // the roots whose unique boxes hold the point, over every component
    let mut roots = comps.list.iter().enumerate().flat_map(|(c, x)| {
        x.classes
            .iter()
            .enumerate()
            .filter(|(_, boxes)| boxes.iter().any(|&q| holds(q, u, v)))
            .map(move |(k, _)| (c, k))
    });
    match (roots.next(), roots.next()) {
        (None, _) => Placing::Other { comp },
        (Some((c, class)), None) if c == comp => Placing::Carrier { comp, class },
        _ => Placing::Undecided,
    }
}

/// Whether the two points `p` and `q` (`(t, u, v)`, each placed as another point of the
/// component `comp`: its gap shown below the tolerance, in a box of it, in no root's unique
/// box) are one seam location (module docs, "The seam pair"; answer11, Q1): in a direction
/// the solve wraps in and `comp` is joined across, one lies in a box of `comp` reaching the
/// knots' range's low end and the other in one reaching its high end, those two boxes are
/// joined directly by the seam rule of [`components`] (their stretches along the seam meet,
/// or meet at a corner across the other seam, and their line parameters overlap), and both
/// points lie where the two boxes meet: their line parameters in the meet of the two boxes'
/// line parameters, and their coordinates along the seam in the meet of the two boxes'
/// stretches along it (at a corner, the corner itself: each on its own box's end of the
/// other direction's range). A link only through the rest of the component is not one.
fn seam_pair(
    net: &LineNet,
    line: &PassLine,
    comps: &Components,
    comp: &Component,
    [p, q]: [[f64; 3]; 2],
) -> bool {
    let seams = net.seams(&line.solve_domain);
    let holds = |b: &PlacedBox, [t, u, v]: [f64; 3]| {
        b.u.lo <= u && u <= b.u.hi && b.v.lo <= v && v <= b.v.hi && b.t.lo <= t && t <= b.t.hi
    };
    let along = |b: &PlacedBox, d: usize| if d == 0 { b.u } else { b.v };
    let at = |[_, u, v]: [f64; 3], d: usize| if d == 0 { u } else { v };
    let boxes: Vec<&PlacedBox> = comp
        .boxes
        .iter()
        .filter_map(|&i| comps.boxes.get(i))
        .collect();
    for d in 0..2 {
        let (Some(&true), Some(&[e0, e1]), Some(span)) =
            (seams.wrap.get(d), seams.range.get(d), comp.hull.get(d))
        else {
            continue;
        };
        if span.then.is_none() {
            continue;
        }
        let other = 1 - d;
        let wrap_other = seams.wrap.get(other).copied().unwrap_or(false);
        let [f0, f1] = seams.range.get(other).copied().unwrap_or([f64::NAN; 2]);
        for (lo_pt, hi_pt) in [(p, q), (q, p)] {
            let lows = boxes
                .iter()
                .filter(|b| along(b, d).lo <= e0 && holds(b, lo_pt));
            for a in lows {
                let highs = boxes
                    .iter()
                    .filter(|b| along(b, d).hi >= e1 && holds(b, hi_pt));
                for b in highs {
                    let Some(t) = meet(a.t, b.t) else {
                        continue;
                    };
                    let in_t = [lo_pt, hi_pt].iter().all(|&[pt, _, _]| within_point(pt, t));
                    let (x, y) = (along(a, other), along(b, other));
                    let (lo_s, hi_s) = (at(lo_pt, other), at(hi_pt, other));
                    let direct =
                        meet(x, y).is_some_and(|m| within_point(lo_s, m) && within_point(hi_s, m));
                    let corner = wrap_other
                        && ((x.lo <= f0 && y.hi >= f1 && lo_s <= f0 && hi_s >= f1)
                            || (y.lo <= f0 && x.hi >= f1 && hi_s <= f0 && lo_s >= f1));
                    if in_t && (direct || corner) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Whether `x` lies in the closed interval `i`.
fn within_point(x: f64, i: Ivl) -> bool {
    i.lo <= x && x <= i.hi
}

/// What a box of the work list becomes; each verdict but the first and the halving carries
/// the box's line parameters (the feet of its points widened by the tolerance).
enum Visit {
    /// Proved farther than the tolerance from the line (step 1).
    ExcludedByDistance,
    /// Its line parameters wholly outside the line's own interval (step 1).
    ExcludedByT(Ivl),
    /// Regular, and Krawczyk's test proves it holds no root (step 2).
    NoRoot(Ivl),
    /// Regular, with its one root, certified and shown.
    Root(CertifiedRoot, Ivl),
    /// Regular, with its one root certified but not reported ([`Isolation::unshown`]), and
    /// the `(u, v)` box that root is the only one in.
    Unshown([Ivl; 2], I3, Ivl),
    /// At the floor (step 3), with its line parameters where it was enclosed.
    Left(Option<Ivl>),
    /// Its two halves, the first to visit first, with the box's line parameters where it was
    /// enclosed (the order [`isolate`] visits boxes in).
    Halve(Scratch, Scratch, Option<Ivl>),
}

/// A box enclosed about the surface's point at its middle: the quotient's terms are then
/// of the size of the box, not of its distance from the origin.
struct Enclosed {
    /// `S − P` at the box's middle, enclosed.
    s_mid: I3,
    /// The point the box is enclosed about.
    about: [f64; 3],
    /// `S − P` over the box.
    r: I3,
    /// `S_u` over the box.
    s_u: I3,
    /// `S_v` over the box.
    s_v: I3,
}

/// The box `[u, v]` enclosed; `None` where the net refuses it (it leaves the knots'
/// range, or weights are not proved positive over it).
fn enclosed(net: &LineNet, line: &ScratchLine, u: Ivl, v: Ivl) -> Option<Enclosed> {
    let s_mid = net.at(u.mid(), v.mid(), line.p)?;
    let [m0, m1, m2] = mids(&s_mid);
    let [p0, p1, p2] = line.p;
    let about = [p0 + m0, p1 + m1, p2 + m2];
    let [s, s_u, s_v] = net.over(u, v, about)?;
    let r = line.off_the_start(&s, about);
    Some(Enclosed {
        s_mid,
        about,
        r,
        s_u,
        s_v,
    })
}

/// `S − P` over the box `[u, v]`, enclosed about the surface's point at its middle as
/// [`enclosed`] encloses it, without the partials (what step 1 reads); `None` where the net
/// refuses it.
fn enclosed_value(net: &LineNet, line: &ScratchLine, u: Ivl, v: Ivl) -> Option<I3> {
    let s_mid = net.at(u.mid(), v.mid(), line.p)?;
    let [m0, m1, m2] = mids(&s_mid);
    let [p0, p1, p2] = line.p;
    let about = [p0 + m0, p1 + m1, p2 + m2];
    let s = net.value_over(u, v, about)?;
    Some(line.off_the_start(&s, about))
}

/// How the floor of step 3 sees a box: whether each direction is still cut (its image
/// along it longer than half the tolerance), and which one step 4 halves.
#[derive(Clone, Copy)]
struct Cuts {
    cut_u: bool,
    cut_v: bool,
    split_u: bool,
}

/// [`Cuts`] of the box `b` from its partials; `None` where the sizes are not finite.
fn cuts(line: &ScratchLine, b: Scratch, s_u: &I3, s_v: &I3) -> Option<Cuts> {
    let (du, dv) = (b.u.hi - b.u.lo, b.v.hi - b.v.lo);
    let (along_u, along_v) = (
        round_up(length_squared(s_u).hi.sqrt() * du),
        round_up(length_squared(s_v).hi.sqrt() * dv),
    );
    // how far each direction carries the surface from the line over the box, times |D|
    let (off_u, off_v) = (
        length_squared(&cross(line.d, s_u)).hi.sqrt() * du,
        length_squared(&cross(line.d, s_v)).hi.sqrt() * dv,
    );
    if !(along_u.is_finite() && along_v.is_finite() && off_u.is_finite() && off_v.is_finite()) {
        return None;
    }
    // a direction is cut while the box's image along it is longer than half the tolerance
    let (cut_u, cut_v) = (
        round_up(along_u + along_u) > line.tol,
        round_up(along_v + along_v) > line.tol,
    );
    // step 4: across the direction that carries the surface farther from the line (the
    // longer one where neither does: a box the line runs along in both)
    let split_u = if cut_u != cut_v {
        cut_u
    } else if off_u > 0.0 || off_v > 0.0 {
        off_u >= off_v
    } else {
        along_u >= along_v
    };
    Some(Cuts {
        cut_u,
        cut_v,
        split_u,
    })
}

/// One box (module docs, steps 1 to 4).
fn visit(net: &LineNet, line: &ScratchLine, b: Scratch) -> Visit {
    // 0. one polynomial piece a box
    if let Some((in_u, knot)) = net.knot_inside(b.u, b.v) {
        return if in_u {
            Visit::Halve(
                Scratch {
                    u: Ivl::new(b.u.lo, knot),
                    ..b
                },
                Scratch {
                    u: Ivl::new(knot, b.u.hi),
                    ..b
                },
                None,
            )
        } else {
            Visit::Halve(
                Scratch {
                    v: Ivl::new(b.v.lo, knot),
                    ..b
                },
                Scratch {
                    v: Ivl::new(knot, b.v.hi),
                    ..b
                },
                None,
            )
        };
    }
    let (um, vm) = (b.u.mid(), b.v.mid());
    // a box that cannot be enclosed is halved across its wider side (left where it cannot)
    let Some(Enclosed {
        s_mid,
        about,
        r,
        s_u,
        s_v,
    }) = enclosed(net, line, b.u, b.v)
    else {
        let split_u = b.u.hi - b.u.lo >= b.v.hi - b.v.lo;
        return halves(b, split_u, if split_u { um } else { vm }, None);
    };

    // 1. farther than the tolerance from the line everywhere, or off the line's own interval
    if line.farther(&r) {
        return Visit::ExcludedByDistance;
    }
    // (Krawczyk's steps start from `t`; the verdicts read its enclosure `feet`)
    let (t, feet) = line.feet(&r);
    if !feet.is_finite() {
        // nothing is proved of a box whose line parameters cannot be enclosed
        return Visit::Left(None);
    }
    let [s0, s1] = line.span;
    if feet.hi < s0 || feet.lo > s1 {
        return Visit::ExcludedByT(feet);
    }

    // 2. a regular interval Jacobian: at most one root, and Krawczyk's test for it
    if let Some(found) = regular(net, line, b, &s_mid, [&s_u, &s_v], t) {
        match found {
            Regular::Empty => return Visit::NoRoot(feet),
            Regular::Root(enclosure, unique) => {
                return match refined(net, line, enclosure, unique) {
                    Some(root) => Visit::Root(root, feet),
                    None => Visit::Unshown(unique, enclosure, feet),
                };
            }
            Regular::Open => {}
        }
    }

    // 3. the floor: no larger than the tolerance, the line running along the surface, or
    // not to be halved
    let Some(c) = cuts(line, b, &s_u, &s_v) else {
        return Visit::Left(Some(feet));
    };
    if !c.cut_u && !c.cut_v {
        return Visit::Left(Some(feet));
    }
    if let Some(m) = in_a_plane_through_the_line(line, &r, t, [&s_u, &s_v]) {
        if fold_is_a_point(net, line, b, about, m, [&s_u, &s_v]) == Some(false) {
            return Visit::Left(Some(feet));
        }
    }

    // 4. halve across the direction that carries the surface farther from the line
    halves(b, c.split_u, if c.split_u { um } else { vm }, Some(feet))
}

/// The two halves of `b` cut at `mid` in u (`split_u`) or in v; the box is left where
/// `mid` is not strictly inside it (a box one rounding wide), with its line parameters `t`
/// where it was enclosed.
fn halves(b: Scratch, split_u: bool, mid: f64, t: Option<Ivl>) -> Visit {
    let (lo, hi) = if split_u {
        (b.u.lo, b.u.hi)
    } else {
        (b.v.lo, b.v.hi)
    };
    if !(mid > lo && mid < hi) {
        return Visit::Left(t);
    }
    let (first, second) = (Ivl::new(lo, mid), Ivl::new(mid, hi));
    if split_u {
        Visit::Halve(Scratch { u: first, ..b }, Scratch { u: second, ..b }, t)
    } else {
        Visit::Halve(Scratch { v: first, ..b }, Scratch { v: second, ..b }, t)
    }
}

/// The unit normal `m` of a plane through the line that the surface over a box lies within
/// the tolerance of: the surface normal at the box's middle made perpendicular to the line
/// (`r` the box's `S − P`, `t` its line parameters). `None` where the surface does not lie
/// that close to that plane, or that normal has no part perpendicular to the line.
fn in_a_plane_through_the_line(
    line: &ScratchLine,
    r: &I3,
    t: Ivl,
    partials: [&I3; 2],
) -> Option<[f64; 3]> {
    let [s_u, s_v] = partials;
    let n = v3(mids(s_u)).cross(v3(mids(s_v)));
    let dv = v3(line.d);
    let dd = dv.dot(dv);
    let across = n - dv * (n.dot(dv) / dd);
    let len = across.length();
    if !(len.is_finite() && len > 0.0 && dd.is_finite() && dd > 0.0) {
        return None;
    }
    let m = f3(across * (1.0 / len));
    // |m| is 1 to rounding, and m · D is 0 to rounding: both enter the bound as they are
    let off = dot(m, r) - t * dot(m, &line.d.map(Ivl::from));
    let unit = round_down(length_squared(&m.map(Ivl::from)).lo.sqrt());
    (unit > 0.0 && round_up(off.mag() / unit) <= line.tol).then_some(m)
}

/// Whether the line's fold on the surface is a point over the box (module docs, step 3):
/// with `f₂ = m · (S − P)` and `f₁ = n · (S − P)`, `n` the unit vector across the line and
/// `m`, the system `G = (f₁, f₁ᵤ f₂ᵥ − f₁ᵥ f₂ᵤ)` has a regular interval Jacobian over the
/// box (the contraction of Krawczyk's operator below 1), so `G` is injective there.
/// `Some(false)` where the system is enclosed and that is not so: the box is then one the
/// line runs along. `None` where the system cannot be enclosed over the box (it crosses a
/// knot line the surface is not `C¹` across): nothing is known of the fold there.
fn fold_is_a_point(
    net: &LineNet,
    line: &ScratchLine,
    b: Scratch,
    about: [f64; 3],
    m: [f64; 3],
    partials: [&I3; 2],
) -> Option<bool> {
    let n = v3(line.d).cross(v3(m));
    let len = n.length();
    if !(len.is_finite() && len > 0.0) {
        return None;
    }
    let n = f3(n * (1.0 / len));
    let [s_uu, s_uv, s_vv] = net.second(b.u, b.v, about)?;
    let [s_u, s_v] = partials;
    // the first and second partials of f₁ (along n) and f₂ (along m)
    let (f1u, f1v, f2u, f2v) = (dot(n, s_u), dot(n, s_v), dot(m, s_u), dot(m, s_v));
    let (f1uu, f1uv, f1vv) = (dot(n, &s_uu), dot(n, &s_uv), dot(n, &s_vv));
    let (f2uu, f2uv, f2vv) = (dot(m, &s_uu), dot(m, &s_uv), dot(m, &s_vv));
    let det_u = f1uu * f2v + f1u * f2uv - f1uv * f2u - f1v * f2uu;
    let det_v = f1uv * f2v + f1u * f2vv - f1vv * f2u - f1v * f2uv;
    let jac = [[f1u, f1v], [det_u, det_v]];
    let Some(y) = inverse_of_columns2(&[[f1u.mid(), det_u.mid()], [f1v.mid(), det_v.mid()]]) else {
        // the middle of the fold's Jacobian is singular to rounding
        return Some(false);
    };
    let (_, q) = contraction(&y, &jac);
    Some(q < 1.0)
}

/// Krawczyk's test on a box whose interval Jacobian is regular; `None` where it is not
/// (or no preconditioner exists).
fn regular(
    net: &LineNet,
    line: &ScratchLine,
    b: Scratch,
    s_mid: &I3,
    partials: [&I3; 2],
    t: Ivl,
) -> Option<Regular> {
    let [s_u, s_v] = partials;
    let minus_d = line.d.map(|x| -x);
    let y = inverse_of_columns3(&[mids(s_u), mids(s_v), minus_d])?;
    let jac: [[Ivl; 3]; 3] =
        core::array::from_fn(|k| match (s_u.get(k), s_v.get(k), minus_d.get(k)) {
            (Some(&a), Some(&c), Some(&e)) => [a, c, Ivl::from(e)],
            _ => [Ivl::whole(); 3],
        });
    let (c, q) = contraction(&y, &jac);
    if !(q < 1.0) {
        return None;
    }
    // every root of the box lies in K(X) ∩ X, for each X the steps reach
    let mut x: I3 = [b.u, b.v, t];
    let mut at_mid = *s_mid;
    for _ in 0..KRAWCZYK_STEPS {
        let mid = mids(&x);
        let [_, _, tm] = mid;
        let g = less(&at_mid, Ivl::from(tm), line.d);
        let k = k_operator(&mid, &y, &g, &c, &x);
        if apart(&k, &x) {
            return Some(Regular::Empty);
        }
        if inside(&k, &x) {
            return Some(Regular::Root(k, [b.u, b.v]));
        }
        let [x0, x1, x2] = x;
        let [k0, k1, k2] = k;
        let (Some(n0), Some(n1), Some(n2)) = (meet(x0, k0), meet(x1, k1), meet(x2, k2)) else {
            return Some(Regular::Empty);
        };
        let next = [n0, n1, n2];
        if next == x {
            break;
        }
        x = next;
        let Some(s) = net.at(n0.mid(), n1.mid(), line.p) else {
            return Some(Regular::Open);
        };
        at_mid = s;
    }
    Some(on_the_border(net, line, b, &x, &y))
}

/// A regular box whose root, if it has one, sits on its border (`x` is what Krawczyk's
/// steps left of the box): the root certified in a small box about `x`'s middle, and the
/// hull of that box and the whole box proved regular too. The hull then holds no root but
/// that one, so the box holds it or none.
fn on_the_border(
    net: &LineNet,
    line: &ScratchLine,
    b: Scratch,
    x: &I3,
    y: &[[f64; 3]; 3],
) -> Regular {
    let value = |p: [Ivl; 3]| -> Result<Option<[Ivl; 3]>> {
        let [u, v, t] = p;
        Ok(net.over(u, v, line.p).map(|[s, _, _]| less(&s, t, line.d)))
    };
    let jacobian = |p: [Ivl; 3]| -> Result<Option<[[Ivl; 3]; 3]>> {
        let [u, v, _] = p;
        Ok(net
            .over(u, v, line.p)
            .map(|[_, s_u, s_v]| [s_u, s_v, line.d.map(|e| Ivl::from(-e))]))
    };
    let Ok(Some((root, _))) = krawczyk_region(mids(x), *y, &value, &jacobian) else {
        return Regular::Open;
    };
    let [ru, rv, _] = root;
    let (hu, hv) = (b.u.hull(ru), b.v.hull(rv));
    if !regular_over(net, line, hu, hv) {
        return Regular::Open;
    }
    // the hull's only root is the one certified
    if meet(ru, b.u).is_none() || meet(rv, b.v).is_none() {
        return Regular::Empty;
    }
    if within(ru, b.u) && within(rv, b.v) {
        return Regular::Root(root, [hu, hv]);
    }
    // its box reaches past the border: the root is certified again in the small box
    // about the middle of the first, which tells the box holding it from its neighbour
    // unless the root is within that small box of the border, where both report it (one
    // root: the identity test in `isolate`)
    let Ok(Some((tight, _))) = krawczyk_region(mids(&root), *y, &value, &jacobian) else {
        return Regular::Open;
    };
    let [tu, tv, _] = tight;
    let inside_first = within(tu, ru) && within(tv, rv);
    if !inside_first {
        return Regular::Open;
    }
    if meet(tu, b.u).is_some() && meet(tv, b.v).is_some() {
        Regular::Root(tight, [hu, hv])
    } else {
        Regular::Empty
    }
}

/// Whether the interval Jacobian of `F` over the `(u, v)` box is regular (the contraction
/// of Krawczyk's operator below 1): the box then holds one root at most, with any line
/// parameter.
fn regular_over(net: &LineNet, line: &ScratchLine, u: Ivl, v: Ivl) -> bool {
    let Some([_, s_u, s_v]) = net.over(u, v, line.p) else {
        return false;
    };
    let minus_d = line.d.map(|e| -e);
    let Some(y) = inverse_of_columns3(&[mids(&s_u), mids(&s_v), minus_d]) else {
        return false;
    };
    let jac: [[Ivl; 3]; 3] =
        core::array::from_fn(|k| match (s_u.get(k), s_v.get(k), minus_d.get(k)) {
            (Some(&a), Some(&c), Some(&e)) => [a, c, Ivl::from(e)],
            _ => [Ivl::whole(); 3],
        });
    let (_, q) = contraction(&y, &jac);
    q < 1.0
}

/// The certified root as it is reported: Newton's root from the enclosure's middle where
/// it lies in the enclosure (else that middle), kept when it is on the leaf's line interval
/// and the gap there is enclosed within the tolerance. `None` otherwise: nothing is
/// reported that is not shown.
fn refined(
    net: &LineNet,
    line: &ScratchLine,
    enclosure: I3,
    unique: [Ivl; 2],
) -> Option<CertifiedRoot> {
    let at = |p: [f64; 3]| -> Result<Linearised<3>> {
        let [u, v, t] = p;
        Ok(net
            .over(Ivl::from(u), Ivl::from(v), line.p)
            .map(|[s, s_u, s_v]| {
                (
                    mids(&less(&s, Ivl::from(t), line.d)),
                    [mids(&s_u), mids(&s_v), line.d.map(|e| -e)],
                )
            }))
    };
    let start = mids(&enclosure);
    let point = match newton(start, &at, &inverse_of_columns3) {
        Ok(Some(p)) => p,
        _ => start,
    };
    // Newton's point stands for the root only inside the box proved to hold it
    let held = point
        .iter()
        .zip(&enclosure)
        .all(|(&x, e)| x >= e.lo && x <= e.hi);
    let point = if held { point } else { start };
    let [u, v, t] = point;
    let [t0, t1] = line.interval;
    if !(t >= t0 && t <= t1) {
        return None;
    }
    let s = net.at(u, v, line.p)?;
    let gap2 = length_squared(&less(&s, Ivl::from(t), line.d));
    let shown = gap2.hi <= (Ivl::from(line.tol) * Ivl::from(line.tol)).lo;
    shown.then_some(CertifiedRoot {
        point,
        enclosure,
        unique,
    })
}

// ---- reuse of a solve's search within one operation (not upstream) ------------------------------

/// What the certified pass of one solve found before it reads the solve's solutions
/// (module docs, "Reuse within an operation"): every searched leaf, in the traversal's
/// order, then every culled strip (none when the leaves' passes all did not run), with the
/// drop applied; and the components, typed (none built when the record is not run).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Search {
    /// The searched leaves, then the strips.
    pub leaves: Vec<SearchedLeaf>,
    /// The components of what the passes kept, typed.
    pub components: Components,
}

impl Search {
    /// About how many bytes the search holds: its records and their lists, each list by its
    /// capacity (a drop's `retain` keeps it) at its items' size, the allocator's own
    /// overhead apart; for [`MAX_KEPT_BYTES`].
    fn held_bytes(&self) -> usize {
        use core::mem::size_of;
        // a list's storage: its capacity at its items' size
        fn bytes<T>(v: &Vec<T>) -> usize {
            v.capacity().saturating_mul(size_of::<T>())
        }
        let lists =
            |v: &Vec<Vec<[Ivl; 2]>>| v.iter().fold(bytes(v), |n, x| n.saturating_add(bytes(x)));
        let leaves = self.leaves.iter().fold(bytes(&self.leaves), |n, l| {
            n.saturating_add(l.floor.as_ref().map_or(0, bytes))
                .saturating_add(bytes(&l.roots))
        });
        let components = self
            .components
            .list
            .iter()
            .fold(bytes(&self.components.list), |n, c| {
                n.saturating_add(bytes(&c.boxes))
                    .saturating_add(bytes(&c.roots))
                    .saturating_add(lists(&c.classes))
                    .saturating_add(lists(&c.beyond))
            });
        leaves
            .saturating_add(components)
            .saturating_add(bytes(&self.components.boxes))
    }
}

/// A leaf the traversal of a solve solved, as its search reads it: its domain, its line
/// interval and whether it lies wholly inside the face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SolvedLeaf {
    /// The leaf's domain.
    pub domain: Extent2d,
    /// The leaf's line interval: its roots are reported for `t` in it.
    pub interval: Extent1d,
    /// Whether it lies wholly inside the face ([`SearchedLeaf::inside`]).
    pub inside: bool,
}

/// The certified pass's search of one solve (module docs, "Reuse within an operation"):
/// each solved leaf of `leaves` searched by [`isolate`], each culled strip of `strips`
/// enclosed ([`enclose_strip`]), the drop and the components, typed. Its inputs are the
/// inputs of its [`PassKey`] and nothing else; `cancel` only stops it, and its work is
/// counted in `counters`.
///
/// # Errors
///
/// A cancel.
pub(crate) fn search(
    net: &LineNet,
    line: &PassLine,
    leaves: &[SolvedLeaf],
    strips: &[Extent2d],
    cancel: Option<&CancelToken>,
    counters: &WorkCounters,
) -> Result<Search> {
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    // the leaves and the culled strips: what lies across each one's sides
    let domains: Vec<Extent2d> = leaves
        .iter()
        .map(|job| job.domain)
        .chain(strips.iter().copied())
        .collect();
    // where the solve's parameter space closes on itself across a seam
    let seams = net.seams(&line.solve_domain);
    let sides = leaf_sides(&domains, &line.solve_domain, seams);
    let mut searched: Vec<SearchedLeaf> = Vec::new();
    // the records the drop and the components read, bounded (`MAX_SOLVE_BOXES`)
    let mut records = 0_usize;
    let mut over_budget = false;
    for (job, job_sides) in leaves.iter().zip(&sides) {
        let found = isolate(net, line, &job.domain, &job.interval, job_sides, cancel)?;
        counters.add_line_pass(count(found.boxes), count(found.unshown), found.stop);
        counters.add_line_exclusion(count(found.exclusion.boxes), found.exclusion.out_of_budget);
        records = records.saturating_add(found.floor.len());
        over_budget |= records > MAX_SOLVE_BOXES;
        searched.push(SearchedLeaf {
            domain: job.domain,
            stop: found.stop,
            // (past the bound the records are not kept, and never read as absence)
            floor: (!over_budget).then_some(found.floor),
            roots: found.roots,
            strip: false,
            inside: job.inside,
        });
    }
    if leaves_not_run(&searched) {
        // every solved leaf's pass did not run: the record is not run whatever the strips
        // would find, so they are not enclosed (nothing but the counters would read them)
        counters.add_line_solve(0, false);
        return Ok(Search {
            leaves: searched,
            components: Components::default(),
        });
    }
    for strip in strips {
        if over_budget {
            // (past the bound nothing is read of a strip: the components are not built)
            searched.push(SearchedLeaf {
                domain: *strip,
                stop: Stop::NotRun,
                floor: None,
                roots: Vec::new(),
                strip: true,
                inside: false,
            });
            continue;
        }
        let found = enclose_strip(net, line, strip, cancel)?;
        counters.add_line_strip(count(found.boxes), found.stop);
        records = records.saturating_add(found.floor.len());
        over_budget |= records > MAX_SOLVE_BOXES;
        searched.push(SearchedLeaf {
            domain: *strip,
            stop: found.stop,
            floor: (!over_budget).then_some(found.floor),
            roots: Vec::new(),
            strip: true,
            inside: false,
        });
    }
    let mut comps = if over_budget {
        Components::over_budget()
    } else {
        let drops = drop_fenced(
            net,
            line,
            &mut searched,
            &sides,
            cancel,
            MAX_DROP_ENCLOSURES,
        )?;
        counters.add_line_drops(&drops);
        components(&searched, &sides, line.span, seams)
    };
    let at_limit = searched.iter().any(|l| !l.strip && l.stop == Stop::AtLimit);
    counters.add_line_solve(count(comps.list.len()), at_limit);
    // (a record not run is not typed)
    if !leaves_not_run(&searched) {
        type_components(net, line, &searched, &sides, &mut comps, cancel)?;
    }
    Ok(Search {
        leaves: searched,
        components: comps,
    })
}

/// The inputs a [`Search`] is a function of, bit for bit (module docs, "Reuse within an
/// operation"): the net, and the line, the leaves and the strips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PassKey {
    /// [`LineNet::words`].
    net: Vec<u64>,
    /// The line's point, vector, own interval, the solve's domain and the tolerance; the
    /// number of leaves and each leaf's domain, line interval and whether it lies wholly
    /// inside the face; the number of strips and each one's domain.
    rest: Vec<u64>,
}

/// A count as a word of a key.
fn count_word(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// An `(u, v)` domain as four words of a key.
fn domain_words(d: &Extent2d, out: &mut Vec<u64>) {
    let (lo, hi) = (d.min(), d.max());
    out.extend([lo.x, lo.y, hi.x, hi.y].map(f64::to_bits));
}

impl PassKey {
    /// The key of the search of `line` with the surface of `net` over the solved `leaves`
    /// (in the traversal's order) and the culled `strips` ([`search`]).
    pub(crate) fn of(
        net: &LineNet,
        line: &PassLine,
        leaves: &[SolvedLeaf],
        strips: &[Extent2d],
    ) -> Self {
        let mut rest = Vec::new();
        let PassLine {
            point,
            vector,
            span,
            solve_domain,
            tol,
        } = line;
        rest.extend(
            [
                point.x, point.y, point.z, vector.x, vector.y, vector.z, span[0], span[1],
            ]
            .map(f64::to_bits),
        );
        domain_words(solve_domain, &mut rest);
        rest.push(tol.to_bits());
        rest.push(count_word(leaves.len()));
        for SolvedLeaf {
            domain,
            interval,
            inside,
        } in leaves
        {
            domain_words(domain, &mut rest);
            rest.extend([interval.min().to_bits(), interval.max().to_bits()]);
            rest.push(u64::from(*inside));
        }
        rest.push(count_word(strips.len()));
        for strip in strips {
            domain_words(strip, &mut rest);
        }
        Self {
            net: net.words(),
            rest,
        }
    }
}

impl LineNet {
    /// Every value the passes read of the net, as words, each `f64` by its bits, each list
    /// after its length, each option after whether it is there: two nets with the same
    /// words are the same to every pass ([`PassKey`]).
    fn words(&self) -> Vec<u64> {
        // (every field, so that a field added is a field keyed)
        let LineNet {
            net,
            range,
            creases,
            breaks,
            revolved,
            form,
            closes,
        } = self;
        let mut out = Vec::new();
        let list = |out: &mut Vec<u64>, xs: &[f64]| {
            out.push(count_word(xs.len()));
            out.extend(xs.iter().map(|x| x.to_bits()));
        };
        let Nurbs {
            p,
            q,
            nu,
            nv,
            u,
            v,
            pw,
        } = net;
        out.extend([*p, *q, *nu, *nv].map(count_word));
        list(&mut out, u);
        list(&mut out, v);
        list(&mut out, pw);
        let [[u0, u1], [v0, v1]] = *range;
        out.extend([u0, u1, v0, v1].map(f64::to_bits));
        for c in creases.iter().chain(breaks) {
            list(&mut out, c);
        }
        out.extend(closes.map(u64::from));
        match revolved {
            None => out.push(0),
            Some(r) => {
                out.push(1);
                let Revolved {
                    origin,
                    x,
                    y,
                    z,
                    shape,
                    turn_in_u,
                } = r;
                for a in [origin, x, y, z] {
                    out.extend(a.map(f64::to_bits));
                }
                let (kind, a, b) = match *shape {
                    Shape::Sphere { radius } => (0, radius, 0.0),
                    Shape::Cone { radius, slope } => (1, radius, slope),
                    Shape::Torus { major, minor } => (2, major, minor),
                };
                out.extend([kind, a.to_bits(), b.to_bits(), u64::from(*turn_in_u)]);
            }
        }
        match form {
            None => out.push(0),
            Some(f) => {
                out.push(1);
                out.extend([f.n(), f.m(), f.p(), f.q(), f.r(), f.s()].map(count_word));
                for row in f.rows() {
                    out.push(count_word(row.len()));
                    for c in row {
                        out.extend([c.x, c.y, c.z, c.w].map(f64::to_bits));
                    }
                }
                list(&mut out, f.u_knots());
                list(&mut out, f.v_knots());
            }
        }
        out
    }
}

/// The most searches a [`LinePasses`] keeps.
pub(crate) const MAX_KEPT_SEARCHES: usize = 1 << 16;

/// The most bytes a [`LinePasses`] holds, over its keys ([`PassKey`], each net once) and
/// its searches ([`Search::held_bytes`]): 32 MiB. A solve at its bound holds
/// [`MAX_SOLVE_BOXES`] floor boxes and as many component boxes, about 18 MiB, so this keeps
/// the searches of one or two such solves, or of thousands of ordinary ones (a Boolean's
/// solve of a tapered wall's rulings kept 4,482 searches in 30 operations, at most 1.6 MiB
/// in one).
pub(crate) const MAX_KEPT_BYTES: usize = 32 << 20;

/// The searches of the certified line pass one operation has made, kept for the solves of
/// that operation that repeat one on the same inputs (module docs, "Reuse within an
/// operation"): a pure memo. A search is found only under a key equal word for word to the
/// one it was kept under, and is the value the pass computes from those inputs, so a solve
/// that reuses it returns what it would have computed. At most [`MAX_KEPT_SEARCHES`]
/// searches and [`MAX_KEPT_BYTES`] bytes are kept; past either a search is not kept, and a
/// solve that repeats it searches again.
#[derive(Debug, Default)]
pub struct LinePasses {
    /// Each net's words once, with its number.
    nets: BTreeMap<Vec<u64>, usize>,
    /// The searches, by their net's number, then the rest of their key.
    searches: BTreeMap<usize, BTreeMap<Vec<u64>, Arc<Search>>>,
    /// The searches kept ([`MAX_KEPT_SEARCHES`]).
    kept: usize,
    /// The bytes held ([`MAX_KEPT_BYTES`]).
    bytes: usize,
}

impl LinePasses {
    /// The search kept under `key`, if any.
    pub(crate) fn found(&self, key: &PassKey) -> Option<Arc<Search>> {
        let net = self.nets.get(&key.net)?;
        self.searches.get(net)?.get(&key.rest).cloned()
    }

    /// Keeps `search` under `key`, within the bounds; a search past them is not kept. The
    /// key's lists are shrunk to their words before they are kept, and charged by what they
    /// then hold (their capacity, as [`Search::held_bytes`] charges a search's lists): the
    /// spare room a key grew with is never held past the budget.
    pub(crate) fn keep(&mut self, mut key: PassKey, search: &Arc<Search>) {
        use core::mem::size_of;
        if self.kept >= MAX_KEPT_SEARCHES {
            return;
        }
        let words = |n: usize| n.saturating_mul(size_of::<u64>());
        let new_net = !self.nets.contains_key(&key.net);
        key.rest.shrink_to_fit();
        if new_net {
            key.net.shrink_to_fit();
        }
        let bytes = words(key.rest.capacity())
            .saturating_add(if new_net {
                words(key.net.capacity())
            } else {
                0
            })
            .saturating_add(search.held_bytes());
        let total = self.bytes.saturating_add(bytes);
        if total > MAX_KEPT_BYTES {
            return;
        }
        let next = self.nets.len();
        let net = *self.nets.entry(key.net).or_insert(next);
        if self
            .searches
            .entry(net)
            .or_default()
            .insert(key.rest, Arc::clone(search))
            .is_none()
        {
            self.kept = self.kept.saturating_add(1);
        }
        self.bytes = total;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::cone::Cone;
    use crate::surface::torus::{Torus, TorusOptions};

    /// The pass over the whole of `s`, for `t` in `[t0, t1]` (the line's interval and the
    /// leaf's).
    fn pass(k: &Kernel, s: &Surface, p: Vec3, d: Vec3, [t0, t1]: [f64; 2], tol: f64) -> Isolation {
        pass_in(k, s, p, d, [t0, t1], [t0, t1], tol)
    }

    /// The pass over the whole of `s`, the line's own interval `span` and the leaf's
    /// `[t0, t1]`, with the solve's drop over that one leaf after it.
    fn pass_in(
        k: &Kernel,
        s: &Surface,
        p: Vec3,
        d: Vec3,
        span: [f64; 2],
        [t0, t1]: [f64; 2],
        tol: f64,
    ) -> Isolation {
        let net = LineNet::of(k, s).unwrap().unwrap();
        let domain = s.natural_uv_domain_with_nurb(k).unwrap();
        let interval = Extent1d::from_min_max(t0, t1);
        let line = PassLine {
            point: p,
            vector: d,
            span,
            solve_domain: domain,
            tol,
        };
        let sides = LeafSides::whole_solve(&domain);
        let mut found = isolate(&net, &line, &domain, &interval, &sides, None).unwrap();
        let mut leaf = [SearchedLeaf {
            domain,
            stop: found.stop,
            floor: Some(std::mem::take(&mut found.floor)),
            roots: Vec::new(),
            strip: false,
            inside: true,
        }];
        drop_fenced(&net, &line, &mut leaf, &[sides], None, MAX_DROP_ENCLOSURES).unwrap();
        found.floor = leaf[0].floor.take().unwrap();
        found
    }

    /// The line parameters of the certified roots, in order.
    fn parameters(found: &Isolation) -> Vec<f64> {
        let mut t: Vec<f64> = found.roots.iter().map(|r| r.point[2]).collect();
        t.sort_by(f64::total_cmp);
        t
    }

    /// The unit cylinder about z, of height 1.
    fn cylinder(k: &Kernel) -> Surface {
        let mut c = Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            1.0,
            0.0,
            360.0,
            1.0,
            false,
            false,
            true,
        );
        c.make_nurb(k).unwrap();
        Surface::from(c)
    }

    /// The line along x at height 0.5 and `y`, from x = −1 over a length of 2: it meets
    /// the unit cylinder where `x = ±√(1 − y²)`, at `t = 0.5 ± √(1 − y²) / 2`.
    fn across(y: f64) -> (Vec3, Vec3) {
        (Vec3::new(-1.0, y, 0.5), Vec3::new(2.0, 0.0, 0.0))
    }

    /// The tolerance and the step of upstream's `my_test_csi_tangency`.
    const TOL: f64 = 1.0001e-3;
    const STEP: f64 = 2.0e-3 / 22.0;

    /// The entry and exit of a ray near a torus's crown (2.5° and 2° off the surface,
    /// 2.75 apart) are two certified crossings: the torus of radii 30 and 5 about z and the
    /// ray of tangency-labs/solidcraft#20499. The parameters are the roots of the torus's
    /// quartic along the line (100.409930808 and 103.163922132, computed by two parties
    /// in closed form).
    #[test]
    fn two_crossings_near_a_torus_crown_are_certified_apart() {
        let k = Kernel::default();
        let mut torus = Torus::new(
            &k,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            30.0,
            5.0,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(&k).unwrap();
        let s = Surface::from(torus);
        let p = Vec3::new(82.388_988_497_226_59, 82.388_988_497_226_59, 10.000_04);
        let d = Vec3::new(
            -0.850_300_871_283_819_5,
            -0.523_935_376_527_776_1,
            -0.049_801_099_552_871_67,
        );
        let found = pass(&k, &s, p, d, [99.0, 105.0], 2.0e-5);
        assert_eq!(found.stop, Stop::Finished);
        let t = parameters(&found);
        assert_eq!(t.len(), 2, "{t:?}");
        assert!((t[0] - 100.409_930_808).abs() < 1.0e-8, "{t:?}");
        assert!((t[1] - 103.163_922_132).abs() < 1.0e-8, "{t:?}");
        for r in &found.roots {
            let [u, v, t] = r.point;
            let on_surface = s
                .evaluate_point(&k, crate::math::Point2::new(u, v))
                .unwrap();
            assert!(on_surface.distance(p + t * d) < 1.0e-10);
        }
    }

    /// A secant far shallower than the tolerance is still two crossings, each certified:
    /// upstream's tangency test at its step 10, 9.09e-5 deep at tolerance 1.0001e-3.
    #[test]
    fn a_shallow_secant_is_two_certified_crossings() {
        let k = Kernel::default();
        let s = cylinder(&k);
        for step in 0..=10 {
            let y = 1.0 - f64::from(11 - step) * STEP;
            let (p, d) = across(y);
            let found = pass(&k, &s, p, d, [0.0, 1.0], TOL);
            assert_eq!(found.stop, Stop::Finished);
            let half = (1.0 - y * y).sqrt() / 2.0;
            let t = parameters(&found);
            assert_eq!(t.len(), 2, "step {step}: {t:?}");
            assert!((t[0] - (0.5 - half)).abs() < 1.0e-12, "step {step}: {t:?}");
            assert!((t[1] - (0.5 + half)).abs() < 1.0e-12, "step {step}: {t:?}");
        }
    }

    /// A tangent line and a line that misses within the tolerance hold no crossing, and
    /// the contact is left to the floor: the box about it is neither excluded nor decided.
    #[test]
    fn a_tangent_line_and_a_near_miss_are_left_to_the_floor() {
        let k = Kernel::default();
        let s = cylinder(&k);
        for step in [0, 1, 10] {
            let (p, d) = across(1.0 + f64::from(step) * STEP);
            let found = pass(&k, &s, p, d, [0.0, 1.0], TOL);
            assert_eq!(found.stop, Stop::Finished);
            assert!(found.roots.is_empty(), "step {step}: {:?}", found.roots);
            assert!(found.left > 0, "step {step}");
        }
    }

    /// A line farther from the surface than the tolerance leaves nothing: every box is
    /// excluded.
    #[test]
    fn a_line_farther_than_the_tolerance_is_excluded() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let (p, d) = across(1.0 + 2.0 * TOL);
        let found = pass(&k, &s, p, d, [0.0, 1.0], TOL);
        assert_eq!(found.stop, Stop::Finished);
        assert!(found.roots.is_empty());
        assert_eq!(found.left, 0);
    }

    /// A line along a generator of the cylinder is left to the floor without being cut
    /// down to the tolerance: far fewer boxes than the 1 / 1e-5 a cut to the tolerance
    /// along it would take.
    #[test]
    fn a_line_along_a_generator_is_left_in_few_boxes() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let found = pass(
            &k,
            &s,
            Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 3.0),
            [0.0, 1.0],
            1.0e-5,
        );
        assert_eq!(found.stop, Stop::Finished);
        assert!(found.roots.is_empty(), "{:?}", found.roots);
        assert!(found.left > 0);
        assert!(found.boxes < 500, "{} boxes", found.boxes);
    }

    /// A crossing on a knot line of the net (the cylinder's quarter arcs meet at 90°) is
    /// one root, though boxes on both sides of the line hold it on their borders.
    #[test]
    fn a_crossing_on_a_knot_line_is_one_root() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let found = pass(
            &k,
            &s,
            Vec3::new(0.0, -2.0, 0.5),
            Vec3::new(0.0, 4.0, 0.0),
            [0.0, 1.0],
            1.0e-5,
        );
        assert_eq!(found.stop, Stop::Finished);
        let t = parameters(&found);
        assert_eq!(t.len(), 2, "{t:?}");
        assert!(
            (t[0] - 0.25).abs() < 1.0e-12 && (t[1] - 0.75).abs() < 1.0e-12,
            "{t:?}"
        );
    }

    /// The sphere of radius 5 about the origin, whole.
    fn sphere(k: &Kernel) -> Surface {
        let mut s = crate::surface::sphere::Sphere::new(
            k,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
            5.0,
            crate::surface::sphere::SphereOptions::default(),
        )
        .unwrap();
        s.make_nurb(k).unwrap();
        Surface::from(s)
    }

    /// A line tangent to a sphere, one that misses it within the tolerance, and one
    /// through its poles: the tangent contact and the poles are left to the floor (no
    /// box about them is regular), the pass finishes well inside its bound, and the
    /// crossings it does certify are the sphere's (none for the first two).
    #[test]
    fn a_tangent_line_and_a_line_through_the_poles_of_a_sphere_finish() {
        let k = Kernel::default();
        let s = sphere(&k);
        let tol = 1.0e-6;
        for gap in [0.0, 0.5 * tol] {
            // along x at y = 3, z = 4 + gap: tangent to the sphere at (0, 3, 4) for no gap
            let p = Vec3::new(-10.0, 3.0 * (1.0 + gap / 5.0), 4.0 * (1.0 + gap / 5.0));
            let found = pass(&k, &s, p, Vec3::new(20.0, 0.0, 0.0), [0.0, 1.0], tol);
            assert_eq!(
                found.stop,
                Stop::Finished,
                "gap {gap}: {} boxes",
                found.boxes
            );
            assert!(found.roots.is_empty(), "gap {gap}: {:?}", found.roots);
            assert!(found.left > 0, "gap {gap}");
            assert!(
                found.boxes < MAX_SCRATCH_BOXES / 4,
                "gap {gap}: {} boxes",
                found.boxes
            );
        }
        let found = pass(
            &k,
            &s,
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::new(0.0, 0.0, 20.0),
            [0.0, 1.0],
            tol,
        );
        assert_eq!(found.stop, Stop::Finished, "{} boxes", found.boxes);
        assert!(found.boxes < MAX_SCRATCH_BOXES / 2, "{} boxes", found.boxes);
        for t in parameters(&found) {
            assert!(
                (t - 0.25).abs() < 1.0e-9 || (t - 0.75).abs() < 1.0e-9,
                "{t}"
            );
        }
    }

    /// A line along a generator that lies on a knot line of the net which no halving of
    /// the domain lands on (a 300° cylinder's arcs meet at 100° and 200°): the box is cut
    /// at the knot, and the line is left in few boxes, as on any other generator.
    #[test]
    fn a_line_along_a_generator_on_a_knot_line_is_left_in_few_boxes() {
        let k = Kernel::default();
        let mut c = Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            1.0,
            0.0,
            300.0,
            1.0,
            false,
            false,
            true,
        );
        c.make_nurb(&k).unwrap();
        let s = Surface::from(c);
        let a = 100.0_f64.to_radians();
        let found = pass(
            &k,
            &s,
            Vec3::new(det::cos(a), det::sin(a), -1.0),
            Vec3::new(0.0, 0.0, 3.0),
            [0.0, 1.0],
            1.0e-5,
        );
        assert_eq!(found.stop, Stop::Finished, "{} boxes", found.boxes);
        assert!(found.roots.is_empty(), "{:?}", found.roots);
        assert!(found.left > 0);
        assert!(found.boxes < 500, "{} boxes", found.boxes);
    }
    /// The torus of radii 30 and 5 about z, whole: its net has knots at the quarters of
    /// both parameters, each of multiplicity 2 at degree 2 (below `C¹` in parameter).
    fn torus(k: &Kernel) -> Surface {
        let mut torus = Torus::new(
            k,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            30.0,
            5.0,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(k).unwrap();
        Surface::from(torus)
    }

    /// A line through the cylinder's axis along x, at a height of half and nine tenths of
    /// the tolerance above its top rim, passes within the tolerance of the rim at x = ±1
    /// (t = 0.25 and 0.75) with no root on the surface. The boxes about those points have
    /// no root, and a side on the top rim (the end of the net's knots in v), or on the
    /// crease under it at x = −1, keeps them as edge boxes: not one of them is a plain box
    /// with no root, and some lie on the rim.
    #[test]
    fn a_line_just_beyond_a_rim_is_kept_in_edge_boxes() {
        let k = Kernel::default();
        let s = cylinder(&k);
        for h in [0.5, 0.9] {
            let found = pass(
                &k,
                &s,
                Vec3::new(-2.0, 0.0, 1.0 + h * TOL),
                Vec3::new(4.0, 0.0, 0.0),
                [0.0, 1.0],
                TOL,
            );
            assert_eq!(found.stop, Stop::Finished);
            assert!(found.roots.is_empty(), "h {h}: {:?}", found.roots);
            for at in [0.25, 0.75] {
                let near: Vec<&FloorBox> = found
                    .floor
                    .iter()
                    .filter(|r| r.t.is_some_and(|t| t.lo <= at && at <= t.hi))
                    .collect();
                assert!(!near.is_empty(), "h {h}, t {at}: {:?}", found.floor);
                for r in &near {
                    assert_eq!(r.kind, FloorKind::Edge, "h {h}, t {at}: {r:?}");
                }
                // (the exclusion phase cuts edge boxes too: a piece under the rim is an
                // edge box by the crease at u = π it touches; the rim keeps the others)
                assert!(
                    near.iter().any(|r| r.v.hi == 1.0),
                    "h {h}, t {at}: {near:?}"
                );
            }
        }
    }

    /// A line along the torus's radius at 90° in its middle plane crosses the tube where
    /// u = 0.25, a quarter knot, at the inner equator (t = 0.25) and at the outer one
    /// (t = 0.75, where v starts and ends). Boxes with no root beside the crossings that
    /// touch u = 0.25 but neither end of the knots are kept as edge boxes by that knot
    /// alone.
    #[test]
    fn a_torus_quarter_knot_keeps_edge_boxes() {
        let k = Kernel::default();
        let s = torus(&k);
        let found = pass(
            &k,
            &s,
            Vec3::new(0.0, 20.0, 0.0),
            Vec3::new(0.0, 20.0, 0.0),
            [0.0, 1.0],
            1.0e-6,
        );
        assert_eq!(found.stop, Stop::Finished);
        let by_the_knot = found.floor.iter().filter(|r| {
            r.kind == FloorKind::Edge
                && (r.u.lo == 0.25 || r.u.hi == 0.25)
                && r.u.lo > 0.0
                && r.u.hi < 1.0
                && r.v.lo > 0.0
                && r.v.hi < 1.0
        });
        assert!(by_the_knot.count() > 0, "{:?}", found.floor);
        // and no box with no root touching the knot is a plain one
        assert!(!found
            .floor
            .iter()
            .any(|r| r.kind == FloorKind::NoRoot && (r.u.lo == 0.25 || r.u.hi == 0.25)));
    }

    /// A line tangent to the torus's crown circle at 45° touches it at one point with a
    /// gap of fourth order; the fold is singular along the crown, so the pass halves the
    /// boxes along it down to the floor's size and stops at its bound. What it did not
    /// reach is recorded as unvisited, never as excluded.
    #[test]
    fn a_pass_at_its_limit_records_the_boxes_it_did_not_visit() {
        let k = Kernel::default();
        let s = torus(&k);
        let c = 30.0 / 2.0_f64.sqrt();
        let along = Vec3::new(-1.0, 1.0, 0.0) * (1.0 / 2.0_f64.sqrt());
        let found = pass(
            &k,
            &s,
            Vec3::new(c, c, 5.0) - 10.0 * along,
            20.0 * along,
            [0.0, 1.0],
            1.0e-6,
        );
        assert_eq!(found.stop, Stop::AtLimit);
        assert_eq!(found.boxes, MAX_SCRATCH_BOXES);
        let unvisited: Vec<&FloorBox> = found
            .floor
            .iter()
            .filter(|r| r.kind == FloorKind::Unvisited)
            .collect();
        assert!(!unvisited.is_empty());
        // each with its line parameters enclosed, so that a ray can tell whether it lies
        // before a hit
        assert!(unvisited.iter().all(|r| r.t.is_some()));
        // the boxes still to visit come last
        let first = found.floor.len() - unvisited.len();
        assert!(found.floor[first..]
            .iter()
            .all(|r| r.kind == FloorKind::Unvisited));
        // a finished pass leaves none
        let (p, d) = across(0.5);
        let done = pass(&k, &cylinder(&k), p, d, [0.0, 1.0], TOL);
        assert_eq!(done.stop, Stop::Finished);
        assert!(!done.floor.iter().any(|r| r.kind == FloorKind::Unvisited));
    }

    /// A pass that nothing can be proved of (no tolerance, a line of no length) does not
    /// run, and is counted as not run: not as stopped at the limit, not as finished.
    #[test]
    fn a_pass_that_does_not_run_is_not_counted_at_the_limit() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let (p, d) = across(0.5);
        for (d, tol) in [(d, 0.0), (Vec3::ZERO, TOL), (d, f64::NAN)] {
            let found = pass(&k, &s, p, d, [0.0, 1.0], tol);
            assert_eq!(found.stop, Stop::NotRun);
            assert_eq!(found.boxes, 0);
            assert!(found.floor.is_empty() && found.roots.is_empty());
            k.work_counters.add_line_pass(0, 0, found.stop);
        }
        assert_eq!(k.work_counters.line_leaves_not_run(), 3);
        assert_eq!(k.work_counters.line_leaves_at_limit(), 0);
        assert_eq!(k.work_counters.line_leaves_finished(), 0);
    }

    /// The line across the cylinder at y = 0.5 meets it at t = 0.5 ∓ √0.75 / 2. With the
    /// leaf's interval [0, 0.5] inside the line's [0, 1], the second root is certified but
    /// lies off the leaf: it is unshown, not excluded. With the line's own interval [0, 0.5]
    /// the boxes wholly past it are excluded by t, and no box that is not wholly past it
    /// is.
    #[test]
    fn excluded_by_t_reads_the_line_s_interval_not_the_leaf_s() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let (p, d) = across(0.5);
        let half = 0.75_f64.sqrt() / 2.0;
        let (near, far) = (0.5 - half, 0.5 + half);
        let holds = |r: &FloorBox, at: f64| r.t.is_some_and(|t| t.lo <= at && at <= t.hi);

        let found = pass_in(&k, &s, p, d, [0.0, 1.0], [0.0, 0.5], TOL);
        assert_eq!(found.stop, Stop::Finished);
        let t = parameters(&found);
        assert_eq!(t.len(), 1, "{t:?}");
        assert!((t[0] - near).abs() < 1.0e-12, "{t:?}");
        assert!(found.unshown > 0);
        assert!(found
            .floor
            .iter()
            .any(|r| matches!(r.kind, FloorKind::Unshown { .. }) && holds(r, far)));
        assert!(!found
            .floor
            .iter()
            .any(|r| r.kind == FloorKind::ExcludedByT && holds(r, far)));

        let found = pass_in(&k, &s, p, d, [0.0, 0.5], [0.0, 0.5], TOL);
        assert_eq!(found.stop, Stop::Finished);
        let t = parameters(&found);
        assert_eq!(t.len(), 1, "{t:?}");
        assert!((t[0] - near).abs() < 1.0e-12, "{t:?}");
        assert!(found.floor.iter().any(|r| r.kind == FloorKind::ExcludedByT));
        for r in &found.floor {
            let past = r.t.is_some_and(|t| t.lo > 0.5 || t.hi < 0.0);
            assert_eq!(past, r.kind == FloorKind::ExcludedByT, "{r:?}");
        }
    }

    /// A searched leaf over `domain`, its pass ended at `stop`, with the records `floor`.
    /// A certified root whose line parameters are `[lo, hi]` (its `(u, v)` boxes unit).
    fn root_over(lo: f64, hi: f64) -> CertifiedRoot {
        let unit = Ivl::new(0.0, 1.0);
        CertifiedRoot {
            point: [0.5, 0.5, 0.5 * (lo + hi)],
            enclosure: [unit, unit, Ivl::new(lo, hi)],
            unique: [unit, unit],
        }
    }

    /// A box left to visit whose line parameters are `t`.
    fn unvisited(t: Ivl) -> FloorBox {
        FloorBox {
            u: Ivl::new(0.0, 1.0),
            v: Ivl::new(0.0, 1.0),
            t: Some(t),
            kind: FloorKind::Unvisited,
        }
    }

    /// The bound of a solve's roots is taken over every leaf at its limit: a leaf whose
    /// root at 10 lies before every box it left adds nothing while another leaf at its limit
    /// left a box from 5 (a crossing at 6 there would come first along the line); a root of
    /// the second leaf below 5 is added; a finished leaf adds all its roots whatever the
    /// others left; a culled strip's undecided boxes and a leaf whose pass did not run bound
    /// nothing; a leaf at its limit whose records were not kept withholds every leaf at its
    /// limit.
    #[test]
    fn a_leaf_at_its_limit_adds_no_root_past_a_box_another_left() {
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let t = |r: &CertifiedRoot| r.enclosure[2].lo;
        let leaf = |stop: Stop, floor: Option<Vec<FloorBox>>, roots: Vec<CertifiedRoot>| {
            let mut l = leaf_of(d, stop, floor);
            l.roots = roots;
            l
        };
        let first = leaf(
            Stop::AtLimit,
            Some(vec![unvisited(Ivl::new(20.0, 30.0))]),
            vec![root_over(10.0, 10.5)],
        );
        let second = leaf(
            Stop::AtLimit,
            Some(vec![unvisited(Ivl::new(5.0, 8.0))]),
            vec![root_over(2.0, 2.5)],
        );
        let finished = leaf(
            Stop::Finished,
            Some(Vec::new()),
            vec![root_over(12.0, 12.5)],
        );
        let mut strip = leaf(
            Stop::AtLimit,
            Some(vec![FloorBox {
                kind: FloorKind::Culled,
                ..unvisited(Ivl::new(1.0, 2.0))
            }]),
            Vec::new(),
        );
        strip.strip = true;
        let not_run = leaf(Stop::NotRun, None, vec![root_over(0.5, 0.6)]);
        let added = |leaves: &[SearchedLeaf]| -> Vec<Vec<f64>> {
            publishable_roots(leaves)
                .iter()
                .map(|roots| roots.iter().map(t).collect())
                .collect()
        };
        assert_eq!(added(core::slice::from_ref(&first)), vec![vec![10.0]]);
        assert_eq!(
            added(&[first.clone(), second.clone(), finished, strip, not_run]),
            vec![vec![], vec![2.0], vec![12.0], vec![], vec![]]
        );
        assert_eq!(added(&[second, first.clone()]), vec![vec![2.0], vec![]]);
        let unkept = leaf(Stop::AtLimit, None, Vec::new());
        let none: Vec<f64> = Vec::new();
        assert_eq!(added(&[first, unkept]), vec![none.clone(), none]);
    }

    /// The roots a leaf's pass adds (`publishable_roots`, one leaf): all of a finished pass;
    /// of a pass at its limit, those wholly below the least line parameter of the boxes it
    /// left, that bound lowered to the start of every root reaching it until none does; none
    /// where a box left holds the whole line, the records were not kept, or the pass did not
    /// run.
    #[test]
    fn a_pass_at_its_limit_adds_the_roots_before_every_box_it_left() {
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let t = |r: &CertifiedRoot| r.enclosure[2].lo;
        let with = |stop: Stop, floor: Option<Vec<FloorBox>>, roots: Vec<CertifiedRoot>| {
            let mut leaf = leaf_of(d, stop, floor);
            leaf.roots = roots;
            publishable_roots(&[leaf])
                .concat()
                .iter()
                .map(t)
                .collect::<Vec<f64>>()
        };
        let roots = vec![
            root_over(4.0, 5.0),
            root_over(9.5, 10.5),
            root_over(20.0, 21.0),
        ];
        assert_eq!(
            with(Stop::Finished, Some(Vec::new()), roots.clone()),
            vec![4.0, 9.5, 20.0]
        );
        // the box left from 10 on: the root reaching it is withheld, and so is every root
        // past it
        let left = vec![
            unvisited(Ivl::new(12.0, 30.0)),
            unvisited(Ivl::new(10.0, 11.0)),
        ];
        assert_eq!(
            with(Stop::AtLimit, Some(left.clone()), roots.clone()),
            vec![4.0]
        );
        // a box left over the whole line decides nothing, nor do records not kept, nor a pass
        // that did not run
        let whole = vec![unvisited(Ivl::whole())];
        assert!(with(Stop::AtLimit, Some(whole), roots.clone()).is_empty());
        assert!(with(Stop::AtLimit, None, roots.clone()).is_empty());
        assert!(with(Stop::NotRun, Some(left.clone()), roots).is_empty());
        // a chain of overlapping roots below the bound: the one reaching 10 lowers it to 8,
        // the one reaching 8 to 7, the one reaching 7 to 6.5; only the root wholly below that
        // is added
        let chain = vec![
            root_over(8.0, 10.5),
            root_over(7.0, 8.5),
            root_over(6.5, 7.5),
            root_over(1.0, 2.0),
        ];
        assert_eq!(with(Stop::AtLimit, Some(left), chain), vec![1.0]);
    }

    fn leaf_of(domain: Extent2d, stop: Stop, floor: Option<Vec<FloorBox>>) -> SearchedLeaf {
        SearchedLeaf {
            domain,
            stop,
            floor,
            roots: Vec::new(),
            strip: false,
            inside: true,
        }
    }

    /// The solve's drop over `leaves` (leaves of a solve over the cylinder's whole domain),
    /// with the line through `p` along `d` and a budget of `budget` side enclosures.
    fn drop_over(
        k: &Kernel,
        p: Vec3,
        d: Vec3,
        leaves: &mut [SearchedLeaf],
        budget: usize,
    ) -> Drops {
        let s = cylinder(k);
        let net = LineNet::of(k, &s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(k).unwrap();
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: solve,
            tol: TOL,
        };
        let doms: Vec<Extent2d> = leaves.iter().map(|l| l.domain).collect();
        let sides = leaf_sides(&doms, &solve, net.seams(&solve));
        drop_fenced(&net, &line, leaves, &sides, None, budget).unwrap()
    }

    /// The solve's drop over one finished leaf over `domain` holding the records `recs`:
    /// the records it keeps, and what it did.
    fn excluded(
        k: &Kernel,
        p: Vec3,
        d: Vec3,
        domain: Extent2d,
        recs: Vec<FloorBox>,
        budget: usize,
    ) -> (Vec<FloorBox>, Drops) {
        let mut leaves = [leaf_of(domain, Stop::Finished, Some(recs))];
        let done = drop_over(k, p, d, &mut leaves, budget);
        (leaves[0].floor.take().unwrap(), done)
    }

    /// A record of `kind` over `[u0, u1] × [v0, v1]`, its line parameters `[0, 1]`.
    fn rec(kind: FloorKind, [u0, u1]: [f64; 2], [v0, v1]: [f64; 2]) -> FloorBox {
        FloorBox {
            u: Ivl::new(u0, u1),
            v: Ivl::new(v0, v1),
            t: Some(Ivl::new(0.0, 1.0)),
            kind,
        }
    }

    /// A connected set of plain boxes with no root whose every touching box is excluded by
    /// distance has its least gap on its border, so it is dropped whole. A set beside a box
    /// that may hold the contact (a box left at the floor), beside an edge box whose crease
    /// side faces such a box, or beside an edge box on the solve's border within the
    /// tolerance is kept. A set touching the border of a leaf with nothing searched across
    /// it has a boundary side there, which an enclosure shows farther: it is dropped.
    #[test]
    fn a_set_with_no_root_fenced_by_excluded_boxes_is_dropped() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = (
            Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL),
            Vec3::new(4.0, 0.0, 0.0),
        );
        // the net's first crease, the quarter knot at u = π/2
        let q = LineNet::of(&k, &cylinder(&k)).unwrap().unwrap().creases[0][0];
        let set = || {
            vec![
                rec(FloorKind::NoRoot, [1.0, 1.1], [0.4, 0.5]),
                rec(FloorKind::NoRoot, [1.1, 1.2], [0.4, 0.5]),
                rec(FloorKind::NoRoot, [1.1, 1.2], [0.5, 0.6]),
            ]
        };
        let (kept, done) = excluded(&k, p, d, whole, set(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(done.no_root, 3);
        assert_eq!(done.across, 0);

        // beside a box left at the floor
        let mut recs = set();
        recs.push(rec(FloorKind::Left, [1.2, 1.3], [0.4, 0.5]));
        let (kept, _) = excluded(&k, p, d, whole, recs, MAX_DROP_ENCLOSURES);
        assert_eq!(kept.len(), 4, "{kept:?}");

        // beside an edge box on the crease u = π/2, whose crease side faces a box left
        let mut recs = set();
        recs.push(rec(FloorKind::Edge, [1.2, q], [0.5, 0.6]));
        recs.push(rec(FloorKind::Left, [q, 1.7], [0.5, 0.6]));
        let (kept, _) = excluded(&k, p, d, whole, recs, MAX_DROP_ENCLOSURES);
        assert_eq!(kept.len(), 5, "{kept:?}");

        // beside an edge box on the top rim where the line passes within the tolerance of
        // it (at u = π, half the tolerance above)
        let pi = std::f64::consts::PI;
        let recs = vec![
            rec(FloorKind::NoRoot, [3.0, 3.1], [0.85, 0.95]),
            rec(FloorKind::Edge, [3.1, pi], [0.9, 1.0]),
        ];
        let (kept, _) = excluded(&k, p, d, whole, recs, MAX_DROP_ENCLOSURES);
        assert_eq!(kept.len(), 2, "{kept:?}");

        // touching the border of a leaf with nothing searched across it: the side on u = 1
        // is a boundary side, about one from the line
        let leaf = Extent2d::from_bounds(1.0, 0.0, 2.0, 1.0);
        let (kept, done) = excluded(&k, p, d, leaf, set(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert!(done.enclosures > 0);
    }

    /// An edge box alone is dropped when its side on the solve's border (the top rim) is
    /// shown farther than the tolerance by an enclosure of that side, and its other sides
    /// face boxes excluded by distance: with the line three tolerances above the rim at
    /// u = π. With the line half a tolerance above it, the side is not shown farther, and
    /// the box is kept.
    #[test]
    fn an_edge_box_is_dropped_only_with_its_border_side_shown_farther() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let pi = std::f64::consts::PI;
        let edge = || vec![rec(FloorKind::Edge, [pi - 1.0e-3, pi], [0.99, 1.0])];
        let d = Vec3::new(4.0, 0.0, 0.0);
        let (kept, done) = excluded(
            &k,
            Vec3::new(-2.0, 0.0, 1.0 + 3.0 * TOL),
            d,
            whole,
            edge(),
            MAX_DROP_ENCLOSURES,
        );
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(done.edge, 1);
        assert!(done.enclosures > 0);
        let (kept, done) = excluded(
            &k,
            Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL),
            d,
            whole,
            edge(),
            MAX_DROP_ENCLOSURES,
        );
        assert_eq!(kept, edge());
        assert_eq!(done.edge, 0);
        // with no budget left for the enclosure of its side, it is kept too
        let (kept, done) = excluded(
            &k,
            Vec3::new(-2.0, 0.0, 1.0 + 3.0 * TOL),
            d,
            whole,
            edge(),
            0,
        );
        assert_eq!(kept, edge());
        assert!(done.out_of_budget);
    }

    /// A set of plain and edge boxes with no root is one set: a plain box beside an edge box
    /// on the top rim and the crease at u = π, and a second edge box across that crease
    /// (two edge boxes side by side, which the rule for one edge box at a time kept, each
    /// waiting for the other). With the line three tolerances above the rim, every
    /// boundary and crease side is shown farther and every other side faces boxes excluded
    /// by distance: all three are dropped, and so are the two edge boxes alone. With the
    /// line half a tolerance above the rim, the rim at u = π is within the tolerance, and
    /// the whole set is kept.
    #[test]
    fn a_set_of_plain_and_edge_boxes_is_dropped_whole() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        // the crease at u = π, the knot between the second and third quarter arcs
        let c = LineNet::of(&k, &cylinder(&k)).unwrap().unwrap().creases[0][1];
        let d = Vec3::new(4.0, 0.0, 0.0);
        let edges = || {
            vec![
                rec(FloorKind::Edge, [3.1, c], [0.9, 1.0]),
                rec(FloorKind::Edge, [c, 3.2], [0.9, 1.0]),
            ]
        };
        let set = || {
            let mut recs = edges();
            recs.insert(0, rec(FloorKind::NoRoot, [3.0, 3.1], [0.85, 0.95]));
            recs
        };
        let far = Vec3::new(-2.0, 0.0, 1.0 + 3.0 * TOL);
        let (kept, done) = excluded(&k, far, d, whole, set(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!((done.no_root, done.edge, done.across), (1, 2, 0));
        let (kept, done) = excluded(&k, far, d, whole, edges(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(done.edge, 2);

        let near = Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL);
        let (kept, done) = excluded(&k, near, d, whole, set(), MAX_DROP_ENCLOSURES);
        assert_eq!(kept, set());
        assert_eq!(done.no_root + done.edge, 0);
    }

    /// A crease where the net breaks (a knot of multiplicity above the degree) is no fence:
    /// the box excluded across it holds the other piece's edge, which may lie far from this
    /// one's. A net of degree 1 in u with a double knot at u = ½, its left piece `(u, v, 0)`
    /// and its right piece `(u + 10, v, 0)`, and a box with no root against the knot on the
    /// left: a vertical line through `(0.5005, 0.5)` has no root but passes half a tolerance
    /// (0.0005 of 0.001) from the left piece's edge, and the right piece leaves nothing
    /// across. The set is kept (its side on the break is enclosed, and is not farther). With
    /// a line 0.01 above the net the same side is shown farther by its own enclosure (one
    /// made) and the set is dropped; on the net continuous across u = ½ (a single knot) the
    /// excluded box across still fences the side, and no enclosure is made.
    #[test]
    fn a_crease_where_the_net_breaks_is_shown_by_its_own_enclosure() {
        use crate::nurbs::math::{CPoint, NOW};
        let k = Kernel::default();
        let net_of = |xs: &[f64], u: Vec<f64>| {
            let rows = xs
                .iter()
                .map(|&x| {
                    (0..2)
                        .map(|j| CPoint::new(x, f64::from(j), 0.0, NOW))
                        .collect()
                })
                .collect();
            let n = crate::nurbs::srf_basic::Surface::new(rows, 1, 1, u, vec![0.0, 0.0, 1.0, 1.0])
                .unwrap();
            Surface::from(crate::nurbs::b_spline_surface::BSplineSurface::from_nurb(
                n, false,
            ))
        };
        let broken = net_of(&[0.0, 0.5, 10.5, 11.0], vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0]);
        let joined = net_of(&[0.0, 0.5, 1.0], vec![0.0, 0.0, 0.5, 1.0, 1.0]);
        let tol = 1.0e-3;
        let drop = |s: &Surface, p: Vec3, d: Vec3| {
            let net = LineNet::of(&k, s).unwrap().unwrap();
            let solve = s.natural_uv_domain_with_nurb(&k).unwrap();
            let line = PassLine {
                point: p,
                vector: d,
                span: [0.0, 1.0],
                solve_domain: solve,
                tol,
            };
            let recs = vec![rec(FloorKind::NoRoot, [0.45, 0.5], [0.45, 0.55])];
            let mut leaves = [leaf_of(solve, Stop::Finished, Some(recs))];
            let sides = leaf_sides(&[solve], &solve, net.seams(&solve));
            let done =
                drop_fenced(&net, &line, &mut leaves, &sides, None, MAX_DROP_ENCLOSURES).unwrap();
            (leaves[0].floor.take().unwrap(), done)
        };
        assert_eq!(
            LineNet::of(&k, &broken).unwrap().unwrap().breaks,
            [vec![0.5], vec![]]
        );
        assert!(LineNet::of(&k, &joined).unwrap().unwrap().breaks[0].is_empty());
        let (near, down) = (Vec3::new(0.5005, 0.5, -1.0), Vec3::new(0.0, 0.0, 2.0));
        let (kept, done) = drop(&broken, near, down);
        assert_eq!(kept.len(), 1, "{done:?}");
        assert_eq!((done.no_root, done.enclosures), (0, 1));
        let (above, along) = (Vec3::new(-2.0, 0.5, 0.01), Vec3::new(4.0, 0.0, 0.0));
        let (kept, done) = drop(&broken, above, along);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!((done.no_root, done.enclosures), (1, 1));
        let (kept, done) = drop(&joined, above, along);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!((done.no_root, done.enclosures), (1, 0));
    }

    /// A crease side shared by two members is shown farther by its own enclosure or keeps
    /// the set: a line along y half a tolerance outside the cylinder's crease at u = π
    /// (x = −1) at height 0.5 passes within the tolerance of that knot line, and the two
    /// edge boxes on either side of it are kept; three tolerances outside it, both are
    /// dropped.
    #[test]
    fn a_crease_side_within_the_tolerance_keeps_its_set() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let c = LineNet::of(&k, &cylinder(&k)).unwrap().unwrap().creases[0][1];
        let pair = || {
            vec![
                rec(FloorKind::Edge, [c - 0.05, c], [0.45, 0.55]),
                rec(FloorKind::Edge, [c, c + 0.05], [0.45, 0.55]),
            ]
        };
        let d = Vec3::new(0.0, 2.0, 0.0);
        let at = |h: f64| Vec3::new(-1.0 - h * TOL, -1.0, 0.5);
        let (kept, done) = excluded(&k, at(0.5), d, whole, pair(), MAX_DROP_ENCLOSURES);
        assert_eq!(kept, pair());
        assert_eq!(done.edge, 0);
        let (kept, done) = excluded(&k, at(3.0), d, whole, pair(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(done.edge, 2);
    }

    /// A crease side no other member meets, with nothing but boxes excluded by distance
    /// across it, lies in closed excluded boxes and needs no enclosure: an edge box beside
    /// the crease at u = π half a tolerance from the line (the line of the test above) is
    /// dropped with no side enclosed, alone in the solve, and across a finished leaf with no
    /// record. Across a leaf whose pass did not run, the side must be enclosed, which fails,
    /// and the box is kept; and a box left at the floor that touches it only at a corner
    /// keeps it too.
    #[test]
    fn a_crease_side_alone_reads_the_boxes_across_it() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let c = LineNet::of(&k, &cylinder(&k)).unwrap().unwrap().creases[0][1];
        let edge = || vec![rec(FloorKind::Edge, [c - 0.05, c], [0.45, 0.55])];
        let (p, d) = (
            Vec3::new(-1.0 - 0.5 * TOL, -1.0, 0.5),
            Vec3::new(0.0, 2.0, 0.0),
        );
        let (kept, done) = excluded(&k, p, d, whole, edge(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!((done.edge, done.enclosures), (1, 0));

        let (a, b) = (
            Extent2d::from_bounds(1.0, 0.0, c, 1.0),
            Extent2d::from_bounds(c, 0.0, 4.0, 1.0),
        );
        let run = |across: SearchedLeaf| {
            let mut leaves = [leaf_of(a, Stop::Finished, Some(edge())), across];
            let done = drop_over(&k, p, d, &mut leaves, MAX_DROP_ENCLOSURES);
            (leaves[0].floor.clone().unwrap(), done)
        };
        let (kept, done) = run(leaf_of(b, Stop::Finished, Some(Vec::new())));
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!((done.edge, done.across, done.enclosures), (1, 1, 0));
        let (kept, done) = run(leaf_of(b, Stop::NotRun, Some(Vec::new())));
        assert_eq!(kept, edge());
        assert_eq!(done.enclosures, 1);

        let mut recs = edge();
        recs.push(rec(FloorKind::Left, [c, c + 0.05], [0.55, 0.6]));
        let (kept, _) = excluded(&k, p, d, whole, recs.clone(), MAX_DROP_ENCLOSURES);
        assert_eq!(kept, recs);
    }

    /// A box excluded by t is a record, and no proof of the gap to the line: a set it
    /// touches is kept.
    #[test]
    fn boxes_excluded_by_t_do_not_fence_a_set() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = (
            Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL),
            Vec3::new(4.0, 0.0, 0.0),
        );
        let recs = vec![
            rec(FloorKind::NoRoot, [1.0, 1.1], [0.4, 0.5]),
            rec(FloorKind::NoRoot, [1.1, 1.2], [0.4, 0.5]),
            FloorBox {
                t: Some(Ivl::new(2.0, 3.0)),
                ..rec(FloorKind::ExcludedByT, [1.2, 1.3], [0.4, 0.5])
            },
        ];
        let (kept, done) = excluded(&k, p, d, whole, recs.clone(), MAX_DROP_ENCLOSURES);
        assert_eq!(kept, recs);
        assert_eq!(done.no_root, 0);
    }

    /// A set on a leaf's border reads the leaf across it: across a finished leaf (or one at
    /// its limit, or a culled strip) with no record touching it, its every touching box is
    /// excluded, and it is dropped; across a leaf whose pass did not run, a leaf whose
    /// records the solve did not keep (no record there is then no proof), or a record that
    /// is not excluded, it is kept. A leaf whose records were not kept also leaves the
    /// components unbuilt.
    #[test]
    fn a_set_on_a_leaf_border_reads_the_leaf_across() {
        let k = Kernel::default();
        let (p, d) = (
            Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL),
            Vec3::new(4.0, 0.0, 0.0),
        );
        let (a, b) = (
            Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            Extent2d::from_bounds(1.0, 0.0, 2.0, 1.0),
        );
        let set = || {
            vec![
                rec(FloorKind::NoRoot, [1.0, 1.1], [0.4, 0.5]),
                rec(FloorKind::NoRoot, [1.1, 1.2], [0.4, 0.5]),
                rec(FloorKind::NoRoot, [1.1, 1.2], [0.5, 0.6]),
            ]
        };
        let run = |across: SearchedLeaf| {
            let mut leaves = [across, leaf_of(b, Stop::Finished, Some(set()))];
            let done = drop_over(&k, p, d, &mut leaves, MAX_DROP_ENCLOSURES);
            (leaves[1].floor.clone().unwrap(), done)
        };
        for stop in [Stop::Finished, Stop::AtLimit] {
            let (kept, done) = run(leaf_of(a, stop, Some(Vec::new())));
            assert!(kept.is_empty(), "{stop:?}: {kept:?}");
            assert_eq!((done.no_root, done.across), (3, 3), "{stop:?}");
            // (the side on u = 1 is read across, not enclosed)
            assert_eq!(done.enclosures, 0, "{stop:?}");
        }
        let strip = SearchedLeaf {
            strip: true,
            ..leaf_of(a, Stop::Finished, Some(Vec::new()))
        };
        let (kept, _) = run(strip);
        assert!(kept.is_empty(), "{kept:?}");

        let (kept, done) = run(leaf_of(a, Stop::NotRun, Some(Vec::new())));
        assert_eq!(kept, set());
        assert_eq!(done.no_root, 0);
        let (kept, done) = run(leaf_of(a, Stop::Finished, None));
        assert_eq!(kept, set());
        assert_eq!(done.no_root, 0);
        let left = vec![rec(FloorKind::Left, [0.9, 1.0], [0.4, 0.5])];
        let (kept, _) = run(leaf_of(a, Stop::Finished, Some(left)));
        assert_eq!(kept, set());

        let leaves = [
            leaf_of(a, Stop::Finished, None),
            leaf_of(b, Stop::Finished, Some(set())),
        ];
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let none = Seams::default();
        let c = components(
            &leaves,
            &leaf_sides(&[a, b], &whole, none),
            [0.0, 1.0],
            none,
        );
        assert!(c.over_budget);
        assert_eq!(
            side_record(&leaves, &c, &[], false, [0.0, 1.0]),
            SideRecord::Undecided(vec![[0.0, 1.0]])
        );
    }

    /// What lies across each leaf's sides is found through an index of the sides on each
    /// line ([`SideIndex`]), not by scanning every side on the line: over full strips in u
    /// stacked along v on a surface that wraps in u (every strip's sides u = u0 and u = u1
    /// on the seam's one line), a side's query visits at most three entries (itself across
    /// the seam and the strips it touches at its ends), where a scan visited all of them.
    /// The result is the scan's, entry for entry, on that tiling and on a mix of seams,
    /// duplicates, domains with no area, a signed zero and a NaN.
    #[test]
    fn leaf_sides_visit_only_the_sides_that_may_meet() {
        // the scan the index replaces: every side on the line, by (line, leaf)
        let scan = |domains: &[Extent2d], solve: &Extent2d, seams: Seams| -> Vec<LeafSides> {
            let all: Vec<[[f64; 2]; 2]> = domains.iter().map(bounds_of).collect();
            let [[r0, r1], [q0, q1]] = seams.range;
            let [wu, wv] = seams.wrap;
            let (lo, hi) = (solve.min(), solve.max());
            all.iter()
                .enumerate()
                .map(|(i, &bounds)| {
                    let [[u0, u1], [v0, v1]] = bounds;
                    let lines = [u0, u1, v0, v1];
                    let glued = [
                        (wu && u0 <= r0).then_some(r1),
                        (wu && u1 >= r1).then_some(r0),
                        (wv && v0 <= q0).then_some(q1),
                        (wv && v1 >= q1).then_some(q0),
                    ];
                    let mut across: [Vec<(usize, [f64; 2])>; 4] = Default::default();
                    for (s, slot) in across.iter_mut().enumerate() {
                        let seam = glued[s];
                        let line = seam.unwrap_or(lines[s]);
                        for (j, &[[a0, a1], [c0, c1]]) in all.iter().enumerate() {
                            let x = [a1, a0, c1, c0][s];
                            if x.total_cmp(&line).is_ne() {
                                continue;
                            }
                            let ([b0, b1], [o0, o1]) = if s < 2 {
                                ([c0, c1], [v0, v1])
                            } else {
                                ([a0, a1], [u0, u1])
                            };
                            let area = a0 < a1 && c0 < c1;
                            if (j != i || seam.is_some()) && area && b0 <= o1 && b1 >= o0 {
                                slot.push((j, [b0, b1]));
                            }
                        }
                        slot.sort_by(|(i, [a, _]), (j, [b, _])| a.total_cmp(b).then(i.cmp(j)));
                    }
                    let [g0, g1, g2, g3] = glued.map(|g| g.is_some());
                    LeafSides {
                        bounds,
                        inside: [
                            u0 > lo.x || g0,
                            u1 < hi.x || g1,
                            v0 > lo.y || g2,
                            v1 < hi.y || g3,
                        ],
                        across,
                        seam: [g0, g1, g2, g3],
                    }
                })
                .collect()
        };
        let whole = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let wrap_u = Seams {
            range: [[0.0, 1.0], [0.0, 1.0]],
            wrap: [true, false],
        };
        let n = 2000;
        let strips: Vec<Extent2d> = (0..n)
            .map(|i| {
                let (a, b) = (f64::from(i) / f64::from(n), f64::from(i + 1) / f64::from(n));
                Extent2d::from_bounds(0.0, a, 1.0, b)
            })
            .collect();
        let all: Vec<[[f64; 2]; 2]> = strips.iter().map(bounds_of).collect();
        let u1s = SideIndex::of(&all, |&[[_, u1], _]| u1, |&[_, v]| v);
        for &[_, v] in &all {
            let visited = u1s.candidates(1.0, v).len();
            assert!((1..=3).contains(&visited), "{v:?}: {visited}");
        }
        assert_eq!(
            leaf_sides(&strips, &whole, wrap_u),
            scan(&strips, &whole, wrap_u)
        );
        let mixed = [
            Extent2d::from_bounds(0.0, 0.0, 0.5, 0.5),
            Extent2d::from_bounds(0.5, 0.0, 1.0, 0.25),
            Extent2d::from_bounds(0.5, 0.25, 1.0, 1.0),
            Extent2d::from_bounds(0.0, 0.5, 0.5, 1.0),
            Extent2d::from_bounds(0.0, 0.5, 0.5, 1.0),
            Extent2d::from_bounds(0.25, 0.5, 0.25, 0.75),
            Extent2d::from_bounds(-0.0, 0.0, 0.5, 0.5),
            Extent2d::from_bounds(0.5, f64::NAN, 1.0, 0.5),
            Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ];
        for wrap in [[false, false], [true, false], [false, true], [true, true]] {
            let seams = Seams {
                range: [[0.0, 1.0], [0.0, 1.0]],
                wrap,
            };
            // (by their debug forms: a NaN bound is never equal to itself)
            assert_eq!(
                format!("{:?}", leaf_sides(&mixed, &whole, seams)),
                format!("{:?}", scan(&mixed, &whole, seams)),
                "{wrap:?}"
            );
        }
    }

    /// A line tangent to the cylinder leaves boxes with no root about the contact, which
    /// the exclusion phase cuts. Its budget running out keeps what it has not decided: the
    /// pass stays finished, the phase spent exactly its budget, and the boxes with no root
    /// it kept hold every piece a phase with its whole budget kept.
    #[test]
    fn an_exclusion_out_of_budget_keeps_its_boxes_and_the_pass_finished() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = across(1.0);
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: domain,
            tol: TOL,
        };
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let sides = LeafSides::whole_solve(&domain);
        let run =
            |budget| isolate_within(&net, &line, &domain, &interval, &sides, None, budget).unwrap();
        let no_root = |f: &Isolation| -> Vec<FloorBox> {
            f.floor
                .iter()
                .filter(|r| matches!(r.kind, FloorKind::NoRoot | FloorKind::Edge))
                .copied()
                .collect()
        };
        let whole = run(MAX_EXCLUSION_BOXES);
        assert_eq!(whole.stop, Stop::Finished);
        assert!(whole.exclusion.ran && !whole.exclusion.out_of_budget);
        assert!(whole.exclusion.boxes > 2);
        for budget in [0, 2] {
            let short = run(budget);
            assert_eq!(short.stop, Stop::Finished);
            assert!(short.exclusion.out_of_budget);
            assert_eq!(short.exclusion.boxes, budget);
            assert_eq!(short.roots, whole.roots);
            // every piece the whole phase kept lies in a box the short one kept
            let kept = no_root(&short);
            assert!(!kept.is_empty());
            for r in no_root(&whole) {
                assert!(
                    kept.iter().any(|o| within(r.u, o.u) && within(r.v, o.v)),
                    "budget {budget}: {r:?}"
                );
            }
        }
    }

    /// A periodic seam counts as a border of a box: a crossing a ten thousandth of a turn
    /// from the cylinder's seam leaves boxes with no root on the seam, and every one of them
    /// is an edge box. The cylinder's net closes exactly in u and the solve covers its whole
    /// knots' range, so the seam is a knot line of the face, a crease (module docs, "The
    /// seam"); over a solve covering part of the range it stays a boundary.
    #[test]
    fn a_seam_side_counts_as_a_border() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (u0, u1) = (domain.min().x, domain.max().x);
        let a = 1.0e-4 * std::f64::consts::TAU;
        let found = pass(
            &k,
            &s,
            Vec3::new(-2.0, det::sin(a), 0.5),
            Vec3::new(4.0, 0.0, 0.0),
            [0.0, 1.0],
            TOL,
        );
        assert_eq!(found.stop, Stop::Finished);
        let on_seam: Vec<&FloorBox> = found
            .floor
            .iter()
            .filter(|r| matches!(r.kind, FloorKind::NoRoot | FloorKind::Edge))
            .filter(|r| r.u.lo == u0 || r.u.hi == u1)
            .collect();
        assert!(!on_seam.is_empty(), "{:?}", found.floor);
        assert!(
            on_seam.iter().all(|r| r.kind == FloorKind::Edge),
            "{on_seam:?}"
        );
        // and the side on it is a crease's (the net closes, the solve is the whole range)
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let b = Scratch {
            u: Ivl::new(u0, 0.1),
            v: Ivl::new(0.4, 0.6),
        };
        let kinds = net.side_kinds(&domain, &LeafSides::whole_solve(&domain), b);
        assert_eq!(
            kinds,
            [
                SideKind::Crease,
                SideKind::Plain,
                SideKind::Plain,
                SideKind::Plain
            ]
        );
        assert_eq!(net.seams(&domain).wrap, [true, false]);
        let part = Extent2d::from_bounds(u0, 0.0, 0.5 * (u0 + u1), 1.0);
        assert_eq!(net.seams(&part).wrap, [false, false]);
        let kinds = net.side_kinds(&part, &LeafSides::whole_solve(&part), b);
        assert_eq!(
            kinds,
            [
                SideKind::Boundary,
                SideKind::Plain,
                SideKind::Plain,
                SideKind::Plain
            ]
        );
    }

    /// The passes of `doms` (leaves of the solve over the surface's whole domain) on the line
    /// `p + t d`, `t` in `[0, 1]`, the solve's drop over them, and their components.
    fn components_of(
        k: &Kernel,
        s: &Surface,
        p: Vec3,
        d: Vec3,
        tol: f64,
        doms: &[Extent2d],
    ) -> (Vec<SearchedLeaf>, Components) {
        let net = LineNet::of(k, s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(k).unwrap();
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: solve,
            tol,
        };
        let seams = net.seams(&solve);
        let sides = leaf_sides(doms, &solve, seams);
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let mut leaves: Vec<SearchedLeaf> = doms
            .iter()
            .zip(&sides)
            .map(|(dom, own)| {
                let f = isolate(&net, &line, dom, &interval, own, None).unwrap();
                SearchedLeaf {
                    domain: *dom,
                    stop: f.stop,
                    floor: Some(f.floor),
                    roots: f.roots,
                    strip: false,
                    inside: true,
                }
            })
            .collect();
        drop_fenced(&net, &line, &mut leaves, &sides, None, MAX_DROP_ENCLOSURES).unwrap();
        let c = components(&leaves, &sides, [0.0, 1.0], seams);
        (leaves, c)
    }

    /// A line half the tolerance from the cylinder at a quadrant (u = π/2, a crease of its
    /// net) misses it near t = 0.5, with no root. Split into two leaves at that quadrant,
    /// the boxes it leaves on both sides are one component across the two leaves, holding
    /// boxes left at the floor, its line parameters about 0.5, and nothing undecided
    /// around it but itself. With the second leaf not searched (as a leaf outside the face,
    /// which no strip covers), the boxes on the quadrant face it: they are edge boxes, and
    /// the component touches a stretch nothing searched.
    #[test]
    fn components_join_across_leaf_borders() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        // the net's first crease, the quarter knot at u = π/2
        let q = LineNet::of(&k, &s).unwrap().unwrap().creases[0][0];
        let (p, d) = across(1.0 + 0.5 * TOL);
        let doms = [
            Extent2d::from_bounds(0.0, 0.0, q, 1.0),
            Extent2d::from_bounds(q, 0.0, whole.max().x, 1.0),
        ];
        let (leaves, c) = components_of(&k, &s, p, d, TOL, &doms);
        assert!(leaves.iter().all(|l| l.stop == Stop::Finished));
        assert!(!c.over_budget);
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        let one = &c.list[0];
        let from: std::collections::BTreeSet<usize> =
            one.boxes.iter().map(|&i| c.boxes[i].leaf).collect();
        assert_eq!(from.len(), 2);
        assert!(one.holds.left > 0);
        assert!(one.roots.is_empty());
        assert!(one.t.lo <= 0.5 && 0.5 <= one.t.hi, "{:?}", one.t);
        assert!(!one.reaches_end);
        assert_eq!(one.touches, Touches::default());
        assert_eq!(one.contact, Contact::Undecided);
        assert_eq!(
            side_record(&leaves, &c, &[], false, [0.0, 1.0]),
            SideRecord::Undecided(vec![[one.t.lo, one.t.hi]])
        );

        let (leaves, c) = components_of(&k, &s, p, d, TOL, &doms[..1]);
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        assert!(c.list[0].touches.unsearched);
        let floor = leaves[0].floor.clone().unwrap();
        let mut on_quadrant = floor.iter().filter(|r| r.u.hi == q);
        assert!(on_quadrant.clone().count() > 0);
        assert!(on_quadrant.all(|r| matches!(r.kind, FloorKind::Edge | FloorKind::Left)));
        // (answer10, Q2: facing the stretch nothing searched, a trim cull, the component
        // reaches a boundary side; with both leaves searched it reaches none, and in a leaf
        // the trim crosses, not wholly inside the face, it does)
        let (_, c) = typed_of(&k, &s, p, d, TOL, &doms[..1]);
        assert!(c.list.iter().all(|x| x.at_boundary), "{:?}", c.list);
        let (mut leaves, mut c) = typed_of(&k, &s, p, d, TOL, &doms);
        assert!(!c.list[0].at_boundary, "{:?}", c.list[0]);
        leaves[0].inside = false;
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: whole,
            tol: TOL,
        };
        let sides = leaf_sides(&doms, &whole, net.seams(&whole));
        type_components(&net, &line, &leaves, &sides, &mut c, None).unwrap();
        assert!(c.list[0].at_boundary, "{:?}", c.list[0]);
    }

    /// A line tangent to the torus's crown circle at 45°, tolerance 1e-6, touches it once.
    /// The pass stops at its limit there (the fold is singular along the crown), and what it
    /// kept, the boxes it did not visit among them, is one component: in a leaf at the
    /// limit, about the contact at t = 0.5, and undecided. With a tolerance of 1e-2 the
    /// pass finishes, and the component is touching by the side function (the line stays
    /// outside the tube at both ends).
    #[test]
    fn the_crown_tangency_at_45_degrees_is_one_component() {
        let k = Kernel::default();
        let s = torus(&k);
        let c = 30.0 / 2.0_f64.sqrt();
        let along = Vec3::new(-1.0, 1.0, 0.0) * (1.0 / 2.0_f64.sqrt());
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (leaves, comps) = components_of(
            &k,
            &s,
            Vec3::new(c, c, 5.0) - 10.0 * along,
            20.0 * along,
            1.0e-6,
            &[whole],
        );
        assert_eq!(leaves[0].stop, Stop::AtLimit);
        assert_eq!(comps.list.len(), 1, "{:?}", comps.list);
        let one = &comps.list[0];
        assert!(one.touches.unfinished);
        assert!(one.holds.unvisited > 0);
        assert!(one.t.lo <= 0.5 && 0.5 <= one.t.hi, "{:?}", one.t);
        let (_, typed) = typed_of(
            &k,
            &s,
            Vec3::new(c, c, 5.0) - 10.0 * along,
            20.0 * along,
            1.0e-6,
            &[whole],
        );
        assert!(typed
            .list
            .iter()
            .all(|x| x.contact == Contact::Undecided && x.typed != Typed::Sides));
        let (leaves, typed) = typed_of(
            &k,
            &s,
            Vec3::new(c, c, 5.0) - 10.0 * along,
            20.0 * along,
            1.0e-2,
            &[whole],
        );
        assert_eq!(leaves[0].stop, Stop::Finished);
        assert_eq!(types(&typed), vec![(Contact::Touching, Typed::Sides)]);
    }

    /// A culled strip the line passes farther than the tolerance from is excluded by
    /// distance whole: the quarter of the cylinder from u = π to 3π/2 (x and y below 0) and
    /// the line across the cylinder at y = 1 + 2 tol, at least 1 from it. The strip finishes
    /// with no record, and as a searched leaf with no record it leaves the side record
    /// complete.
    #[test]
    fn a_culled_strip_excluded_by_distance_reads_as_searched() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (c1, c2) = (net.creases[0][1], net.creases[0][2]);
        let quarter = Extent2d::from_bounds(c1, 0.0, c2, 1.0);
        let (p, d) = across(1.0 + 2.0 * TOL);
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: whole,
            tol: TOL,
        };
        let found = enclose_strip(&net, &line, &quarter, None).unwrap();
        assert_eq!(found.stop, Stop::Finished);
        assert!(found.floor.is_empty(), "{:?}", found.floor);
        assert!(found.roots.is_empty());
        let leaves = [SearchedLeaf {
            strip: true,
            ..leaf_of(quarter, found.stop, Some(found.floor))
        }];
        let seams = net.seams(&whole);
        let c = components(
            &leaves,
            &leaf_sides(&[quarter], &whole, seams),
            [0.0, 1.0],
            seams,
        );
        assert!(c.list.is_empty());
        assert_eq!(
            side_record(&leaves, &c, &[], false, [0.0, 1.0]),
            SideRecord::Complete
        );
    }

    /// A culled strip that runs out of its budget keeps what it left undecided with its line
    /// parameters: the quarter of the cylinder from u = 0 to π/2 (x and y above 0), whose
    /// feet on the line across the cylinder at y = 0.5 lie in t = (x + 1) / 2, [0.5, 1],
    /// with the line's own interval [−1, 2]. Stopped after three enclosures, it is at its
    /// limit, every box it kept is undecided with its line parameters, and the side record
    /// is undecided only inside the line parameters of the quarter's enclosure (about
    /// [0.5, 1], widened by the tolerance and the enclosure), not over the whole line.
    /// With its whole budget it finishes, keeping only boxes about the crossing at
    /// x = √0.75, t = (1 + √0.75) / 2.
    #[test]
    fn a_culled_strip_out_of_budget_is_undecided_over_its_boxes_only() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let quarter = Extent2d::from_bounds(0.0, 0.0, net.creases[0][0], 1.0);
        let (p, d) = across(0.5);
        let span = [-1.0, 2.0];
        let line = PassLine {
            point: p,
            vector: d,
            span,
            solve_domain: whole,
            tol: TOL,
        };
        let found = enclose_strip_within(&net, &line, &quarter, None, 3).unwrap();
        assert_eq!(found.stop, Stop::AtLimit);
        assert_eq!(found.boxes, 3);
        assert!(!found.floor.is_empty());
        assert!(found
            .floor
            .iter()
            .all(|r| r.kind == FloorKind::Culled && r.t.is_some()));
        let leaves = [SearchedLeaf {
            strip: true,
            ..leaf_of(quarter, found.stop, Some(found.floor))
        }];
        let seams = net.seams(&whole);
        let c = components(&leaves, &leaf_sides(&[quarter], &whole, seams), span, seams);
        let record = side_record(&leaves, &c, &[], false, span);
        let SideRecord::Undecided(spans) = &record else {
            panic!("{record:?}");
        };
        // (inside the line parameters of the quarter's own enclosure, which lie inside the
        // line's interval)
        let scratch = ScratchLine::of(&line, &Extent1d::from_min_max(-1.0, 2.0)).unwrap();
        let (u, v) = (Ivl::new(0.0, net.creases[0][0]), Ivl::new(0.0, 1.0));
        let (_, feet) = scratch.feet(&enclosed(&net, &scratch, u, v).unwrap().r);
        assert!(feet.lo > -1.0 && feet.hi < 2.0, "{feet:?}");
        for &[lo, hi] in spans {
            assert!(lo >= feet.lo && hi <= feet.hi, "{spans:?} {feet:?}");
        }

        let done = enclose_strip(&net, &line, &quarter, None).unwrap();
        assert_eq!(done.stop, Stop::Finished);
        assert!(!done.floor.is_empty());
        let at = (1.0 + 0.75_f64.sqrt()) / 2.0;
        for r in &done.floor {
            assert_eq!(r.kind, FloorKind::Culled, "{r:?}");
            assert!(r.t.is_some_and(|t| (t.mid() - at).abs() < 1.0e-2), "{r:?}");
        }
    }

    /// The side record: not run when no leaf's pass ran; complete with no component and
    /// every cull's strip placed; undecided over each component's line parameters, and over
    /// the whole of the line's interval with a cull whose strip could not be placed.
    #[test]
    fn the_side_record_has_three_states() {
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let leaf = |stop| leaf_of(domain, stop, Some(Vec::new()));
        let span = [0.0, 1.0];
        let none = Components::default();
        assert_eq!(
            side_record(&[leaf(Stop::NotRun)], &none, &[], true, span),
            SideRecord::NotRun
        );
        assert_eq!(
            side_record(&[leaf(Stop::Finished)], &none, &[], false, span),
            SideRecord::Complete
        );
        assert_eq!(
            side_record(&[leaf(Stop::Finished)], &none, &[], true, span),
            SideRecord::Undecided(vec![span])
        );
        assert_eq!(
            side_record(
                &[leaf(Stop::Finished)],
                &Components::over_budget(),
                &[],
                false,
                span
            ),
            SideRecord::Undecided(vec![span])
        );
        // a leaf not run among others is a component over the whole line
        let leaves = [leaf(Stop::Finished), leaf(Stop::NotRun)];
        let flat = Seams::default();
        let c = components(
            &leaves,
            &leaf_sides(&[domain, domain], &domain, flat),
            span,
            flat,
        );
        assert_eq!(c.list.len(), 1);
        assert!(c.list[0].touches.not_run);
        assert_eq!(
            side_record(&leaves, &c, &[], false, span),
            SideRecord::Undecided(vec![span])
        );
        // (a leaf whose pass stopped at its limit beside one not run: undecided too)
        let leaves = [leaf(Stop::AtLimit), leaf(Stop::NotRun)];
        let c = components(
            &leaves,
            &leaf_sides(&[domain, domain], &domain, flat),
            span,
            flat,
        );
        assert_eq!(
            side_record(&leaves, &c, &[], false, span),
            SideRecord::Undecided(vec![span])
        );

        // every solved leaf not run: not run, whatever its strips found (a strip that
        // finished with a box kept, one with none, one at its limit) and with a cull unplaced
        let strip = |stop, floor: Vec<FloorBox>| SearchedLeaf {
            strip: true,
            ..leaf_of(domain, stop, Some(floor))
        };
        let kept = vec![FloorBox {
            t: Some(Ivl::new(0.4, 0.6)),
            ..rec(FloorKind::Culled, [0.2, 0.3], [0.2, 0.3])
        }];
        let leaves = [
            leaf(Stop::NotRun),
            strip(Stop::Finished, kept.clone()),
            strip(Stop::Finished, Vec::new()),
            strip(Stop::AtLimit, kept.clone()),
        ];
        let sides = leaf_sides(&[domain; 4], &domain, flat);
        let c = components(&leaves, &sides, span, flat);
        assert!(!c.list.is_empty());
        for unplaced in [false, true] {
            assert_eq!(
                side_record(&leaves, &c, &[], unplaced, span),
                SideRecord::NotRun
            );
        }

        // no solved leaf (every leaf culled): the strips decide
        let only = |strips: &[SearchedLeaf], unplaced: bool| {
            let sides = leaf_sides(&vec![domain; strips.len()], &domain, flat);
            side_record(
                strips,
                &components(strips, &sides, span, flat),
                &[],
                unplaced,
                span,
            )
        };
        assert_eq!(
            only(&[strip(Stop::Finished, Vec::new())], false),
            SideRecord::Complete
        );
        assert_eq!(
            only(&[strip(Stop::Finished, Vec::new())], true),
            SideRecord::Undecided(vec![span])
        );
        let SideRecord::Undecided(spans) = only(&[strip(Stop::AtLimit, kept.clone())], false)
        else {
            panic!("a strip's kept box is undecided");
        };
        assert_eq!(spans, vec![[0.4, 0.6]]);
        assert_eq!(
            only(&[strip(Stop::NotRun, Vec::new())], false),
            SideRecord::NotRun
        );
        // (a strip not run beside one that ran is nothing known: undecided over the line)
        assert_eq!(
            only(
                &[
                    strip(Stop::NotRun, Vec::new()),
                    strip(Stop::Finished, Vec::new())
                ],
                false
            ),
            SideRecord::Undecided(vec![span])
        );
    }

    /// [`components_of`] with the components typed ([`type_components`]).
    fn typed_of(
        k: &Kernel,
        s: &Surface,
        p: Vec3,
        d: Vec3,
        tol: f64,
        doms: &[Extent2d],
    ) -> (Vec<SearchedLeaf>, Components) {
        let net = LineNet::of(k, s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(k).unwrap();
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: solve,
            tol,
        };
        let (leaves, mut c) = components_of(k, s, p, d, tol, doms);
        let sides = leaf_sides(doms, &solve, net.seams(&solve));
        type_components(&net, &line, &leaves, &sides, &mut c, None).unwrap();
        (leaves, c)
    }

    /// [`typed_of`] with the line's own interval `span` (and the leaves' line interval).
    fn typed_in(k: &Kernel, s: &Surface, p: Vec3, d: Vec3, tol: f64, span: [f64; 2]) -> Components {
        let net = LineNet::of(k, s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(k).unwrap();
        let line = PassLine {
            point: p,
            vector: d,
            span,
            solve_domain: solve,
            tol,
        };
        let doms = [solve];
        let seams = net.seams(&solve);
        let sides = leaf_sides(&doms, &solve, seams);
        let [s0, s1] = span;
        let interval = Extent1d::from_min_max(s0, s1);
        let mut leaves: Vec<SearchedLeaf> = doms
            .iter()
            .zip(&sides)
            .map(|(dom, own)| {
                let f = isolate(&net, &line, dom, &interval, own, None).unwrap();
                SearchedLeaf {
                    domain: *dom,
                    stop: f.stop,
                    floor: Some(f.floor),
                    roots: f.roots,
                    strip: false,
                    inside: true,
                }
            })
            .collect();
        drop_fenced(&net, &line, &mut leaves, &sides, None, MAX_DROP_ENCLOSURES).unwrap();
        let mut c = components(&leaves, &sides, span, seams);
        type_components(&net, &line, &leaves, &sides, &mut c, None).unwrap();
        c
    }

    /// A component reaching an end of the line's own interval (answer8, Q2), on the sphere of
    /// radius 5 about the origin and the secant from (−10, 1, 2) along (20, 0, 0), which
    /// crosses at `t = (10 ∓ √20) / 20` (0.27639 and 0.72361) at the cosine `√20 / 5 = 0.894`
    /// to the normal. With the interval ending `10⁻⁶` past the second crossing, the end
    /// `E` lies `2·10⁻⁵` along the line past it, about `1.8·10⁻⁵` from the sphere, more than
    /// the tolerance 10⁻⁶: the component reaching the end is a crossing by its count (its
    /// root's enclosure inside the interval, the end certified off the net). Ending `10⁻¹⁰`
    /// past it, `E` is within `2·10⁻⁹` of the sphere, and the component reaching the end is
    /// undecided.
    #[test]
    fn a_component_reaching_the_line_s_end_is_typed_with_its_end_certified() {
        let k = Kernel::default();
        let s = sphere(&k);
        let (p, d) = (Vec3::new(-10.0, 1.0, 2.0), Vec3::new(20.0, 0.0, 0.0));
        let second = (10.0 + 20.0_f64.sqrt()) / 20.0;
        let c = typed_in(&k, &s, p, d, 1.0e-6, [0.0, second + 1.0e-6]);
        let at_end: Vec<&Component> = c.list.iter().filter(|x| x.reaches_end).collect();
        assert_eq!(at_end.len(), 1, "{:?}", c.list);
        assert_eq!(
            (at_end[0].contact, at_end[0].typed),
            (Contact::Crossing, Typed::Count(1)),
            "{:?}",
            at_end[0]
        );
        let c = typed_in(&k, &s, p, d, 1.0e-6, [0.0, second + 1.0e-10]);
        let at_end: Vec<&Component> = c.list.iter().filter(|x| x.reaches_end).collect();
        assert_eq!(at_end.len(), 1, "{:?}", c.list);
        assert_eq!(
            at_end[0].typed,
            Typed::Undecided(Why::ReachesEnd),
            "{:?}",
            at_end[0]
        );
        // (its root counted: the end, not the root, leaves it undecided)
        assert_eq!(at_end[0].classes.len(), 1, "{:?}", at_end[0]);
    }

    /// The types of the components, in order.
    fn types(c: &Components) -> Vec<(Contact, Typed)> {
        c.list.iter().map(|x| (x.contact, x.typed)).collect()
    }

    /// The side function of each kind of surface of revolution vanishes on its net and has
    /// one sign on each side of it: a sphere, a cone, a cylinder and a torus placed off the
    /// origin and tilted, at points of their nets and a hundredth along the normal.
    #[test]
    fn the_side_function_vanishes_on_the_net() {
        let k = Kernel::default();
        let (o, x, y) = (
            Vec3::new(1.0, -2.0, 3.0),
            Vec3::new(0.6, 0.8, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let mut sphere = crate::surface::sphere::Sphere::new(
            &k,
            o,
            x,
            y,
            Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
            2.5,
            crate::surface::sphere::SphereOptions::default(),
        )
        .unwrap();
        sphere.make_nurb(&k).unwrap();
        let mut cone = Cone::new_bounded(o, x, y, 1.5, 0.8, 0.0, 300.0, 2.0, false, false, false);
        cone.make_nurb(&k).unwrap();
        let mut cylinder =
            Cone::new_cylinder_bounded(o, x, y, 1.5, 0.0, 300.0, 2.0, false, false, true);
        cylinder.make_nurb(&k).unwrap();
        let mut torus = Torus::new(
            &k,
            o,
            x,
            y,
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            4.0,
            1.0,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(&k).unwrap();
        for s in [
            Surface::from(sphere),
            Surface::from(cone),
            Surface::from(cylinder),
            Surface::from(torus),
        ] {
            let net = LineNet::of(&k, &s).unwrap().unwrap();
            let rev = net.revolved.expect("a surface of revolution");
            let [[u0, u1], [v0, v1]] = net.range;
            for (i, j) in [(1, 1), (3, 2), (5, 4), (7, 3)] {
                let (u, v) = (
                    u0 + (u1 - u0) * f64::from(i) / 8.0,
                    v0 + (v1 - v0) * f64::from(j) / 5.0,
                );
                let on = net.at(u, v, rev.origin).unwrap();
                let f = rev.side(&on);
                assert!(f.mag() < 1.0e-12, "{s:?}: {f:?}");
                let [at, s_u, s_v] = net.over(Ivl::from(u), Ivl::from(v), rev.origin).unwrap();
                let point = v3(mids(&at)) + v3(rev.origin);
                let normal = v3(mids(&s_u)).cross(v3(mids(&s_v))).unitize_or_z();
                let off = |e: f64| {
                    let q = point + e * normal;
                    let r = q - v3(rev.origin);
                    rev.side(&[r.x, r.y, r.z].map(Ivl::from))
                };
                let (a, b) = (off(0.01), off(-0.01));
                assert!(
                    (a.lo > 0.0 && b.hi < 0.0) || (a.hi < 0.0 && b.lo > 0.0),
                    "{a:?} {b:?}"
                );
            }
        }
    }

    /// The certified bound of the net from its closed form ([`side_bounds`], [`side_eps`]):
    /// on the placed sphere, cone, cylinder and torus of the side function's test, every
    /// Bézier patch has a bound, the patches tile the net's range, and `ε` over the whole
    /// net is at the rounding of a net of size 5, widened by the interval products of its
    /// equation (the torus's of degree 4: measured 3.6e-10), below 1e-8 (a hundredth of the
    /// tolerance 1e-6 the solves read). A bound is never below a value `F` takes: against the sphere of radius
    /// `r' = r (1 + 10⁻⁶)` the net's points (on the sphere of radius `r` to rounding) have
    /// `|F| = r'² − r² ≈ 2 r² 10⁻⁶ = 1.25e-5`, so every patch's bound is at least 1.2e-5.
    #[test]
    fn the_net_is_within_a_certified_bound_of_its_closed_form() {
        let k = Kernel::default();
        let (o, x, y) = (
            Vec3::new(1.0, -2.0, 3.0),
            Vec3::new(0.6, 0.8, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let mut sphere = crate::surface::sphere::Sphere::new(
            &k,
            o,
            x,
            y,
            Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
            2.5,
            crate::surface::sphere::SphereOptions::default(),
        )
        .unwrap();
        sphere.make_nurb(&k).unwrap();
        let mut cone = Cone::new_bounded(o, x, y, 1.5, 0.8, 0.0, 300.0, 2.0, false, false, false);
        cone.make_nurb(&k).unwrap();
        let mut cylinder =
            Cone::new_cylinder_bounded(o, x, y, 1.5, 0.0, 300.0, 2.0, false, false, true);
        cylinder.make_nurb(&k).unwrap();
        let mut torus = Torus::new(
            &k,
            o,
            x,
            y,
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            4.0,
            1.0,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(&k).unwrap();
        let sphere = Surface::from(sphere);
        for s in [
            sphere.clone(),
            Surface::from(cone),
            Surface::from(cylinder),
            Surface::from(torus),
        ] {
            let net = LineNet::of(&k, &s).unwrap().unwrap();
            let rev = net.revolved.expect("a surface of revolution");
            let bounds = net.side_bounds(None).unwrap();
            assert!(!bounds.is_empty(), "{s:?}");
            assert!(bounds.iter().all(|b| b.residual.is_some()), "{bounds:?}");
            let [[u0, u1], [v0, v1]] = net.range;
            let area: f64 = bounds
                .iter()
                .map(|b| (b.span[0][1] - b.span[0][0]) * (b.span[1][1] - b.span[1][0]))
                .sum();
            assert!(((u1 - u0) * (v1 - v0) - area).abs() < 1.0e-12, "{bounds:?}");
            let eps = side_eps(&net, &rev, &bounds, Ivl::new(u0, u1), Ivl::new(v0, v1)).unwrap();
            assert!(eps < 1.0e-8, "{eps}");
        }
        let net = LineNet::of(&k, &sphere).unwrap().unwrap();
        let rev = net.revolved.expect("a sphere");
        let off = Revolution::Sphere {
            centre: v3(rev.origin),
            radius: 2.5 * (1.0 + 1.0e-6),
        };
        let bounds = side_bounds(net.form.as_ref().unwrap(), &off, None).unwrap();
        assert!(!bounds.is_empty());
        for b in &bounds {
            assert!(b.residual.unwrap() >= 1.2e-5, "{b:?}");
        }
    }

    /// A net fitted to a definition is not its surface, and gets no pass: a B-spline net
    /// carrying a swept wall's definition (a screw motion, its bound above 0) is refused, and
    /// so is the evaluation copy a surface cache holds of it, which drops the definition but
    /// keeps that its net is fitted, and so is a copy or a piece of that copy, and the plane
    /// its copy with analytics recognises (the net is planar). The same net with no
    /// definition (a plain B-spline: its net is the surface), its evaluation copy and their
    /// copies keep the pass.
    #[test]
    fn a_net_fitted_to_a_definition_gets_no_pass() {
        use crate::nurbs::b_spline_surface::SurfParamType;
        use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
        let k = Kernel::default();
        let bare = Surface::from(
            crate::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::new(4.0, 0.0, 0.0),
                Vec3::new(5.0, 0.0, 0.0),
                Vec3::new(4.0, 1.0, 1.0),
                Vec3::new(5.0, 1.0, 1.0),
            )
            .unwrap(),
        );
        let line = crate::nurbs::b_spline_curve::BSplineCurve::create_line_segment(
            3,
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(5.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 1.5, None).unwrap();
        let sweep =
            ProceduralSweep::create(&line, motion, &[0.0, 0.5, 1.0], &[0.25, 0.25]).unwrap();
        assert!(sweep.bound() > 0.0);
        let mut swept = bare.clone();
        swept.set_sweep_definition(sweep);
        assert!(LineNet::of(&k, &bare).unwrap().is_some());
        assert!(LineNet::of(&k, &bare.clone_for_evaluation().unwrap())
            .unwrap()
            .is_some());
        assert!(swept.net_is_fitted());
        assert!(LineNet::of(&k, &swept).unwrap().is_none());
        let copy = swept.clone_for_evaluation().unwrap();
        assert!(copy.procedural_definition().is_none());
        assert!(copy.net_is_fitted());
        assert!(LineNet::of(&k, &copy).unwrap().is_none());
        assert!(LineNet::of(&k, &copy.clone_for_evaluation().unwrap())
            .unwrap()
            .is_none());
        // a copy (or a copy with analytics) of the evaluation copy is still a fitted net, and
        // gets no pass; a copy of the plain B-spline, or of its evaluation copy, is its net
        let (left, right) = copy.split_at(&k, 0.5, SurfParamType::U).unwrap();
        for again in [
            copy.copy(&k).unwrap(),
            copy.copy_and_add_analytics(&k).unwrap(),
            left,
            right,
        ] {
            assert!(again.net_is_fitted());
            assert!(LineNet::of(&k, &again).unwrap().is_none());
        }
        // the swept wall's net is planar: its copy with analytics is a plane, which keeps no
        // sweep, but keeps that it is made from the fitted net
        let plane = swept.copy_and_add_analytics(&k).unwrap();
        assert!(matches!(
            plane.kind(),
            crate::surface::surface::SurfaceKind::Plane(_)
        ));
        assert!(plane.procedural_definition().is_none());
        assert!(plane.net_is_fitted());
        assert!(LineNet::of(&k, &plane).unwrap().is_none());
        for plain in [
            bare.copy(&k).unwrap(),
            bare.clone_for_evaluation().unwrap().copy(&k).unwrap(),
        ] {
            assert!(!plain.net_is_fitted());
            assert!(LineNet::of(&k, &plain).unwrap().is_some());
        }
    }

    /// The side bounds poll the solve's cancel token before each Bézier patch: a token
    /// already cancelled stops them with the cancel (never an empty list, which reads as no
    /// bound), a fuel budget that runs out between patches stops them with its own error,
    /// and a token that only counts sees one poll a patch and changes no bound.
    #[test]
    fn the_side_bounds_stop_at_a_cancel() {
        let k = Kernel::default();
        let mut torus = Torus::new(
            &k,
            Vec3::new(1.0, -2.0, 3.0),
            Vec3::new(0.6, 0.8, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            4.0,
            1.0,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(&k).unwrap();
        let net = LineNet::of(&k, &Surface::from(torus)).unwrap().unwrap();
        let plain = net.side_bounds(None).unwrap();
        assert!(plain.len() > 1, "{plain:?}");
        let counting = CancelToken::after_polls(u64::MAX);
        let counted = net.side_bounds(Some(&counting)).unwrap();
        assert_eq!(counted, plain);
        assert_eq!(counting.polls(), Some(plain.len() as u64));
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let r = net.side_bounds(Some(&cancelled));
        assert!(
            matches!(r, Err(crate::SolidError::Cancelled { .. })),
            "{r:?}"
        );
        let fuel = CancelToken::with_fuel(1);
        let r = net.side_bounds(Some(&fuel));
        assert!(
            matches!(r, Err(crate::SolidError::OutOfFuel { .. })),
            "{r:?}"
        );
    }

    /// A line tangent to the sphere touches it: one component of boxes left at the floor,
    /// typed by the side function at its ends (both outside the sphere). Missing it within
    /// the tolerance is touching too. A secant crosses it twice: two components, each a
    /// count of one certified root. A line along the axis through both poles meets the
    /// sphere at each pole: two components, each turning about the whole axis, undecided:
    /// the net closes exactly in u and the solve covers its whole range, so the solve wraps
    /// there, and each one's boxes cover the whole turn, which no lift less than a turn
    /// wide holds (module docs, "The seam"; no chart of the net over it is certified either:
    /// a pole row lies on the hull's edge, where the line passes).
    #[test]
    fn a_sphere_is_touched_by_a_tangent_and_crossed_by_a_secant_and_through_its_poles() {
        let k = Kernel::default();
        let s = sphere(&k);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let tol = 1.0e-6;
        for gap in [0.0, 0.5 * tol] {
            let p = Vec3::new(-10.0, 3.0 * (1.0 + gap / 5.0), 4.0 * (1.0 + gap / 5.0));
            let (_, c) = typed_of(&k, &s, p, Vec3::new(20.0, 0.0, 0.0), tol, &[whole]);
            assert_eq!(
                types(&c),
                vec![(Contact::Touching, Typed::Sides)],
                "gap {gap}"
            );
        }
        let (_, c) = typed_of(
            &k,
            &s,
            Vec3::new(-10.0, 1.0, 2.0),
            Vec3::new(20.0, 0.0, 0.0),
            tol,
            &[whole],
        );
        assert_eq!(
            types(&c),
            vec![(Contact::Crossing, Typed::Count(1)); 2],
            "{:?}",
            c.list
        );
        let (_, c) = typed_of(
            &k,
            &s,
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::new(0.0, 0.0, 20.0),
            tol,
            &[whole],
        );
        assert_eq!(c.list.len(), 2, "{:?}", c.list);
        assert!(
            c.list
                .iter()
                .all(|x| x.contact == Contact::Undecided
                    && x.typed == Typed::Undecided(Why::WholeTurn)),
            "{:?}",
            c.list
        );
    }

    /// The cylinder: a tangent line and lines missing it within the tolerance touch it (the
    /// side function, the radius past the cylinder's, positive at both ends); a shallow
    /// secant at upstream's step 10 is one component of two certified roots, touching as a
    /// whole (its two crossings cancel); a line through the axis just above the rim passes
    /// within the tolerance of the rim beyond the surface, undecided (its edge boxes have a
    /// side on the surface's boundary, or the side function's zeros between its ends lie past
    /// the rim).
    #[test]
    fn a_cylinder_is_touched_by_a_tangent_and_a_near_miss() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        for step in [0, 1, 10] {
            let (p, d) = across(1.0 + f64::from(step) * STEP);
            let (_, c) = typed_of(&k, &s, p, d, TOL, &[whole]);
            assert_eq!(
                types(&c),
                vec![(Contact::Touching, Typed::Sides)],
                "step {step}: {:?}",
                c.list
            );
            // (a tangent inside the face reaches no boundary side)
            assert!(!c.list[0].at_boundary, "step {step}: {:?}", c.list);
        }
        let (p, d) = across(1.0 - STEP);
        let (_, c) = typed_of(&k, &s, p, d, TOL, &[whole]);
        assert_eq!(c.list.len(), 1, "{:?}", types(&c));
        assert_eq!(c.list[0].contact, Contact::Touching, "{:?}", types(&c));
        assert_eq!(c.list[0].classes.len(), 2, "{:?}", types(&c));
        for h in [0.5, 0.9] {
            let (_, c) = typed_of(
                &k,
                &s,
                Vec3::new(-2.0, 0.0, 1.0 + h * TOL),
                Vec3::new(4.0, 0.0, 0.0),
                TOL,
                &[whole],
            );
            assert!(!c.list.is_empty());
            assert!(
                c.list.iter().all(|x| x.contact == Contact::Undecided),
                "{:?}",
                types(&c)
            );
            // (each reaches the rim, the surface's boundary: another ray's case, answer10 Q2)
            assert!(c.list.iter().all(|x| x.at_boundary), "{:?}", c.list);
        }
    }

    /// A B-spline surface of degrees `(p, q)` from its control points (rows along u) on
    /// clamped knots over `[0, 1]` with no interior knot.
    fn bezier(rows: Vec<Vec<[f64; 3]>>, p: usize, q: usize) -> Surface {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::srf_basic::Surface as NlSurface;
        let knots = |d: usize| {
            let mut k = vec![0.0; d + 1];
            k.extend(vec![1.0; d + 1]);
            k
        };
        let net = rows
            .into_iter()
            .map(|r| {
                r.into_iter()
                    .map(|[x, y, z]| CPoint::new(x, y, z, NOW))
                    .collect()
            })
            .collect();
        Surface::from(BSplineSurface::from_nurb(
            NlSurface::new(net, p, q, knots(p), knots(q)).unwrap(),
            false,
        ))
    }

    /// The foot rule (module docs, "Typing"), on a B-spline dome `z = (1 − x²)(1 − y²)` over
    /// `[−1, 1]²` (biquadratic: the only control height not 0 is the middle one, 4, since
    /// `1 − x² = 2 B₁(u)` with `x = 2u − 1`), peak `(0, 0, 1)` at `(u, v) = (½, ½)`: the line
    /// `z = 1` along x touches it there, and so does the line half the tolerance above (each
    /// end above the dome, `S_u × S_v` upward: both feet's sides positive), touching by the
    /// foot. The line along the dome's edge `y = −1, z = 0` lies in its boundary curve: its
    /// feet sit on the patch's border, where no foot box lies strictly inside the patch, so it
    /// is undecided (the edge counterexample of answer8: a branch leaving the domain), at a
    /// stretch at the floor whose Newton steps toward the foot leave the knots' range. On a
    /// fold, `S(u, v) = (4u(1 − u), v, u)` (the profile `x = 4z(1 − z)` turns back at
    /// `x = 1, z = ½`), the line along z through the tip touches it from outside (`S_u × S_v
    /// = (−1, 0, 4 − 8u)`, `L − S` along +x at both ends: both sides negative), touching by
    /// the foot.
    #[test]
    fn the_foot_rule_types_b_spline_tangencies() {
        let k = Kernel::default();
        let tol = 1.0e-6;
        let z = |i: usize, j: usize| if i == 1 && j == 1 { 4.0 } else { 0.0 };
        #[allow(clippy::cast_precision_loss)]
        let dome = bezier(
            (0..3)
                .map(|i| {
                    (0..3)
                        .map(|j| [i as f64 - 1.0, j as f64 - 1.0, z(i, j)])
                        .collect()
                })
                .collect(),
            2,
            2,
        );
        let whole = dome.natural_uv_domain_with_nurb(&k).unwrap();
        for gap in [0.0, 0.5 * tol] {
            let (_, c) = typed_of(
                &k,
                &dome,
                Vec3::new(-2.0, 0.0, 1.0 + gap),
                Vec3::new(4.0, 0.0, 0.0),
                tol,
                &[whole],
            );
            assert_eq!(
                types(&c),
                vec![(Contact::Touching, Typed::Foot)],
                "gap {gap}: {:?}",
                c.list
            );
        }
        let (_, c) = typed_of(
            &k,
            &dome,
            Vec3::new(-2.0, -1.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            tol,
            &[whole],
        );
        assert!(!c.list.is_empty());
        assert!(
            c.list.iter().all(|x| x.typed
                == Typed::Undecided(Why::FootNotShown(FootWhy::Floor(Stretch::NewtonOff)))),
            "{:?}",
            types(&c)
        );
        let fold = bezier(
            vec![
                vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                vec![[2.0, 0.0, 0.5], [2.0, 1.0, 0.5]],
                vec![[0.0, 0.0, 1.0], [0.0, 1.0, 1.0]],
            ],
            2,
            1,
        );
        let whole = fold.natural_uv_domain_with_nurb(&k).unwrap();
        let (_, c) = typed_of(
            &k,
            &fold,
            Vec3::new(1.0, 0.5, -1.0),
            Vec3::new(0.0, 0.0, 3.0),
            tol,
            &[whole],
        );
        assert_eq!(
            types(&c),
            vec![(Contact::Touching, Typed::Foot)],
            "{:?}",
            c.list
        );
    }

    /// A line along a cone's axis through its apex: the component about the apex is
    /// undecided (the profile ends there), whatever the side function says.
    #[test]
    fn a_line_through_a_cone_s_apex_is_undecided() {
        let k = Kernel::default();
        // (radius 1 at height 0, shrinking to its apex at height 1)
        let mut c = Cone::new_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            1.0,
            0.0,
            0.0,
            360.0,
            1.0,
            false,
            false,
            false,
        );
        c.make_nurb(&k).unwrap();
        let s = Surface::from(c);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (_, comps) = typed_of(
            &k,
            &s,
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(0.0, 0.0, -4.0),
            1.0e-5,
            &[whole],
        );
        assert!(!comps.list.is_empty());
        let near_apex = comps
            .list
            .iter()
            .filter(|x| x.t.lo <= 0.25 && 0.25 <= x.t.hi)
            .collect::<Vec<_>>();
        assert_eq!(near_apex.len(), 1, "{:?}", types(&comps));
        assert_eq!(near_apex[0].contact, Contact::Undecided);
    }

    /// Which closed intervals meet another (the side test's second condition): touching
    /// ends meet, a NaN end meets everything, and the order given does not matter.
    #[test]
    fn overlapping_line_parameters_are_told_by_a_sweep() {
        let i = |a: f64, b: f64| Ivl::new(a, b);
        assert_eq!(
            overlapped(&[i(2.0, 3.0), i(0.0, 1.0), i(3.0, 4.0), i(5.0, 6.0)]),
            vec![true, false, true, false]
        );
        assert_eq!(
            overlapped(&[i(0.0, 10.0), i(5.0, 6.0), i(11.0, 12.0)]),
            vec![true, true, false]
        );
        assert_eq!(
            overlapped(&[i(0.0, 1.0), Ivl::new(f64::NAN, 1.0), i(5.0, 6.0)]),
            vec![true, true, true]
        );
        assert_eq!(overlapped(&[i(0.0, 1.0)]), vec![false]);
    }

    /// A hull is covered by the searched leaves and strips when every cell of their grid
    /// inside it lies in one: two leaves meeting along a side cover the hull across them;
    /// with a hole (a trim cull) inside it, a leaf missing at its corner, or a gap between
    /// two leaves one ulp wide, it is not.
    #[test]
    fn a_hull_is_covered_only_by_searched_domains() {
        let leaf = |a: f64, b: f64, c: f64, d: f64| {
            leaf_of(
                Extent2d::from_bounds(a, c, b, d),
                Stop::Finished,
                Some(Vec::new()),
            )
        };
        let (u, v) = (Ivl::new(0.2, 0.8), Ivl::new(0.1, 0.9));
        let two = [leaf(0.0, 0.5, 0.0, 1.0), leaf(0.5, 1.0, 0.0, 1.0)];
        assert!(covered_by_leaves(u, v, &two));
        let holed = [
            leaf(0.0, 0.5, 0.0, 1.0),
            leaf(0.5, 1.0, 0.0, 0.4),
            leaf(0.5, 1.0, 0.6, 1.0),
        ];
        assert!(!covered_by_leaves(u, v, &holed));
        let corner = [leaf(0.0, 0.5, 0.0, 1.0), leaf(0.5, 1.0, 0.0, 0.85)];
        assert!(!covered_by_leaves(u, v, &corner));
        // a gap one ulp wide between two leaves is not covered: the open strip between 1
        // and the next double was not searched, though the gap's middle rounds to 1, which
        // the first leaf holds; the same leaves meeting at 1 cover the hull
        let (u, v) = (Ivl::new(0.0, 2.0), Ivl::new(0.0, 1.0));
        let gap = [
            leaf(0.0, 1.0, 0.0, 1.0),
            leaf(1.0_f64.next_up(), 2.0, 0.0, 1.0),
        ];
        assert_eq!(0.5 * (1.0 + 1.0_f64.next_up()), 1.0);
        assert!(!covered_by_leaves(u, v, &gap));
        let met = [leaf(0.0, 1.0, 0.0, 1.0), leaf(1.0, 2.0, 0.0, 1.0)];
        assert!(covered_by_leaves(u, v, &met));
    }

    /// A net of `rows` (each a row of homogeneous points `(x w, y w, z w, w)` along v, the
    /// rows along u) on clamped knots of degrees `rows − 1` and `points − 1`, no interior knot.
    fn net_of(rows: &[Vec<[f64; 4]>]) -> Nurbs {
        let (nu, nv) = (rows.len(), rows[0].len());
        let knots = |n: usize| {
            let mut k = vec![0.0; n];
            k.extend(vec![1.0; n]);
            k
        };
        Nurbs {
            p: nu - 1,
            q: nv - 1,
            nu,
            nv,
            u: knots(nu),
            v: knots(nv),
            pw: rows.iter().flatten().flatten().copied().collect(),
        }
    }

    /// The exact closure test reads the two edge rows as one homogeneous row up to one
    /// common factor (answer10, defect 1), and a product that underflows to 0 is not exact
    /// (defect 2). Rows `(0, 0, 0)`, `(1, 1, 0)`, `(2, 0, 0)` of weights 1, 1, 1 and 1, 2, 1
    /// are one Cartesian point at each place, but the edges they make are two curves: at
    /// v = ½ the first is `(1, ½, 0)` (`(0 + 2·1 + 2)/4`, `(0 + 2·1 + 0)/4`) and the second
    /// `(1, ⅔, 0)` (`(0 + 2·2 + 2)/6`, `(0 + 2·2 + 0)/6`): no closure. The second row
    /// scaled by λ = 2 as a homogeneous row (not bitwise equal) is one curve: closure. One
    /// row is compared with itself: no closure. With `a_x = 5·10⁻³²⁴` (the least subnormal)
    /// against `b_x = 0` and `b_w = 0.4`, `a_x b_w` rounds to 0 with its error, while the
    /// exact product is not 0: no closure; with `a_x = 0` the rows are one up to λ = 0.4.
    #[test]
    fn the_net_closes_only_on_one_homogeneous_row_up_to_a_factor() {
        let h = |[x, y, z]: [f64; 3], w: f64| [x * w, y * w, z * w, w];
        let (p0, p1, p2) = ([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]);
        let first = vec![h(p0, 1.0), h(p1, 1.0), h(p2, 1.0)];
        let weighted = vec![h(p0, 1.0), h(p1, 2.0), h(p2, 1.0)];
        assert!(!closes_exactly(
            &net_of(&[first.clone(), weighted.clone()]),
            true
        ));
        // (the same rows read in v: across each row the first and last points differ)
        assert!(!closes_exactly(
            &net_of(&[first.clone(), weighted.clone()]),
            false
        ));
        let scaled: Vec<[f64; 4]> = weighted.iter().map(|p| p.map(|c| 2.0 * c)).collect();
        assert!(closes_exactly(&net_of(&[weighted.clone(), scaled]), true));
        assert!(closes_exactly(
            &net_of(&[first.clone(), first.clone()]),
            true
        ));
        assert!(!closes_exactly(&net_of(std::slice::from_ref(&first)), true));
        // (a start knot of multiplicity the degree plus two: the first row's basis function
        // vanishes, the edge there is the second row, and no closure is shown on the first)
        let mut extra = net_of(&[first.clone(), weighted.clone(), first.clone()]);
        extra.u = vec![0.0, 0.0, 0.0, 1.0, 1.0];
        extra.p = 1;
        assert!(!closes_exactly(&extra, true));
        let mut clamped = extra.clone();
        clamped.u = vec![0.0, 0.0, 0.5, 1.0, 1.0];
        assert!(closes_exactly(&clamped, true));
        let tiny = 5.0e-324;
        let under = |ax: f64| {
            net_of(&[
                vec![[ax, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]],
                vec![[0.0, 0.0, 0.0, 0.4], [0.4, 0.0, 0.0, 0.4]],
            ])
        };
        assert_eq!(tiny * 0.4, 0.0);
        assert!(!closes_exactly(&under(tiny), true));
        assert!(closes_exactly(&under(0.0), true));
        assert!(!equal_products(tiny, 0.4, 0.0, 1.0));
        assert!(equal_products(0.0, 0.4, 0.0, 1.0));
        assert!(equal_products(-0.0, 0.4, 0.0, 1.0));
        assert!(!equal_products(f64::NAN, 1.0, f64::NAN, 1.0));
        assert!(!equal_products(f64::INFINITY, 1.0, f64::INFINITY, 1.0));
    }

    /// [`typed_in`] on the net `net` (not rebuilt from a surface) over the solve `solve`,
    /// one leaf, the line's interval `[0, 1]`.
    fn typed_net(net: &LineNet, solve: Extent2d, p: Vec3, d: Vec3, tol: f64) -> Components {
        typed_net_leaves(net, solve, p, d, tol).1
    }

    /// [`typed_net`] with its leaves.
    fn typed_net_leaves(
        net: &LineNet,
        solve: Extent2d,
        p: Vec3,
        d: Vec3,
        tol: f64,
    ) -> (Vec<SearchedLeaf>, Components) {
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: solve,
            tol,
        };
        let seams = net.seams(&solve);
        let sides = leaf_sides(&[solve], &solve, seams);
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let f = isolate(net, &line, &solve, &interval, &sides[0], None).unwrap();
        let mut leaves = vec![SearchedLeaf {
            domain: solve,
            stop: f.stop,
            floor: Some(f.floor),
            roots: f.roots,
            strip: false,
            inside: true,
        }];
        drop_fenced(net, &line, &mut leaves, &sides, None, MAX_DROP_ENCLOSURES).unwrap();
        let mut c = components(&leaves, &sides, [0.0, 1.0], seams);
        type_components(net, &line, &leaves, &sides, &mut c, None).unwrap();
        (leaves, c)
    }

    /// Whether a component is joined across a seam.
    fn glued(c: &Component) -> bool {
        c.hull.iter().any(|h| h.then.is_some())
    }

    /// The sphere of radius 5 about the origin with its placement's x axis `x` and y axis
    /// `y` (its seam, u = 0, toward `x`).
    fn sphere_on(k: &Kernel, x: Vec3, y: Vec3) -> Surface {
        let mut s = crate::surface::sphere::Sphere::new(
            k,
            Vec3::ZERO,
            x,
            y,
            Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
            5.0,
            crate::surface::sphere::SphereOptions::default(),
        )
        .unwrap();
        s.make_nurb(k).unwrap();
        Surface::from(s)
    }

    /// The seam join (answer10, Q1) needs exact closure. The line along x at `y = tol / 2`,
    /// `z = 1` leaves the sphere of radius 5 at `x = √(24 − y²)`, a root `y / x ≈ 10⁻⁷`
    /// radians past its seam (u = 0, toward +x); the sphere's other end, below the seam, lies
    /// within the tolerance of the line too, so boxes on both ends of the knots' range hold
    /// that exit: joined across the seam (the net closes bitwise there, and the solve is the
    /// whole range), one component counting one root, a crossing. Moved by one ulp (one
    /// coordinate of an interior control point of the last row, or one weight with its
    /// coordinates unchanged: a point 10⁻¹⁵ off), the net does not close, nothing joins, and
    /// the line is typed as the closed net is without a wrap (its closure ignored). A solve
    /// over part of the knots' range does not wrap either.
    #[test]
    fn the_seam_joins_only_where_the_net_closes_exactly() {
        let k = Kernel::default();
        let s = sphere(&k);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let tol = 1.0e-6;
        let (p, d) = (Vec3::new(-10.0, 0.5 * tol, 1.0), Vec3::new(20.0, 0.0, 0.0));
        assert_eq!(net.closes, [true, false]);
        assert_eq!(net.seams(&whole).wrap, [true, false]);
        let c = typed_net(&net, whole, p, d, tol);
        let exit: Vec<&Component> = c.list.iter().filter(|x| x.t.lo > 0.5).collect();
        assert_eq!(exit.len(), 1, "{:?}", c.list);
        assert!(glued(exit[0]), "{:?}", exit[0]);
        assert_eq!(
            (exit[0].contact, exit[0].typed),
            (Contact::Crossing, Typed::Count(1)),
            "{:?}",
            exit[0]
        );
        // the closed net with its closure ignored: the solve without a wrap
        let unwrapped = LineNet {
            closes: [false, false],
            ..net.clone()
        };
        let before = typed_net(&unwrapped, whole, p, d, tol);
        assert!(before.list.iter().all(|x| !glued(x)));
        let (last, mid) = (net.net.nu - 1, net.net.nv / 2);
        let at = (last * net.net.nv + mid) * 4;
        for slot in [at, at + 3] {
            let mut moved = net.clone();
            moved.net.pw[slot] = moved.net.pw[slot].next_up();
            moved.closes = [
                closes_exactly(&moved.net, true),
                closes_exactly(&moved.net, false),
            ];
            assert_eq!(moved.closes, [false, false], "slot {slot}");
            assert_eq!(moved.seams(&whole).wrap, [false, false]);
            let c = typed_net(&moved, whole, p, d, tol);
            assert!(c.list.iter().all(|x| !glued(x)), "slot {slot}");
            assert_eq!(types(&c), types(&before), "slot {slot}");
        }
        let (lo, hi) = (whole.min(), whole.max());
        let part = Extent2d::from_bounds(lo.x, lo.y, 0.5 * (lo.x + hi.x), hi.y);
        assert_eq!(net.seams(&part).wrap, [false, false]);
    }

    /// The type of a line does not depend on where the seam lies (answer10, Q1): the sphere
    /// of radius 5 with its seam toward +x, and the same sphere with its seam toward +y (its
    /// placement turned a quarter about z, a net that closes bitwise too). The line of
    /// [`the_seam_joins_only_where_the_net_closes_exactly`] crosses both at its exit, by
    /// one counted root; a line tangent at `(4, 0, 3)` across the seam's meridian (along y,
    /// and half the tolerance off it) touches both, by the side function: joined across the
    /// seam on the first, its hull is the two pieces glued there and its chart's edges both
    /// pieces' edges.
    #[test]
    fn a_line_is_typed_alike_with_the_seam_on_it_or_turned_away() {
        let k = Kernel::default();
        let tol = 1.0e-6;
        let on = sphere_on(&k, Vec3::X, Vec3::Y);
        let away = sphere_on(&k, Vec3::Y, Vec3::new(-1.0, 0.0, 0.0));
        for s in [&on, &away] {
            assert_eq!(LineNet::of(&k, s).unwrap().unwrap().closes, [true, false]);
        }
        let typed = |s: &Surface, p: Vec3, d: Vec3| {
            let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
            typed_of(&k, s, p, d, tol, &[whole]).1
        };
        let (p, d) = (Vec3::new(-10.0, 0.5 * tol, 1.0), Vec3::new(20.0, 0.0, 0.0));
        let exit = |c: &Components| {
            let e: Vec<(Contact, Typed)> = c
                .list
                .iter()
                .filter(|x| x.t.lo > 0.5)
                .map(|x| (x.contact, x.typed))
                .collect();
            e
        };
        let (a, b) = (typed(&on, p, d), typed(&away, p, d));
        assert_eq!(
            exit(&a),
            vec![(Contact::Crossing, Typed::Count(1))],
            "{:?}",
            a.list
        );
        assert_eq!(exit(&a), exit(&b), "{:?}", b.list);
        for gap in [0.0, 0.5 * tol] {
            let q = Vec3::new(4.0, 0.0, 3.0) * (1.0 + gap / 5.0);
            let (p, d) = (q - Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 20.0, 0.0));
            let (a, b) = (typed(&on, p, d), typed(&away, p, d));
            assert_eq!(
                types(&a),
                vec![(Contact::Touching, Typed::Sides)],
                "gap {gap}: {:?}",
                a.list
            );
            assert!(glued(&a.list[0]), "gap {gap}: {:?}", a.list[0]);
            assert_eq!(types(&a), types(&b), "gap {gap}: {:?}", b.list);
        }
    }

    /// A line crossing the cylinder exactly on its seam's meridian (along x at y = 0, its
    /// seam toward +x) is never counted twice (answer10, Q1, condition 6): no root on the
    /// seam itself is certified from either end (Krawczyk's operator must lie strictly in a
    /// box on the knots' range), so the boxes about it are left at the floor on both ends,
    /// one component joined across the seam, a crossing by the side function (the radius
    /// past the cylinder's changes sign along the line); a root whose enclosure reached the
    /// seam would leave it undecided. Before the join, the two ends were two components
    /// whose line parameters overlap, each undecided. The crossing at `x = −1` is one root,
    /// inside the range.
    #[test]
    fn a_crossing_on_the_seam_is_never_counted_twice() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (_, c) = typed_of(
            &k,
            &s,
            Vec3::new(-2.0, 0.0, 0.5),
            Vec3::new(4.0, 0.0, 0.0),
            1.0e-5,
            &[whole],
        );
        assert!(
            c.list.iter().all(|x| x.typed != Typed::Count(2)),
            "{:?}",
            types(&c)
        );
        let seam: Vec<&Component> = c.list.iter().filter(|x| x.t.lo > 0.5).collect();
        assert_eq!(seam.len(), 1, "{:?}", c.list);
        assert_eq!(
            (seam[0].contact, seam[0].typed),
            (Contact::Crossing, Typed::Sides),
            "{:?}",
            seam[0]
        );
        assert!(glued(seam[0]), "{:?}", seam[0]);
        let far: Vec<&Component> = c.list.iter().filter(|x| x.t.hi < 0.5).collect();
        assert_eq!(far.len(), 1, "{:?}", c.list);
        assert_eq!(far[0].typed, Typed::Count(1), "{:?}", far[0]);
    }

    /// A closed quadratic B-spline ring about z through the control points `points` (the
    /// first and the last one point, the seam), z from 0 to 1 along v, on the knots `0, 0,
    /// 0, 0.1, 0.5, 0.7, 1, 1, 1`: its first and last rows are one, so it closes exactly, and
    /// its end spans differ (0.1 and 0.3), so `S_u` jumps at the seam whatever its
    /// directions there.
    fn ring_of(points: [[f64; 2]; 6]) -> Surface {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::srf_basic::Surface as NlSurface;
        let net = points
            .iter()
            .map(|&[x, y]| vec![CPoint::new(x, y, 0.0, NOW), CPoint::new(x, y, 1.0, NOW)])
            .collect();
        Surface::from(BSplineSurface::from_nurb(
            NlSurface::new(
                net,
                2,
                1,
                vec![0.0, 0.0, 0.0, 0.1, 0.5, 0.7, 1.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap(),
            false,
        ))
    }

    /// The ring of [`ring_of`] through `(1, 0)`, `(1, 1)`, `(−1, 1)`, `(−1, −1)`, `(1, −1)`,
    /// `(1, 0)`: its seam at `(1, 0)`, `u = 0`, which it leaves along +y at the low end (`y
    /// ≈ 20 u`, `S_u = 2 (P₁ − P₀) / 0.1 = (0, 20)`) and reaches from −y at the high end
    /// (`S_u = 2 (P₅ − P₄) / 0.3 = (0, 6.7)`): one direction, two lengths, so `G¹` and not
    /// `C¹`. Every point has `x ≤ 1`, equal only on the seam (each end span's hull touches
    /// `x = 1` along its end edge only).
    fn crease_ring() -> Surface {
        ring_of([
            [1.0, 0.0],
            [1.0, 1.0],
            [-1.0, 1.0],
            [-1.0, -1.0],
            [1.0, -1.0],
            [1.0, 0.0],
        ])
    }

    /// The ring of [`ring_of`] through `(1, 0)`, `(2, −½)`, `(3, −3)`, `(−1, −3)`, `(1,
    /// −1)`, `(1, 0)`: it reaches its seam `(1, 0)` from −y as [`crease_ring`] does (`S_u =
    /// (0, 6.7)`, `x ≤ 1` on its last span, equal at the seam only) and leaves it back and
    /// outward, along `(1, −½)` (`S_u = 2 (P₁ − P₀) / 0.1 = (20, −10)`): a fold. With `S_v =
    /// (0, 0, 1)`, `S_u × S_v` is `(−10, −20, 0)` on the low end and `(6.7, 0, 0)` on the
    /// high end, their product −67 negative: more than a quarter turn apart (117°). It
    /// stays in `x > 1` from the seam down to below `y = −1`, so a line along y at `x ≤ 1`
    /// over `y` from −1 to 1 meets only its last span.
    fn fold_ring() -> Surface {
        ring_of([
            [1.0, 0.0],
            [2.0, -0.5],
            [3.0, -3.0],
            [-1.0, -3.0],
            [1.0, -1.0],
            [1.0, 0.0],
        ])
    }

    /// A seam the net is not proved `C¹` across is crossed by the foot along a frame, not by
    /// the closest-point foot (answer10, Q1, condition 3; answer12, Q2), on [`crease_ring`]
    /// and the line along y at `x = 1 + tol / 2`, `z = ½`: it passes the seam within the
    /// tolerance, and the component is joined there. Its closest-point foot box would
    /// straddle the seam, which that rule refuses (a crease); the foot along a frame needs
    /// `S` continuous only, which the exact closure gives, with the hull of both ends'
    /// `S_u` (`(0, 20)` and `(0, 6.7)`: a contraction of a half). Every point of the ring
    /// has `x ≤ 1` and the line lies at `x > 1`: it does not cross, so both its ends lie on
    /// one side, touching. (Before answer12's rule: undecided at the crease.)
    #[test]
    fn a_seam_not_proved_c1_is_crossed_by_the_foot_along_a_frame() {
        let k = Kernel::default();
        let s = crease_ring();
        let line = LineNet::of(&k, &s).unwrap().unwrap();
        assert_eq!(line.closes, [true, false]);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let tol = 1.0e-6;
        let (_, c) = typed_of(
            &k,
            &s,
            Vec3::new(1.0 + 0.5 * tol, -1.0, 0.5),
            Vec3::new(0.0, 2.0, 0.0),
            tol,
            &[whole],
        );
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        assert!(glued(&c.list[0]), "{:?}", c.list[0]);
        assert_eq!(
            (c.list[0].contact, c.list[0].typed),
            (Contact::Touching, Typed::FrameFoot),
            "{:?}",
            c.list[0]
        );
        // (the seam is a crease, no boundary)
        assert!(!c.list[0].at_boundary, "{:?}", c.list[0]);
    }

    /// Newton's steps toward the closest-point foot that leave the knots' range are told by
    /// where they leave it (answer12, Q1), on the ring of [`crease_ring`] and the line along y
    /// at `x = 1 + tol / 2`. From `(u, v) = (0.005, ½)` (the low end, `y ≈ 0.1`) toward the
    /// foot of the line's point at `y = −0.05` (`t = 0.475`), which lies across the seam on
    /// the high end, the first step asks for `u ≈ −0.003` (to first order `0.005 − 0.15 / 20`,
    /// the curvature term making it a little longer): off the low end of
    /// u alone, v unmoved (the point's height ½ is the start's). The solve over the whole
    /// range wraps in u, so the steps stopped at the seam a foot box would straddle: a
    /// crease. The same steps in a solve over half the range in u (no wrap: its border there
    /// is a boundary) leave past a border: `NewtonOff`, as before. So do steps past the high
    /// end of v, which never wraps here (the ring is open along z): toward a point at height
    /// `1.5`, with u leaving too (`y = −0.05`) or not (`y = 0.1`, `t = 0.55`). A foot on the
    /// range is found (`y = 0.1` at height ½).
    #[test]
    fn newton_leaving_only_past_a_wrapped_end_stops_at_a_crease() {
        let k = Kernel::default();
        let s = crease_ring();
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (lo, hi) = (whole.min(), whole.max());
        let part = Extent2d::from_bounds(lo.x, lo.y, 0.5 * (lo.x + hi.x), hi.y);
        assert_eq!(net.seams(&whole).wrap, [true, false]);
        assert_eq!(net.seams(&part).wrap, [false, false]);
        let tol = 1.0e-6;
        let scratch = |solve: Extent2d, z: f64| {
            ScratchLine::of(
                &PassLine {
                    point: Vec3::new(1.0 + 0.5 * tol, -1.0, z),
                    vector: Vec3::new(0.0, 2.0, 0.0),
                    span: [0.0, 1.0],
                    solve_domain: solve,
                    tol,
                },
                &Extent1d::from_min_max(0.0, 1.0),
            )
            .unwrap()
        };
        let start = [0.005, 0.5];
        let foot =
            |solve: Extent2d, z: f64, t: f64| newton_foot(&net, &scratch(solve, z), t, start);
        assert_eq!(foot(whole, 0.5, 0.475), Err(Stretch::Crease));
        assert_eq!(foot(part, 0.5, 0.475), Err(Stretch::NewtonOff));
        assert_eq!(foot(whole, 1.5, 0.475), Err(Stretch::NewtonOff));
        assert_eq!(foot(whole, 1.5, 0.55), Err(Stretch::NewtonOff));
        let [u, v] = foot(whole, 0.5, 0.55).unwrap();
        assert!(u > 0.0 && u < 0.1 && (v - 0.5).abs() < 1.0e-9, "{u} {v}");
    }

    /// A crossing beside a seam the net is not proved `C¹` across (answer11, the 8 + 2
    /// points; answer12, Q2). The invariant: the solution point at each certified root is
    /// never typed as another contact than a crossing, and where it is undecided its
    /// component is undecided at that crease (`FootNotShown(Floor(Crease))`).
    ///
    /// On [`crease_ring`] (seam at `(1, 0)`, `x ≤ 1` with equality on the seam only), lines a
    /// hair to one side of the seam's meridian (along −x at `y = ε`, `|ε|` from a quarter of
    /// the tolerance to twice it) and along it (along y at `x = 1 − δ`, two shallow
    /// crossings, one on each end, beside the line's tangency at the seam, `δ` from a quarter
    /// of the tolerance to 64 times it). Where the line passes within the tolerance of the
    /// seam the two ends join into one component; with boxes left at the floor there (`δ`
    /// up to the tolerance) the closest-point foot box would straddle the seam, and the foot
    /// along a frame types it: touching (the line is outside the ring, `x > ` the ring's,
    /// before its first crossing and after its second, so its ends lie on one side), each
    /// root's point a crossing. No root's point is undecided on this ring any more (before
    /// answer12's rule the three closest lines along the meridian were undecided at the
    /// crease, and the test asked for one such case here).
    ///
    /// The case that must stay undecided is a fold at the seam ([`fold_ring`]): the two
    /// ends' normals are more than a quarter turn apart, so `(e₁ × e₂) · (S_u × S_v)` has
    /// opposite signs on the two ends whichever end's box gives the frame, the Jacobian of
    /// the foot along it holds a singular matrix over any box across the seam, and
    /// Krawczyk's operator cannot lie inside it. The lines along y at `x = 1 − δ`, `δ` a
    /// quarter of the tolerance, a half and the whole, cross the ring's last span once,
    /// shallowly, beside the seam, within the tolerance of it: one component joined there
    /// with boxes left at the floor, undecided at the crease by the closest-point foot and
    /// by its contraction along the frame, its roots' points undecided. At least one such
    /// point is required, so the test reads the case the invariant is about.
    #[test]
    fn a_crossing_beside_a_crease_seam_is_a_crossing_or_undecided_at_the_crease() {
        let k = Kernel::default();
        let tol = 1.0e-6;
        let at_the_crease = Typed::Undecided(Why::FootNotShown(FootWhy::Floor(Stretch::Crease)));
        // the points at the certified roots of each line: how many are undecided, and
        // the components typed by the foot along a frame
        let sweep = |s: &Surface, lines: &[(Vec3, Vec3)], folded: bool| {
            let line_net = LineNet::of(&k, s).unwrap().unwrap();
            let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
            let (mut undecided, mut by_frame) = (0, 0);
            for &(p, d) in lines {
                let line = PassLine {
                    point: p,
                    vector: d,
                    span: [0.0, 1.0],
                    solve_domain: whole,
                    tol,
                };
                let (_, c) = typed_of(&k, s, p, d, tol, &[whole]);
                for comp in &c.list {
                    by_frame += usize::from(comp.typed == Typed::FrameFoot);
                    for root in &comp.roots {
                        let [u, v, t] = root.point;
                        let at = [SolutionAt::Point([t, u, v])];
                        let (typed, _) = type_solutions(&line_net, &line, &c, &at, false);
                        match typed[0].map(|x| x.0) {
                            Some(Contact::Crossing) => {}
                            Some(Contact::Undecided) => {
                                undecided += 1;
                                assert_eq!(comp.typed, at_the_crease, "{p:?}: {comp:?}");
                                // (and there only through the fold: no contraction)
                                assert!(folded, "{p:?}: {comp:?}");
                                assert_eq!(
                                    comp.frame_why,
                                    Some(Why::FootNotShown(FootWhy::Floor(Stretch::NoContraction))),
                                    "{p:?}: {comp:?}"
                                );
                            }
                            other => panic!("{p:?}: {other:?}, {comp:?}"),
                        }
                    }
                }
            }
            (undecided, by_frame)
        };
        // across the seam's meridian a hair to one side (along −x at y = ε), and along it
        // (along y at x = 1 − δ: two shallow crossings, one on each end, beside the tangency
        // at the seam)
        let along = |f: f64| {
            (
                Vec3::new(1.0 - f * tol, -1.0, 0.5),
                Vec3::new(0.0, 2.0, 0.0),
            )
        };
        let mut lines: Vec<(Vec3, Vec3)> = [-2.0, -0.5, -0.25, 0.25, 0.5, 2.0]
            .map(|f| (Vec3::new(2.0, f * tol, 0.5), Vec3::new(-2.0, 0.0, 0.0)))
            .to_vec();
        lines.extend([0.25, 0.5, 1.0, 2.0, 4.0, 16.0, 64.0].map(along));
        let (undecided, by_frame) = sweep(&crease_ring(), &lines, false);
        assert_eq!(undecided, 0);
        // (the three closest lines along the meridian)
        assert_eq!(by_frame, 3);
        let (undecided, by_frame) = sweep(&fold_ring(), &[0.25, 0.5, 1.0].map(along), true);
        // (the case the invariant is about occurs in the sweep)
        assert!(undecided > 0);
        assert_eq!(by_frame, 0);
    }

    /// A component around the whole turn holding boxes left at the floor stays undecided
    /// (answer10, Q1, condition 7; answer11, Q3: its sides and foot read a lift): a
    /// cylinder of radius `4·10⁻⁴` and height `2·10⁻³` about z lies within the tolerance
    /// `10⁻³` of the line along z through it all round, so its boxes cover the whole knots'
    /// range in u, which the solve wraps: no lift of the component is a rectangle less than
    /// a turn wide.
    #[test]
    fn a_component_around_the_whole_turn_is_undecided() {
        let k = Kernel::default();
        let mut c = Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            4.0e-4,
            0.0,
            360.0,
            2.0e-3,
            false,
            false,
            true,
        );
        c.make_nurb(&k).unwrap();
        let s = Surface::from(c);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (_, c) = typed_of(
            &k,
            &s,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 3.0),
            TOL,
            &[whole],
        );
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        assert!(c.list[0].whole_turn, "{:?}", c.list[0]);
        assert_eq!(
            (c.list[0].contact, c.list[0].typed),
            (Contact::Undecided, Typed::Undecided(Why::WholeTurn)),
            "{:?}",
            c.list[0]
        );
    }

    /// A component around the whole turn whose boxes are all regular is typed by its count
    /// (answer11, Q3): the count reads no lift (every box regular, a root unique in its native
    /// box and off the seam, the end certificate and the placing box by box), so a whole turn
    /// does not refuse it. On records over the unit cylinder's net: two boxes with no root
    /// covering the whole turn in u, one band in v, their line parameters overlapping, are one
    /// component covering the whole range, a count of no root, touching (before, undecided
    /// by its whole turn); with a box left at the floor among them it stays undecided by its
    /// whole turn (the side chart and the foot chain read a lift).
    #[test]
    fn a_whole_turn_of_regular_boxes_is_typed_by_its_count() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let [[u0, u1], [v0, v1]] = net.range;
        let (um, vm) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
        let h = 1.0e-3 * (v1 - v0);
        let line = PassLine {
            point: Vec3::new(-2.0, 0.0, 0.5),
            vector: Vec3::new(4.0, 0.0, 0.0),
            span: [0.0, 1.0],
            solve_domain: whole,
            tol: 1.0e-2,
        };
        let seams = net.seams(&whole);
        let sides = leaf_sides(&[whole], &whole, seams);
        let typed = |second: FloorKind| {
            let at = |kind: FloorKind, [a, b]: [f64; 2]| FloorBox {
                t: Some(Ivl::new(0.4, 0.6)),
                ..rec(kind, [a, b], [vm - h, vm + h])
            };
            let floor = vec![at(FloorKind::NoRoot, [u0, um]), at(second, [um, u1])];
            let leaves = [leaf_of(whole, Stop::Finished, Some(floor))];
            let mut c = components(&leaves, &sides, [0.0, 1.0], seams);
            type_components(&net, &line, &leaves, &sides, &mut c, None).unwrap();
            c
        };
        let c = typed(FloorKind::NoRoot);
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        assert!(c.list[0].whole_turn, "{:?}", c.list[0]);
        assert_eq!(
            (c.list[0].contact, c.list[0].typed),
            (Contact::Touching, Typed::Count(0)),
            "{:?}",
            c.list[0]
        );
        let c = typed(FloorKind::Left);
        assert!(c.list[0].whole_turn, "{:?}", c.list[0]);
        assert_eq!(
            (c.list[0].contact, c.list[0].typed),
            (Contact::Undecided, Typed::Undecided(Why::WholeTurn)),
            "{:?}",
            c.list[0]
        );
    }

    /// The join across a seam on records alone (module docs, "The seam"), one leaf over the
    /// whole range `[0, 1]²`: two boxes on opposite ends of u whose v stretches meet and
    /// whose line parameters overlap are one component, its hull in u the piece at the high
    /// end glued to the one at the low end; with their line parameters apart, or no wrap,
    /// two components. Boxes whose stretches cover the whole range are a whole turn (two
    /// meeting at ½, or one box over the range, joined to itself across the seam). At a
    /// corner of a solve wrapping in both directions, boxes on opposite corners meet across
    /// both seams and are glued in both.
    #[test]
    fn boxes_on_opposite_ends_join_across_a_seam() {
        let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let at = |u: [f64; 2], v: [f64; 2], t: [f64; 2]| FloorBox {
            t: Some(Ivl::new(t[0], t[1])),
            ..rec(FloorKind::Left, u, v)
        };
        let seams = |wu: bool, wv: bool| Seams {
            range: [[0.0, 1.0], [0.0, 1.0]],
            wrap: [wu, wv],
        };
        let run = |floor: Vec<FloorBox>, s: Seams| {
            let leaves = [leaf_of(dom, Stop::Finished, Some(floor))];
            components(&leaves, &leaf_sides(&[dom], &dom, s), [0.0, 1.0], s)
        };
        let (a, b) = (
            at([0.9, 1.0], [0.4, 0.5], [0.2, 0.3]),
            at([0.0, 0.1], [0.45, 0.55], [0.25, 0.35]),
        );
        let c = run(vec![a, b], seams(true, false));
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        assert_eq!(
            c.list[0].hull[0],
            Span {
                first: Ivl::new(0.9, 1.0),
                then: Some(Ivl::new(0.0, 0.1)),
            }
        );
        assert_eq!(c.list[0].hull[1], Span::native(Ivl::new(0.4, 0.55)));
        assert!(!c.list[0].whole_turn);
        assert_eq!(run(vec![a, b], seams(false, false)).list.len(), 2);
        let apart = at([0.0, 0.1], [0.45, 0.55], [0.5, 0.6]);
        assert_eq!(run(vec![a, apart], seams(true, false)).list.len(), 2);
        let halves = vec![
            at([0.5, 1.0], [0.4, 0.5], [0.2, 0.3]),
            at([0.0, 0.5], [0.4, 0.5], [0.2, 0.3]),
        ];
        let c = run(halves, seams(true, false));
        assert_eq!(c.list.len(), 1);
        assert!(c.list[0].whole_turn, "{:?}", c.list[0]);
        let c = run(
            vec![at([0.0, 1.0], [0.4, 0.5], [0.2, 0.3])],
            seams(true, false),
        );
        assert!(c.list[0].whole_turn, "{:?}", c.list[0]);
        let corner = vec![
            at([0.9, 1.0], [0.9, 1.0], [0.2, 0.3]),
            at([0.0, 0.1], [0.0, 0.1], [0.2, 0.3]),
        ];
        assert_eq!(run(corner.clone(), seams(true, false)).list.len(), 2);
        let c = run(corner, seams(true, true));
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        assert!(
            c.list[0].hull.iter().all(|h| h.then.is_some()),
            "{:?}",
            c.list[0]
        );
        assert!(!c.list[0].whole_turn);
    }

    /// The test of the seam pair (answer11, Q1): over every component the solutions
    /// stand for, the crossings among the points placed in it, a linked pair counted once,
    /// add up to its parity mod 2.
    fn points_add_up(
        net: &LineNet,
        line: &PassLine,
        comps: &Components,
        at: &[SolutionAt],
        (typed, stood): (&[Option<Typing>], &[bool]),
    ) -> bool {
        let holds = |[qu, qv]: [Ivl; 2], u: f64, v: f64| {
            qu.lo <= u && u <= qu.hi && qv.lo <= v && v <= qv.hi
        };
        comps.list.iter().enumerate().all(|(c, comp)| {
            if !stood.get(c).copied().unwrap_or(false) {
                return true;
            }
            let mut links: Vec<ContactId> = Vec::new();
            let mut crossings = 0_usize;
            for (a, t) in at.iter().zip(typed) {
                let SolutionAt::Point(p) = *a else {
                    continue;
                };
                let mine = match place(net, line, comps, p, holds) {
                    Placing::Carrier { comp: pc, .. } | Placing::Other { comp: pc } => pc == c,
                    Placing::Undecided => false,
                };
                match t {
                    Some((Contact::Crossing, _, link)) if mine => match link {
                        Some(l) if links.contains(l) => {}
                        Some(l) => {
                            links.push(*l);
                            crossings += 1;
                        }
                        None => crossings += 1,
                    },
                    _ => {}
                }
            }
            crossings % 2 == usize::from(comp.contact == Contact::Crossing)
        })
    }

    /// A crossing on the seam reported on both ends of the range is one contact (answer11,
    /// Q1): the line along x at y = 0, z = ½ crosses the unit cylinder (seam toward +x)
    /// exactly on its seam's meridian at `t = ¾`, one component joined across the seam, a
    /// crossing by its sides with no certified root. The two points `(¾, u₀, v)` and
    /// `(¾, u₁, v)` (the same point of the net, on the two ends) are one seam location: both
    /// crossings, linked, and the component stood for; answer11's sum over the component counts
    /// the pair once. Without the pair rule they were two other points of an odd rest,
    /// undecided.
    #[test]
    fn a_seam_crossing_reported_on_both_ends_is_one_contact() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let (p, d, tol) = (Vec3::new(-2.0, 0.0, 0.5), Vec3::new(4.0, 0.0, 0.0), 1.0e-5);
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: whole,
            tol,
        };
        let (_, c) = typed_of(&k, &s, p, d, tol, &[whole]);
        let seam = c.list.iter().position(|x| x.t.lo > 0.5).unwrap();
        assert_eq!(
            (c.list[seam].contact, c.list[seam].typed),
            (Contact::Crossing, Typed::Sides)
        );
        assert!(glued(&c.list[seam]) && c.list[seam].classes.is_empty());
        let [[u0, u1], [v0, v1]] = net.range;
        let vm = 0.5 * (v0 + v1);
        let at = [
            SolutionAt::Point([0.75, u0, vm]),
            SolutionAt::Point([0.75, u1, vm]),
        ];
        let (typed, stood) = type_solutions(&net, &line, &c, &at, false);
        let link = typed[0].and_then(|(_, _, l)| l);
        assert!(link.is_some(), "{typed:?}");
        assert_eq!(typed[0], Some((Contact::Crossing, None, link)));
        assert_eq!(typed[1], Some((Contact::Crossing, None, link)));
        assert!(stood[seam]);
        assert!(points_add_up(&net, &line, &c, &at, (&typed, &stood)));
    }

    /// A scene of [`seam_records`]: the surface, the line's point and vector, and the solve's
    /// domain (`None` for the whole).
    type SeamScene<'a> = (&'a Surface, Vec3, Vec3, Option<Extent2d>);

    /// The unit cylinder and the line along x at y = 0, z = ½ through its seam point
    /// `(1, 0, ½)` at `t = ¾`, over the solve `solve`.
    fn cylinder_seam(s: &Surface, solve: Option<Extent2d>) -> SeamScene<'_> {
        (
            s,
            Vec3::new(-2.0, 0.0, 0.5),
            Vec3::new(4.0, 0.0, 0.0),
            solve,
        )
    }

    /// The records of a seam pair (answer11, Q1) on the net of the scene's surface, its line
    /// through a seam point at `t = ¾` (tolerance 10⁻²): boxes `(u, v, t)` of one leaf over
    /// the solve's domain, joined into components, the one holding `t = ¾` typed a crossing
    /// by its sides with no root (`inside` its leaves), and the solutions `at` typed.
    fn seam_records(
        (s, point, vector, solve): SeamScene<'_>,
        boxes: &[([f64; 2], [f64; 2], [f64; 2])],
        at: &[[f64; 3]],
        edit: impl Fn(&mut Components),
    ) -> (Components, Vec<Option<Typing>>, Vec<bool>, bool) {
        let k = Kernel::default();
        let whole = solve.unwrap_or_else(|| s.natural_uv_domain_with_nurb(&k).unwrap());
        let net = LineNet::of(&k, s).unwrap().unwrap();
        let line = PassLine {
            point,
            vector,
            span: [0.0, 1.0],
            solve_domain: whole,
            tol: 1.0e-2,
        };
        let floor = boxes
            .iter()
            .map(|&(u, v, [t0, t1])| FloorBox {
                t: Some(Ivl::new(t0, t1)),
                ..rec(FloorKind::Left, u, v)
            })
            .collect();
        let seams = net.seams(&whole);
        let leaves = [leaf_of(whole, Stop::Finished, Some(floor))];
        let mut c = components(
            &leaves,
            &leaf_sides(&[whole], &whole, seams),
            [0.0, 1.0],
            seams,
        );
        for x in &mut c.list {
            if x.t.lo <= 0.75 && 0.75 <= x.t.hi {
                x.contact = Contact::Crossing;
                x.typed = Typed::Sides;
            }
        }
        edit(&mut c);
        let at: Vec<SolutionAt> = at.iter().map(|&p| SolutionAt::Point(p)).collect();
        let (typed, stood) = type_solutions(&net, &line, &c, &at, false);
        let adds = points_add_up(&net, &line, &c, &at, (&typed, &stood));
        (c, typed, stood, adds)
    }

    /// Each condition of the seam pair (answer11, Q1) failing on its own leaves the two
    /// points unlinked (undecided; or, with one not placed, the other the single point of the
    /// rest; or, each in a root's unique box, two carriers): on the cylinder's records of [`seam_records`], with `A` a box on the low
    /// end of u and `B` one on the high end, joined across the seam, `p` in `A` and `q` in
    /// `B` at the seam point (both linked crossings, the case that passes, and two touching
    /// for an even rest). Failing: a third other point; `A` and `B` joined only through a
    /// third box (`A`'s stretch along the seam apart from `B`'s, `C` on the high end meeting
    /// both); `p`'s line parameter outside the meet of `A`'s and `B`'s, or its coordinate
    /// along the seam outside the meet of their stretches (for an odd rest and for an even
    /// one: two unlinked points are never both touching); `q`'s gap at or above the
    /// tolerance (not placed: `p` alone carries the rest, unlinked); the component in leaves
    /// not all inside the face; certified roots in unique boxes about `p` and `q` (each a
    /// carrier, no pair); and the component not joined across the seam. At a corner of the
    /// torus (both directions wrap), boxes on opposite corners meet across both seams: the
    /// two points at the corner are linked, a point inside its corner box is not.
    #[test]
    fn a_seam_pair_needs_every_condition() {
        let k = Kernel::default();
        let cyl = cylinder(&k);
        let net = LineNet::of(&k, &cyl).unwrap().unwrap();
        let cyl_records = |boxes: &[([f64; 2], [f64; 2], [f64; 2])],
                           at: &[[f64; 3]],
                           edit: &dyn Fn(&mut Components)| {
            seam_records(cylinder_seam(&cyl, None), boxes, at, edit)
        };
        let [[u0, u1], [v0, v1]] = net.range;
        let vm = 0.5 * (v0 + v1);
        let h = 1.0e-3 * (u1 - u0).min(v1 - v0);
        let a = ([u0, u0 + h], [vm - h, vm + h], [0.749, 0.7505]);
        let b = ([u1 - h, u1], [vm - h, vm + h], [0.7495, 0.751]);
        let (p, q) = ([0.75, u0, vm], [0.75, u1, vm]);
        let linked =
            |typed: &[Option<Typing>]| typed.iter().any(|t| t.is_some_and(|x| x.2.is_some()));
        // passes
        let (c, typed, stood, adds) = cyl_records(&[a, b], &[p, q], &|_| {});
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        let l = typed[0].and_then(|x| x.2);
        assert!(l.is_some() && typed[1].and_then(|x| x.2) == l, "{typed:?}");
        assert!(typed
            .iter()
            .all(|t| t.is_some_and(|x| x.0 == Contact::Crossing)));
        assert!(stood[0] && adds);
        // an even rest: both touching, still one location
        let (_, typed, stood, adds) = cyl_records(&[a, b], &[p, q], &|c| {
            c.list[0].contact = Contact::Touching;
        });
        assert!(typed
            .iter()
            .all(|t| t.is_some_and(|x| x.0 == Contact::Touching)));
        assert!(linked(&typed) && stood[0] && adds);
        let undecided = |typed: &[Option<Typing>], stood: &[bool]| {
            typed
                .iter()
                .all(|t| *t == Some((Contact::Undecided, None, None)))
                && !stood[0]
        };
        // three other points
        let third = [0.75, u0 + 0.5 * h, vm];
        let (_, typed, stood, _) = cyl_records(&[a, b], &[p, q, third], &|_| {});
        assert!(undecided(&typed, &stood), "{typed:?}");
        // joined only through the rest of the component: A's stretch along the seam apart
        // from B's, C on the high end meeting A across the seam and B natively
        let a_up = ([u0, u0 + h], [vm + 2.0 * h, vm + 3.0 * h], a.2);
        let b_down = ([u1 - h, u1], [vm - h, vm], b.2);
        let c_mid = ([u1 - h, u1], [vm, vm + 3.0 * h], b.2);
        let p_up = [0.75, u0, vm + 2.5 * h];
        let q_down = [0.75, u1, vm - 0.5 * h];
        let (c, typed, stood, _) = cyl_records(&[a_up, b_down, c_mid], &[p_up, q_down], &|_| {});
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        assert!(undecided(&typed, &stood), "{typed:?}");
        // p's line parameter outside the meet of A's and B's (0.7492 < 0.7495), gap 0.0032
        let (_, typed, stood, _) = cyl_records(&[a, b], &[[0.7492, u0, vm], q], &|_| {});
        assert!(undecided(&typed, &stood), "{typed:?}");
        // p along the seam outside the meet of the two stretches
        let b_high = ([u1 - h, u1], [vm, vm + 2.0 * h], b.2);
        let (_, typed, stood, _) =
            cyl_records(&[a, b_high], &[[0.75, u0, vm - 0.5 * h], q], &|_| {});
        assert!(undecided(&typed, &stood), "{typed:?}");
        // the same two unlinked points of an even rest: two crossings and two touching
        // points are both even, so neither is touching, and the component is not stood for
        let (_, typed, stood, _) =
            cyl_records(&[a, b_high], &[[0.75, u0, vm - 0.5 * h], q], &|c| {
                c.list[0].contact = Contact::Touching;
            });
        assert!(undecided(&typed, &stood), "{typed:?}");
        // q's gap above the tolerance: not placed, p alone carries the odd rest, unlinked
        let wide = ([u1 - h, u1], [vm - h, vm + h], [0.7495, 0.77]);
        let (_, typed, stood, adds) = cyl_records(&[a, wide], &[p, [0.76, u1, vm]], &|_| {});
        assert_eq!(typed[0], Some((Contact::Crossing, None, None)), "{typed:?}");
        assert_eq!(
            typed[1],
            Some((Contact::Undecided, None, None)),
            "{typed:?}"
        );
        assert!(stood[0] && adds);
        // the component in leaves not all inside the face (it may straddle its border)
        let (_, typed, stood, _) = cyl_records(&[a, b], &[p, q], &|c| c.list[0].inside = false);
        assert!(undecided(&typed, &stood), "{typed:?}");
        // certified roots about the seam: each point a root's carrier, no pair
        let (_, typed, stood, adds) = cyl_records(&[a, b], &[p, q], &|c| {
            c.list[0].contact = Contact::Touching;
            c.list[0].classes = vec![
                vec![[Ivl::new(u0, u0 + h), Ivl::new(vm - h, vm + h)]],
                vec![[Ivl::new(u1 - h, u1), Ivl::new(vm - h, vm + h)]],
            ];
        });
        assert!(typed
            .iter()
            .all(|t| t.is_some_and(|x| x.0 == Contact::Crossing && x.1.is_some())));
        assert!(!linked(&typed) && stood[0] && adds);
        // the component not joined across the seam (its hull one native piece)
        let (_, typed, stood, _) = cyl_records(&[a, b], &[p, q], &|c| {
            c.list[0].hull[0] = Span::native(c.list[0].u);
        });
        assert!(undecided(&typed, &stood), "{typed:?}");
        // at a corner of the torus (both directions wrap): boxes on opposite corners meet
        // across both seams, and the two points at the corner are one location; a point
        // inside its corner box, on neither seam line, is not
        let tor = torus(&k);
        let tn = LineNet::of(&k, &tor).unwrap().unwrap();
        assert_eq!(tn.closes, [true, true]);
        let [[a0, a1], [b0, b1]] = tn.range;
        // (a turn of 2π·10⁻⁵ moves a point 0.0022 at the torus's outer radius 35: placed)
        let g = 1.0e-5 * (a1 - a0).min(b1 - b0);
        let corner = (
            &tor,
            Vec3::new(38.0, 0.0, 0.0),
            Vec3::new(-4.0, 0.0, 0.0),
            None,
        );
        let low = ([a0, a0 + g], [b0, b0 + g], [0.749, 0.7505]);
        let high = ([a1 - g, a1], [b1 - g, b1], [0.7495, 0.751]);
        let (c, typed, stood, adds) = seam_records(
            corner,
            &[low, high],
            &[[0.75, a0, b0], [0.75, a1, b1]],
            |_| {},
        );
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        assert!(
            c.list[0].hull.iter().all(|h| h.then.is_some()),
            "{:?}",
            c.list[0]
        );
        assert!(typed
            .iter()
            .all(|t| t.is_some_and(|x| x.0 == Contact::Crossing)));
        assert!(linked(&typed) && stood[0] && adds, "{typed:?}");
        let inner = [0.75, a1 - 0.5 * g, b1 - 0.5 * g];
        let (_, typed, stood, _) =
            seam_records(corner, &[low, high], &[[0.75, a0, b0], inner], |_| {});
        assert!(undecided(&typed, &stood), "{typed:?}");
    }

    /// A patch of degrees (2, 1) with a crease along `u = ½` (a double knot there; with
    /// `broken`, a triple one and the crease's control row twice: the same surface, stored
    /// as two patches the net is not continuous across by its knots): a profile in the xz
    /// plane swept along y from 0 to 1. For `u ≤ ½` the dome `z = −0.2 x²` over `x` from −1
    /// to 0 (control points `(−1, −0.2)`, `(−½, 0)`, `(0, 0)`), horizontal at the ridge `x
    /// = z = 0`; for `u ≥ ½`, with `τ = (cos b, −sin b)` the direction turned down by the
    /// bend `b` and `ν = (sin b, cos b)` its normal, the parabola `ξ τ + ½ ξ² ν` over `ξ`
    /// from 0 to 1 (control points `0`, `½ τ`, `τ + ½ ν`; `ξ = 2u − 1`), leaving the ridge
    /// along `τ`. `S_u × S_v` is `(0, 0, 2)` on the dome's side of the ridge and `2 ν` on
    /// the other: `b` apart.
    fn creased(bend: f64, broken: bool) -> Surface {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::srf_basic::Surface as NlSurface;
        let (sin, cos) = det::sin_cos(bend.to_radians());
        let mut profile = vec![[-1.0, -0.2], [-0.5, 0.0], [0.0, 0.0]];
        let mut knots = vec![0.0, 0.0, 0.0, 0.5, 0.5];
        if broken {
            profile.push([0.0, 0.0]);
            knots.push(0.5);
        }
        profile.push([0.5 * cos, -0.5 * sin]);
        profile.push([cos + 0.5 * sin, -sin + 0.5 * cos]);
        knots.extend([1.0, 1.0, 1.0]);
        let net = profile
            .iter()
            .map(|&[x, z]| vec![CPoint::new(x, 0.0, z, NOW), CPoint::new(x, 1.0, z, NOW)])
            .collect();
        Surface::from(BSplineSurface::from_nurb(
            NlSurface::new(net, 2, 1, knots, vec![0.0, 0.0, 1.0, 1.0]).unwrap(),
            false,
        ))
    }

    /// The lines of the crease tests on [`creased`] with the bend `bend`, each through or
    /// over the ridge point `(0, ½, 0)` (`u = v = ½`) at `t = ½`, of length 2: along `τ −
    /// 10⁻³ ν` and along `τ + ⅛ tol ν` through it (each starts at the ridge point less its
    /// direction, and its direction doubled is its vector, so the stored line passes through
    /// the ridge point exactly), and along x half the tolerance above it.
    fn crease_lines(bend: f64, tol: f64) -> [(Vec3, Vec3); 3] {
        let (sin, cos) = det::sin_cos(bend.to_radians());
        let (along, out) = (Vec3::new(cos, 0.0, -sin), Vec3::new(sin, 0.0, cos));
        let ridge = Vec3::new(0.0, 0.5, 0.0);
        let through = |d: Vec3| (ridge - d, d * 2.0);
        [
            through(along - out * 1.0e-3),
            through(along + out * (0.125 * tol)),
            (Vec3::new(-1.0, 0.5, 0.5 * tol), Vec3::new(2.0, 0.0, 0.0)),
        ]
    }

    /// The leaves of the crease tests: the patch's domain cut at `u` = 0.4375, ½, 0.5625 and
    /// `v` = 0.4375, 0.5625, so that the leaves about the ridge point `(½, ½)` are small
    /// (each leaf's pass and exclusion phase have their own budgets).
    fn crease_leaves() -> Vec<Extent2d> {
        let (us, vs) = ([0.0, 0.4375, 0.5, 0.5625, 1.0], [0.0, 0.4375, 0.5625, 1.0]);
        us.windows(2)
            .flat_map(|u| {
                vs.windows(2)
                    .map(move |v| Extent2d::from_bounds(u[0], v[0], u[1], v[1]))
            })
            .collect()
    }

    /// The foot along a frame across an interior crease with a real angle (answer12, Q2):
    /// [`creased`] bent by 30°, crossed by grazing lines whose boxes are left at the floor,
    /// so no count decides them, and whose closest-point foot box would cross the crease
    /// (refused: the second partials are read there). With `τ` the direction the far side
    /// leaves the ridge along and `ν` its normal:
    ///
    /// - through the ridge point along `τ − 10⁻³ ν`: before the ridge the line rises at 30°
    ///   and more over the dome (`z ≤ 0` there, the line at `z > 0`); past it, at `ξ` along
    ///   `τ`, it lies `10⁻³ ξ + ½ ξ²` below the far side. One crossing, exactly on the crease
    ///   curve, at a milliradian to the far side: a crossing;
    /// - through the ridge point along `τ + ⅛ tol ν`: past the ridge it lies `⅛ tol ξ − ½
    ///   ξ²` above the far side, so it touches the ridge and crosses the far side once at
    ///   `ξ = ¼ tol`: `2.5·10⁻⁷` in model units from the crease along the surface (`u = ½ +
    ///   1.25·10⁻⁷`), at an angle of `1.25·10⁻⁷`, where the box left at the floor that holds
    ///   the crossing has its side on the crease (the boxes there are `2.4·10⁻⁷` wide in
    ///   u). One crossing: a crossing;
    /// - along x at half the tolerance above the ridge, on the convex side: the dome and
    ///   the far side both fall away below `z = 0`. No crossing, a near miss within half the
    ///   tolerance: touching.
    ///
    /// Each is one component, typed by the foot along a frame.
    #[test]
    fn the_foot_along_a_frame_types_contacts_at_a_crease() {
        let k = Kernel::default();
        let tol = 1.0e-6;
        let s = creased(30.0, false);
        let want = [Contact::Crossing, Contact::Crossing, Contact::Touching];
        for (i, ((p, d), contact)) in crease_lines(30.0, tol).into_iter().zip(want).enumerate() {
            let (_, c) = typed_of(&k, &s, p, d, tol, &crease_leaves());
            assert_eq!(
                types(&c),
                vec![(contact, Typed::FrameFoot)],
                "line {i}: {:?}",
                c.list
            );
            assert!(c.list[0].holds.left > 0, "line {i}: {:?}", c.list[0]);
            // (it holds boxes on both sides of the crease)
            assert!(c.list[0].u.lo < 0.5 && c.list[0].u.hi > 0.5, "line {i}");
            if i == 1 {
                // the box left at the floor holding the crossing meets the crease
                let at = 0.5 + 0.125 * tol;
                assert!(
                    c.boxes
                        .iter()
                        .any(|b| b.part == Part::Floor(FloorKind::Left)
                            && b.u.lo == 0.5
                            && b.u.hi > at
                            && b.v.lo <= 0.5
                            && b.v.hi >= 0.5),
                    "{:?}",
                    c.list
                );
            }
        }
    }

    /// A crease that folds the surface over relative to the frame stays undecided (answer12,
    /// Q2, condition 3): [`creased`] bent by 150°, so that `S_u × S_v` on the two sides of
    /// the ridge, `(0, 0, 2)` and `2 ν = (1, 0, −√3)`, are 150° apart, and `(e₁ × e₂) ·
    /// (S_u × S_v)`, the determinant of the Jacobian of the foot along the frame, has
    /// opposite signs on the two sides whichever side's box gives the frame (read here from
    /// each component's own frame). Its enclosure
    /// over a box across the crease holds a singular matrix, so the contraction is not below
    /// 1 and no box is certified: every component of the three lines of
    /// [`the_foot_along_a_frame_types_contacts_at_a_crease`] (the same directions about this
    /// bend's `τ` and `ν`) is undecided, by the closest-point foot at a stretch at the floor
    /// and by the foot along a frame at its contraction, never typed.
    #[test]
    fn a_crease_folding_the_surface_over_the_frame_is_undecided() {
        let k = Kernel::default();
        let tol = 1.0e-6;
        let s = creased(150.0, false);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        // the frame's determinant on the two sides, for the frame of either side's normal
        let normal = |u: f64| {
            let [_, s_u, s_v] = net.over(Ivl::from(u), Ivl::from(0.5), [0.0; 3]).unwrap();
            mids(&cross_i(&s_u, &s_v))
        };
        let (near, far) = (normal(0.25), normal(0.75));
        let along: f64 = near.iter().zip(&far).map(|(a, b)| a * b).sum();
        assert!(along < 0.0, "{near:?} {far:?}");
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        for (i, (p, d)) in crease_lines(150.0, tol).into_iter().enumerate() {
            let (_, c) = typed_of(&k, &s, p, d, tol, &crease_leaves());
            assert!(!c.list.is_empty(), "line {i}");
            let scratch = ScratchLine::of(
                &PassLine {
                    point: p,
                    vector: d,
                    span: [0.0, 1.0],
                    solve_domain: whole,
                    tol,
                },
                &Extent1d::from_min_max(0.0, 1.0),
            )
            .unwrap();
            for x in &c.list {
                // the component's own frame: `(e₁ × e₂) · (S_u × S_v)` on the two sides
                let boxes: Vec<PlacedBox> = x.boxes.iter().map(|&b| c.boxes[b]).collect();
                let frame = Frame::of(&net, &scratch, &boxes, x.t).unwrap();
                let [[ax, ay, az], [bx, by, bz]] = frame.e;
                let w = [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx];
                let det = |n: [f64; 3]| -> f64 { w.iter().zip(&n).map(|(a, b)| a * b).sum() };
                assert!(det(near) * det(far) < 0.0, "line {i}: {frame:?}");
                assert!(
                    matches!(
                        x.typed,
                        Typed::Undecided(Why::FootNotShown(FootWhy::Floor(_)))
                    ),
                    "line {i}: {x:?}"
                );
                assert_eq!(x.contact, Contact::Undecided, "line {i}: {x:?}");
                assert_eq!(
                    x.frame_why,
                    Some(Why::FootNotShown(FootWhy::Floor(Stretch::NoContraction))),
                    "line {i}: {x:?}"
                );
            }
        }
    }

    /// A break of the net is never crossed (answer12, Q2, condition 2): [`creased`] bent by
    /// 30° with the knot at the crease of multiplicity 3, one above the degree, and the
    /// crease's control row stored twice. The surface is the same point set as
    /// [`the_foot_along_a_frame_types_contacts_at_a_crease`]'s, but its net is not continuous
    /// across that knot line by its knots, and no enclosure crosses it. The same three
    /// lines are undecided: the foot along a frame refuses the box at the break.
    #[test]
    fn a_break_of_the_net_is_not_crossed_by_the_foot_along_a_frame() {
        let k = Kernel::default();
        let tol = 1.0e-6;
        let s = creased(30.0, true);
        for (i, (p, d)) in crease_lines(30.0, tol).into_iter().enumerate() {
            let (_, c) = typed_of(&k, &s, p, d, tol, &crease_leaves());
            assert_eq!(c.list.len(), 1, "line {i}: {:?}", c.list);
            let x = &c.list[0];
            assert!(
                matches!(
                    x.typed,
                    Typed::Undecided(Why::FootNotShown(FootWhy::Floor(_)))
                ),
                "line {i}: {x:?}"
            );
            assert_eq!(x.contact, Contact::Undecided, "line {i}: {x:?}");
            assert_eq!(
                x.frame_why,
                Some(Why::FootNotShown(FootWhy::Floor(Stretch::Break))),
                "line {i}: {x:?}"
            );
        }
    }

    /// The foot along a frame crosses the ends of the knots' range only where the solve
    /// wraps (answer12, Q2, condition 2): the net of [`crease_ring`] with one coordinate of
    /// its last row moved by one ulp does not close, so nothing wraps, and the line of
    /// [`a_seam_not_proved_c1_is_crossed_by_the_foot_along_a_frame`] (touching by that rule
    /// on the closed net, its box two pieces glued at the seam) leaves two components, one
    /// on each end of the range, never joined. The rule asked of each directly (their line
    /// parameters meet, so the typing does not reach it) certifies no box: the range's end
    /// is a border there, which its boxes may not reach (the component's boxes lie on it) and
    /// its Newton steps may not pass.
    #[test]
    fn a_net_one_ulp_from_closing_is_not_crossed_at_its_ends() {
        let k = Kernel::default();
        let s = crease_ring();
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let tol = 1.0e-6;
        let (p, d) = (
            Vec3::new(1.0 + 0.5 * tol, -1.0, 0.5),
            Vec3::new(0.0, 2.0, 0.0),
        );
        // (the x coordinate, 1, of the last row's first point)
        let at = (net.net.nu - 1) * net.net.nv * 4;
        assert_eq!(net.net.pw[at], 1.0);
        let mut open = net.clone();
        open.net.pw[at] = open.net.pw[at].next_up();
        open.closes = [
            closes_exactly(&open.net, true),
            closes_exactly(&open.net, false),
        ];
        assert_eq!(open.closes, [false, false]);
        assert_eq!(open.seams(&whole).wrap, [false, false]);
        let scratch = ScratchLine::of(
            &PassLine {
                point: p,
                vector: d,
                span: [0.0, 1.0],
                solve_domain: whole,
                tol,
            },
            &Extent1d::from_min_max(0.0, 1.0),
        )
        .unwrap();
        // the rule asked of every component holding a box left at the floor
        let asked = |net: &LineNet| {
            let (leaves, c) = typed_net_leaves(net, whole, p, d, tol);
            let found: Vec<(bool, core::result::Result<Contact, Why>)> = c
                .list
                .iter()
                .filter(|x| x.holds.left > 0)
                .map(|x| {
                    let boxes: Vec<PlacedBox> = x.boxes.iter().map(|&i| c.boxes[i]).collect();
                    (glued(x), by_frame(net, &scratch, &leaves, &boxes, x, x.t))
                })
                .collect();
            found
        };
        assert_eq!(asked(&net), vec![(true, Ok(Contact::Touching))]);
        let ends = asked(&open);
        assert_eq!(ends.len(), 2, "{ends:?}");
        for (joined, found) in ends {
            assert!(!joined);
            assert!(
                matches!(
                    found,
                    Err(Why::FootNotShown(FootWhy::Floor(
                        Stretch::LeavesDomain | Stretch::NewtonOff
                    )))
                ),
                "{found:?}"
            );
        }
    }

    /// A search kept in a [`LinePasses`] (module docs, "Reuse within an operation") is found
    /// under a key equal word for word to its own and under no key that differs in one
    /// input: a net with one control point a rounding away, another kind of net, a line one
    /// rounding over, a negative zero for a zero, a tolerance a rounding wider, a leaf's inside
    /// flag, one strip more.
    #[test]
    fn a_kept_search_is_found_only_under_its_own_inputs_bit_for_bit() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = across(0.25);
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: solve,
            tol: TOL,
        };
        let leaf = SolvedLeaf {
            domain: solve,
            interval: Extent1d::from_min_max(0.0, 1.0),
            inside: true,
        };
        let strip = Extent2d::from_points(
            crate::math::Point2::new(solve.min().x, solve.min().y),
            crate::math::Point2::new(0.0, solve.max().y),
        );
        let key = |net: &LineNet, line: &PassLine, leaf: SolvedLeaf, strips: &[Extent2d]| {
            PassKey::of(net, line, &[leaf], strips)
        };
        let search = Arc::new(Search {
            leaves: vec![SearchedLeaf {
                domain: solve,
                stop: Stop::Finished,
                floor: Some(Vec::new()),
                roots: Vec::new(),
                strip: false,
                inside: true,
            }],
            components: Components::default(),
        });
        let mut passes = LinePasses::default();
        passes.keep(key(&net, &line, leaf, &[]), &search);
        assert_eq!(
            passes.found(&key(&net, &line, leaf, &[])),
            Some(Arc::clone(&search))
        );
        let mut nudged = net.clone();
        let w = nudged.net.pw.first_mut().unwrap();
        *w = f64::from_bits(w.to_bits() + 1);
        let other = LineNet::of(&k, &torus(&k)).unwrap().unwrap();
        let over = |x: f64| f64::from_bits(x.to_bits() + 1);
        let lines = [
            PassLine {
                point: Vec3::new(over(p.x), p.y, p.z),
                ..line
            },
            PassLine {
                span: [-0.0, 1.0],
                ..line
            },
            PassLine {
                tol: over(TOL),
                ..line
            },
        ];
        let mut missed = vec![
            key(&nudged, &line, leaf, &[]),
            key(&other, &line, leaf, &[]),
            key(
                &net,
                &line,
                SolvedLeaf {
                    inside: false,
                    ..leaf
                },
                &[],
            ),
            key(&net, &line, leaf, &[strip]),
        ];
        missed.extend(lines.iter().map(|l| key(&net, l, leaf, &[])));
        for miss in &missed {
            assert_eq!(passes.found(miss), None, "{miss:?}");
        }
    }

    /// A [`LinePasses`] keeps at most [`MAX_KEPT_SEARCHES`] searches: one past the bound is
    /// not kept, and the searches kept before it are still found.
    #[test]
    fn kept_searches_stop_at_their_bound() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = across(0.25);
        let key = |i: usize| {
            let line = PassLine {
                point: p,
                vector: d,
                span: [0.0, f64::from(u32::try_from(i).unwrap())],
                solve_domain: solve,
                tol: TOL,
            };
            PassKey::of(&net, &line, &[], &[])
        };
        let search = Arc::new(Search::default());
        let mut passes = LinePasses::default();
        for i in 0..=MAX_KEPT_SEARCHES {
            passes.keep(key(i), &search);
        }
        assert!(passes.found(&key(0)).is_some());
        assert!(passes.found(&key(MAX_KEPT_SEARCHES - 1)).is_some());
        assert_eq!(passes.found(&key(MAX_KEPT_SEARCHES)), None);
    }

    /// A search whose lists hold more than [`MAX_KEPT_BYTES`] (counted by their capacity,
    /// which a drop's `retain` keeps) is not kept, and takes nothing of the budget: a small
    /// search after it is kept.
    #[test]
    fn a_search_past_the_byte_bound_is_not_kept() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = across(0.25);
        let key = |end: f64| {
            let line = PassLine {
                point: p,
                vector: d,
                span: [0.0, end],
                solve_domain: solve,
                tol: TOL,
            };
            PassKey::of(&net, &line, &[], &[])
        };
        let room = MAX_KEPT_BYTES / size_of::<FloorBox>() + 1;
        let big = Arc::new(Search {
            leaves: vec![SearchedLeaf {
                domain: solve,
                stop: Stop::Finished,
                floor: Some(Vec::with_capacity(room)),
                roots: Vec::new(),
                strip: false,
                inside: true,
            }],
            components: Components::default(),
        });
        assert!(big.held_bytes() > MAX_KEPT_BYTES);
        let mut passes = LinePasses::default();
        passes.keep(key(1.0), &big);
        assert_eq!(passes.found(&key(1.0)), None);
        let small = Arc::new(Search::default());
        passes.keep(key(2.0), &small);
        assert_eq!(passes.found(&key(2.0)), Some(small));
    }

    /// A key is charged by the storage it is kept with, not by its words alone: a key that
    /// grew room for more than [`MAX_KEPT_BYTES`] is kept shrunk to its words, and the bytes
    /// held are exactly the capacity of the lists kept (each net once).
    #[test]
    fn a_kept_key_is_charged_by_its_storage() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = across(0.25);
        let key = |end: f64| {
            let line = PassLine {
                point: p,
                vector: d,
                span: [0.0, end],
                solve_domain: solve,
                tol: TOL,
            };
            PassKey::of(&net, &line, &[], &[])
        };
        let mut roomy = key(1.0);
        roomy.rest.reserve(MAX_KEPT_BYTES / size_of::<u64>() + 1);
        roomy.net.reserve(MAX_KEPT_BYTES / size_of::<u64>() + 1);
        let search = Arc::new(Search::default());
        let mut passes = LinePasses::default();
        passes.keep(roomy, &search);
        passes.keep(key(2.0), &search);
        assert!(passes.found(&key(1.0)).is_some());
        assert!(passes.found(&key(2.0)).is_some());
        let held: usize = passes
            .nets
            .keys()
            .map(Vec::capacity)
            .chain(
                passes
                    .searches
                    .values()
                    .flat_map(|m| m.keys().map(Vec::capacity)),
            )
            .sum::<usize>()
            * size_of::<u64>();
        assert_eq!(passes.bytes, held);
        assert!(held < MAX_KEPT_BYTES / 1024, "{held}");
    }
}
