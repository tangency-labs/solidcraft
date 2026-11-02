//! Emboss and Deboss features store a projection mapping mode (`mapping`: `"wrap"`).
//! Older designs need no transformation: an Emboss without `mapping` uses the default
//! cylindrical wrap projection when targeting a cylinder, or planar projection on planar
//! faces. The last release refuses a design saved with this (the newer format number).
