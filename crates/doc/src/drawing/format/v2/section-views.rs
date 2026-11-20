//! A drawing's views can be section views (`kind.section`, #13046): a parent view cut along a cutting
//! line drawn in its plane (`line`: its shape `full`, `half`, `offset` or `aligned`, its points in the
//! parent's plane coordinates, and the side its arrows point to). An older drawing has none. The last
//! release warns about the newer format number, opens what it can read and keeps the rest.
