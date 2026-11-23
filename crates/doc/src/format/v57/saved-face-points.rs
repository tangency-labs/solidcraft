//! Saved model caches keep the face points resolved before each feature ran, shared between
//! states. Patterns and mirrors of cuts can reuse these points after reopening a design.
//! Older caches without the points are discarded and the design's history is evaluated again.
//! The last release warns about the newer format, opens what it can and keeps unknown data
//! when saving (crate::newer_file).
