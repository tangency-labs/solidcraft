//! Match can retain the earlier tensor interpolation basis, evaluator depth,
//! and continuation order when aligned subdivision changes only topology.
//! Older Match definitions omit the optional continued_surface record and
//! retain their ordinary current-evaluator fit. An older reader warns and
//! preserves the continuation record it cannot read.
