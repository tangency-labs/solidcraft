//! A design can keep named versions and branches in the file (`versions`).
//!
//! A version is a frozen copy of the parametric design. A branch is a workspace that can be
//! edited, started from a version. The workspace that is open is the rest of the file. A design
//! saved before this opens as one workspace, Main, with no versions. A release that does not
//! know this change refuses the file.
