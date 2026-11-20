//! Thread features may save `form: "unified"` and an inch Unified designation, including its
//! optional ASME B1.1 class. Older designs have no Unified Thread form and need no rewrite;
//! their automatic metric sizing is unchanged. The last release opens this newer format with
//! a warning and preserves the feature data it cannot read (`crate::newer_file`).
