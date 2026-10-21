// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_geom_p5`. The entry point sc_blend_fillet_geom_p5_eval
// is defined in blend_fillet_geom_p5_static.inc, which blend_fillet_geom_p1.cpp includes at its
// end: every op builds its pseudo B-rep with that file's Model (the translation unit that
// compiles upstream SmFilletGeom.cpp) and its solvers with blend_fillet_geom_p4_static.inc's
// base solver. This translation unit is intentionally empty.
