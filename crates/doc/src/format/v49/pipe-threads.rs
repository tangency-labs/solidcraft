//! A thread's `form` may be `bspp` (ISO 228-1 parallel pipe thread, `G 1/2`), `rp` (ISO 7-1's
//! parallel internal thread, `Rp 1/2`), `bspt` (ISO 7-1 taper, `R 1/2` external, `Rc 1/2`
//! internal) or `npt` (ASME B1.20.1 taper, `1/2-14 NPT`), and a tapped hole's `thread` may be
//! `Rc 1/2` or `1/2-14 NPT`. An older design has none of them and needs no rewrite. The last
//! release refuses a design saved with this (the newer format number).
