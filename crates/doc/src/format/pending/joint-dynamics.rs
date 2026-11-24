//! A joint's `dynamics`: per value, the effort and velocity limits, damping and friction a
//! simulator reads (`joint.dynamics`, docs/urdf.md). Older designs have none, and exports write
//! what they always did. The last release refuses a design with this format number.
