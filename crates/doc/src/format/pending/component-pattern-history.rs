//! Copies of a component Pattern keep their stored occurrence ids and assembly references while
//! the Pattern is suppressed or behind the history marker. Older designs without saved copies
//! keep their existing empty list until the Pattern is built. Older builds are warned before
//! opening the new format because they may remove inactive copies and their references.
