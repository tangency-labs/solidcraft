# Metallic flakes under the clear coat

Metallic car paint scatters small tilted metal flakes in its base layer, under the
clear coat. Each flake is a tiny mirror at its own angle, so the paint sparkles where
a flake happens to face both the light and the eye, and the sparkle moves as the view
turns. `solidcraft_viewport::flakes` is the shading model for this; it is a bounded
preview model, not a simulation of pigment layers.

## Parameters

A `FlakeLayer` belongs to one paint and is checked when it is made:

| Parameter | Range | Meaning |
| --- | --- | --- |
| coverage | 0..1 | Share of the surface that flakes cover. 0 means no flakes. |
| size | 0.005..10 mm | About the width of one flake. |
| spread | 0..1 | How far flakes tilt from the surface, as a share of 45°. |
| depth | 0..1 | How deep the deepest flake sits in the paint. |
| color | linear sRGB 0..1 | The flakes' metal reflectance. |
| metallic | 0..1 | 1 is a metal flake, 0 a pearl-like dielectric one. |
| roughness | 0.045..1 | The flake's own perceptual roughness. |

Every face and triangle carries its surface, and the CPU and GPU memory estimates
count it, so a layer is stored in 20 bytes: fractions and color in steps of 1/65535,
the size in single precision. 0 and 1 stay exact. Stored values read back inside
the admitted ranges.

## Where the flakes are

Space, not the mesh, is cut into cells one flake size wide. Each cell holds one
centre, moved inside the cell by a hash of the cell's integer coordinates, and a
point belongs to the cell whose centre is nearest. The cells are irregular, follow no
triangle and have no seams. The same hash, with a different salt for each use,
decides for every cell:

- whether it holds a flake: present when a hash fraction is below coverage;
- the tilt: `spread * 45° * sqrt(u)`, so tilted normals spread evenly over their cone;
- the turn about the surface normal, uniform in a full circle;
- the depth: `depth * u`.

The hash is integer arithmetic on the cell coordinates taken modulo 65536, so the
pattern is identical on every platform and the GPU shader repeats it in 32-bit
integers. Positions further than 10¹² cells from the origin are refused.

## Shading a point on a flake

At a hit, the surface asks the layer for the flake under the point. Between flakes,
the paint shades exactly as before. On a flake:

- The base color, metallic and roughness move from the flake's toward the base
  paint's by the flake's depth: a flake on top is the authored metal, a buried one is
  the base paint, with exact values at both ends.
- The diffuse and specular base lobes turn about the flake's normal `m` instead of
  the surface normal `n`. The clear coat above keeps `n`.
- A flake seen or lit from behind (`m·v <= 0` or `m·l <= 0`) returns nothing.
- The base lobes are multiplied by `min(1, m·v / n·v) * min(1, m·l / n·l)`. This is
  the flake's foreshortening. It is symmetric in view and light, so reciprocity
  holds. Each factor is at most one and keeps the flake within what its own cosine
  admits, so a tilted flake never returns more energy than it receives.

The resolved flake normal is kept in single precision, like the GPU's, and is
renormalized when it is read.

`surface_pdf` is the density of drawing the base lobes about the flake's normal and
the coat lobe about the surface normal. A half vector below the flake's plane is
never drawn, so it has no density.

## Checks

The tests check admission of every parameter, that the covered share matches
coverage, that a flake is stable, stays within its tilt and spans several points,
that there is no seam across the 65536-cell period, the depth mix at both ends,
reciprocity and darkness from behind, and a white furnace: with a full coat over
fully tilted flakes, no tested view returns more than it receives, and the sampler
matches the density.

## Sources

The model was designed from public descriptions of microfacet reflection and of
metallic paint as a binder holding oriented metal flakes, as in
[PBRT's microfacet models](https://pbr-book.org/4ed/Reflection_Models/Roughness_Using_Microfacet_Theory).
The cell pattern is an ordinary jittered-grid nearest-point (Worley) pattern. This
implementation is original. No reference implementation source was copied.
