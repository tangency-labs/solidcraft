//! An Emboss or Deboss may save its picked `face` on a planar face, an interior cylinder wall or
//! another curved face, not only on an exterior cylinder; it is then built onto that face. Older
//! designs need no transformation: their features keep the faces they saved. The last release
//! warns about the newer format number and opens the design, but fails to rebuild such a feature,
//! since it embosses a picked face only on an exterior cylinder.
