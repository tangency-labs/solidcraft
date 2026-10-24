//! Tapped holes may store `thread_depth`, a length expression independent of the drill depth.
//! An absent value retains the full useful cylindrical wall used by older designs. An explicit
//! depth starts at that wall's entry, excludes the drill point and entry recess, and is refused
//! when it is not positive and finite or exceeds the available cylindrical wall. Older readers
//! would ignore the quota and describe a full-depth thread, so the design format must advance.
