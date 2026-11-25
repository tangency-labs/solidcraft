# Neighbouring unequal chamfers: closed forms

The regression `neighbouring_unequal_chamfers_build_in_either_history_order` in
`crates/solidcore-ops/tests/blend.rs` uses the 10 mm cube and neighbouring top edges from
SolidCraft #18940. The front edge is chamfered 1 mm along the front face at 30 degrees
from that face. The right edge is chamfered 1 mm along the top and 3 mm down the right
face. The expected geometry is derived below, independently of either history order.

For a box of length L, width Y and height H, put u = L - x and w = H - z. Let the
first chamfer's setbacks along the top and front be a and b, and the second's along
the top and right be c and d. The retained half-spaces are

```text
y/a + w/b >= 1
u/c + w/d >= 1
```

Assume 0 < a < Y, 0 < c < L and 0 < b,d < H. At depth w the removed strips have
widths a(1-w/b) and c(1-w/d), where positive. With m = min(b,d), their overlap is

```text
I = integral[0,m] a c (1-w/b)(1-w/d) dw
  = a c [m - m^2(1/b + 1/d)/2 + m^3/(3 b d)]
V = L Y H - L a b/2 - Y c d/2 + I
```

The eight face areas follow by clipping each plane against these half-spaces:

| Face | Area |
| --- | --- |
| Bottom | L Y |
| Top | (L-c)(Y-a) |
| Front | L(H-b) - c max(d-b,0)^2/(2d) |
| Back | L H - c d/2 |
| Left | Y H - a b/2 |
| Right | Y(H-d) - a max(b-d,0)^2/(2b) |
| First chamfer | sqrt(a^2+b^2) [L - (c/b)(m-m^2/(2d))] |
| Second chamfer | sqrt(c^2+d^2) [Y - (a/d)(m-m^2/(2b))] |

For the first chamfer, integrate its width along x: L-c(1-w/d) where 0 <= w <= m,
otherwise L, with surface element sqrt(a^2+b^2)/b dw. The second chamfer follows by
exchanging the two strips. The front and right face deductions are the remaining
triangles below the other chamfer's depth.

For the reported case L = Y = H = 10 and (a,b,c,d) = (1/sqrt(3),1,1,3):

```text
I = 4/(9 sqrt(3))
V = 985 - 41/(9 sqrt(3)) = 982.3698487736917 mm^3
A = 3287/6 + 10 sqrt(10) + (53-sqrt(10))/(6 sqrt(3))
  = 584.2517470031337 mm^2
```

Both histories retain the same two half-spaces, so these values and the eight faces
are independent of order. Flipping a chamfer exchanges its two setbacks. Scaling
every length by s scales volume by s^3 and area by s^2.

The installed SolidCraft 0.44.1 independently reproduced the reported status-1001
corner failure in the reported order. Reversing the two chamfers built a closed body
with eight planar faces, volume 982.3698487736918 mm^3 and area 584.2517470031337 mm^2,
agreeing with these closed forms. This is a baseline cross-check, not validation of
the modified kernel.

## End classification

The second chamfer's end is on the first chamfer's plane. Its lower rail, x = 10 and
w = 3, crosses that plane at y = -2/sqrt(3), past the end of the diagonal side edge at
(10,0,9). The first chamfer's front boundary there is convex. The old
`end_sections_fit_their_faces_with_ops` judged a past-far-end crossing only when an
end-face boundary was concave. Its ordinary section classification likewise ignored
convex boundaries. The End was consequently handed to a construction requiring the
section to remain on the original end face.

The boundary test now recognizes sharp convex boundaries of planar end faces for
material-removing blends as well. Both forms of the crossing use that test. The
planarity check uses the existing exact surface query: `cut_fillet_at_open_end`
requires a plane, so that is the construction's domain. It selects the existing
overflow construction: cut the removal region by the end face's plane, then subtract
the region from the actual body. Past the diagonal's far end the continued end plane
is outside the original body; subtraction clips the chamfer against the front face.
No second surface construction or retry is introduced.

Passing the side edge's far vertex alone does not prove that the crossing leaves
the end face: the boundary can turn into an L-shaped face there. The far crossing
is therefore classified against the end face's own trim loops before admitting a
new convex cut. The ordinary section check already proves that its interval is off
the face. Both use the solver's existing approximation tolerance.

The existing `a_round_on_an_arc_running_under_a_block_names_the_radius_that_fits`
test distinguishes these events. Its radius-12.5 arc meets z = -7.5 at x = -10;
the side ends at x = -6. For a round of radius 3, the rail's radius is 9.5 and its
crossing is x = -sqrt(9.5^2 - 7.5^2) = -sqrt(34), greater than -6. That crossing
lies on the L-shaped end face, so a convex edge at the side's endpoint must not
select the cut. The existing corner correctly names the limit
12.5 - sqrt(6^2 + 7.5^2). An isolated run with the original concave-only predicate
passed that test and returned 2.8953136438499314 as the fitting radius.

Tangent continuations retain the corner's global merge. Existing concave-blend
classification is retained: the newly recognized convex-boundary event applies only
to blends that remove material. The construction is the End intersection and
overflow trim described in `docs/generic-blend.md`, sections 3.1 and 3.4, and the
natural continuation of a plane in `docs/definitions-and-certificates.md`, section 6.
