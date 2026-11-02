//! A sheet metal rule can set the K-factor its bends unfold by from a press brake chart's 90°
//! bend deduction or bend allowance (`unfold_method`: `k_factor`, the default, `deduction` or
//! `allowance`, with the 90° value as a length expression in `unfold_value`; #6794). Each is
//! saved only when it isn't the default (an empty value), in the design's rules; sheet bodies
//! keep the K-factor they unfold by, as before. Older designs need no step: a rule without them
//! unfolds by its `k_factor`, as before. The last release refuses a design saved with this (the
//! newer format number).
