//! Thread features save `form: "unified"` for an inch Unified designation, including when
//! creation or editing inferred it from the designation; the optional ASME B1.1 class is kept.
//! Older Threads without a form keep historical inference: a Unified designation remains a
//! refused metric thread, while forms such as trapezoidal still infer as before. No rewrite is
//! needed and automatic metric sizing is unchanged. The last release opens this newer format
//! with a warning and preserves the feature data it cannot read (`crate::newer_file`).
