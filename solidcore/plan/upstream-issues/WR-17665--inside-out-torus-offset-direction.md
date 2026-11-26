#### WR-17665. An inside-out torus offsets against its NURB normal

- **Severity:** Wrong geometry and an incorrect collapse decision.
- **Area:** Torus offsets, `surface.torus`.
- **Location:** `Torus::create_offset_surface` and `Torus::offset_collapses` in
  `crates/solidcore/src/surface/torus.rs`. The port's offset sign matches the
  `SmTorus::CreateOffsetSurface` expression quoted in SolidCraft issue #17091:
  it reverses for UV swapping but omits the inside-out reversal.
- **Trigger:** R = 4, r = 1.5, d = 0.375. With both UV swapping and inside out,
  the offset has r = 1.125; the source moved along its NURB normal has r = 1.875.
  Without UV swapping, an inside-out torus gives the opposite wrong result.
  Both miss the requested offset by 2d = 0.75.
- **Confirmed:** A standalone Rust probe evaluated the source's NURB point and
  derivatives, formed `P + d * normalize(Su × Sv)`, and compared its tube radius
  with the constructed offset. Two runs reproduced both inside-out failures;
  both ordinary orientations agreed to rounding. The existing golden proof
  explicitly recorded 50 inside-out rows as wrong by 2d.
- **Construction:** For a regular ring torus, its outward unit normal is radial
  from the tube centre. UV swapping reverses `Su × Sv`; reversing the generator
  does so independently. Thus `r_new = r + d` when the two orientation flags
  agree, and `r_new = r - d` otherwise. The same radius decides collapse, with
  the existing strict `r_new < tolerance` boundary.
- **Reference:** Patrikalakis, Maekawa and Cho, *Shape Interrogation for Computer
  Aided Design and Manufacturing*, §11.3.1, defines the offset as `S + d n`:
  https://web.mit.edu/hyperbook/Patrikalakis-Maekawa-Cho/node224.html.
- **Fix:** Both methods use one orientation sign. Regression tests cover all
  four orientations, signed distances and the collapse boundary. The golden
  proof checks the formerly excluded inside-out rows against their normal
  offsets, preserving its separate exclusion for the wholly folded spindle.
- **Tracking:** SolidCraft #17091, PR #17665.
