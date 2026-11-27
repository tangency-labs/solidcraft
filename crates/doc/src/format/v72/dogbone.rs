//! Designs save dogbone features (`dogbone`): the picked inside corner edges and faces, the bit
//! diameter, the style (normal, minimal with its gap, T-bone with its side), the largest corner
//! angle relieved and the bit's axis. Older designs have no dogbones and need no migration. The
//! last release opens a design with one with a warning about the newer format and keeps the
//! feature it can't read, rather than dropping the reliefs.
