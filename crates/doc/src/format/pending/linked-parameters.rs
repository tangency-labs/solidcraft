//! A design can use the user parameters of other designs (Link Parameters): `linked_designs`
//! lists each linked design's path (relative to the design's folder), which of its parameters come
//! in (`scope`) and their values as last read. Older designs need no transformation: they link no
//! parameters. The last release refuses a design saved with this (the newer format number).
