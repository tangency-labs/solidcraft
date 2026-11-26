//! A design saves the bodies drawn without their edge lines (`edgeless_bodies`, by body name),
//! set with `body.edge_lines`: an overlay such as a thin label keeps no outline while the other
//! bodies keep theirs (#21975). Older designs list none, so every body draws its edges as the
//! visual style says, as before. The last release opens a design that has it with a warning
//! about the newer format and draws those bodies with their edges; saving it there drops the list.
