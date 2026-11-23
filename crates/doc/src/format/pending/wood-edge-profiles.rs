//! Saves the wood_edge_profile timeline feature: the body's input edge points, profile
//! kind, size expressions and tangent-chain flag. Older designs have no routed-edge feature.
//! The last release warns about the newer format and retains features it cannot read.
//! Model caches keep each routed edge's length, including when intermediate timeline states
//! are omitted. Older caches have no routed lengths.
