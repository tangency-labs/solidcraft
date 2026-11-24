//! Align now keeps the input faces' persistent names through exact transform history.
//! Older designs may name projected edges with Align's output face ordinals instead;
//! both rc.4 and 0.45.0 ordinals are recovered only at a unique exact edge through the
//! saved point on that Align's body. No JSON rewrite can infer their different orders.
//! The last release opens designs carrying this change with a newer-format warning
//! and preserves what it cannot read (`crate::newer_file`).
