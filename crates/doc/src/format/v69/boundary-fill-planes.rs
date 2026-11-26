//! A Boundary Fill feature may list planes (`planes`: origin or construction plane references,
//! as a split saves its plane) next to its tool bodies. Each plane cuts space like an infinite
//! plane, so it can close an open end of a tool body, and the feature follows the plane when it
//! moves. A feature without `planes` has none, so older designs are read as they were and a
//! Boundary Fill with no planes saves exactly what it saved before. The last release warns about
//! the newer format number, opens the design and ignores the planes, so it fills the cells from
//! the tool bodies alone.
