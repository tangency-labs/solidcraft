// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_geom_p4`. The entry point sc_blend_fillet_geom_p4_eval
// is defined in blend_fillet_geom_p4_static.inc, which blend_fillet_geom_p1.cpp includes at its
// end: two of the unit's routines (sm_CreateUVCurve, sm_CalcNx2VertFilletXFillet) are
// file-statics of upstream SmFilletGeom.cpp, which can only be compiled into that one
// translation unit, and every op builds its pseudo B-rep with that file's Model. This
// translation unit is intentionally empty.
