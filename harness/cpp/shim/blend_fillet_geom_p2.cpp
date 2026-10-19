// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_geom_p2`. The entry point sc_blend_fillet_geom_p2_eval
// is defined in blend_fillet_geom_p2_static.inc, which blend_fillet_geom_p1.cpp includes at its
// end: sm_SrfSrfIntersection is defined in upstream SmFilletGeom.cpp, which can only be compiled
// into that one translation unit (linking it from libSMLib.a as well would duplicate every
// SmFilletGeom symbol), and the topology ops build their pseudo B-rep with that file's Model.
// This translation unit is intentionally empty.
