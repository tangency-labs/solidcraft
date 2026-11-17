//! A sheet's fold panels save `holes`: material an earlier fold took out of the panel (a lip
//! folded up between cut-outs), so a later fold that carries the panel keeps the opening. Older
//! designs have no panel holes and rebuild them from their feature history. The last release
//! warns about the newer format number, opens what it can read and keeps the rest.
