# Trimmed-face tight box: the search starts from the face's trims (2026-11-25)

Issue #21660: on the four corpus vases (`Fillet/vase.smb`, `vase1.smb` to `vase3.smb`) the
exact box of each offset face was 10 to 13 mm too large in z and took 5 to 121 s.

![Before, the search over the whole UV domain: cells along the trim divided to the limits, one
of them counted with the solver's point off the face. After, the search over the box of the
face's trims, a cell no trim box meets with its middle outside set aside.](query.face_p3--21660-offset-face-search-in-trims.svg)

*The search domain before and after: compare where each search starts.*

## Mechanism

`face_directed_max_with` searched cells of the face's whole `uv_domain`, `[0,1] x [0,1]` on
vase1 face 8, while its trims cover `[0.19, 0.92] x [0.18, 0.82]`. An offset surface has no
subpatch, so no control-point hull or monotone test set a cell aside. Cells along the trims were
divided to the depth and cell limits, and a cell at the limits counted with the solver's value,
a point of the surface outside the face (z ±89.670384182 against the trims' 76.2).

## What the search does now

- On a surface open both ways over its natural domain it starts from the box of the face's UV
  trims (`query::contact::trim_boxes`).
- A cell no trim box meets, whose middle classifies outside, is set aside.
- A surface without a subpatch is bounded over a cell by the enclosure of its definition
  (`NetSurface::of_exact`, `partials`): the bound before solving, monotony from its first
  partials, a cell without a solution divided, the bound counted at the limits.

## Measured (a probe, not a test; single calls, times indicative)

Before at `a16f86f729`, after at the change, per offset face, the box and the time of `query::face::p3::calculate_tight_bounding_box`
after `create_missing_uv_trim_curves`:

| Body | z before | z after | Time before | Time after |
|---|---|---|---|---|
| vase1 | ±89.670384182 | -57.150094 / 76.200000003 | 4.8 to 6.2 s | 0.005 to 0.007 s |
| vase2 | ±89.670384182 | -57.150120 / 76.200114300 | 7.1 to 84 s | 0.004 to 0.071 s |
| vase3 | ±89.670384182 | -57.150113 / 76.200066596 | 7.3 to 121 s | 0.004 to 0.092 s |
| vase | -111.607282004 / 85.036663880 | -88.900040 / 76.200161332 | 16 to 24 s | 0.024 to 0.035 s |

With a temporary counter, at an earlier commit of the change that gave the same boxes: no cell
reached the limits on these faces, at most 61 cells per direction. What remains above
76.2 (at most 1.6e-4) are solved points that classify inside the face: its trims lie within
their edges' tolerances (5.3e-4 to 1.07e-3) of the edges at z = 76.2, so the face as trimmed
reaches that far.

## Deciding offset cells on the enclosure alone

Deciding an offset's cells only on the enclosure, as a B-spline's on its subpatch (a solved
value in the face only raising the box so far), was measured and not taken: vase1 face 7's
y max went from -9.525 to 24.909829186, and the time from 0.005 s to 0.8 s (vase2 to 2.4 s).
The enclosure is first order in the cell's size (over the whole trim box of the test's quarter
cylinder of radius 1.5 it allows x up to 2.84, the face's greatest is 1.41), so the cells around
each greatest value inside the face were divided to the cell limit and the cells left counted
with their loose bounds. A tighter enclosure (a centred or Taylor form) would let those cells be
decided on bounds.
