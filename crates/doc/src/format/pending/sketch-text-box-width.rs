//! A sketch text's style can say how wide the box it is laid out in is (`width`, in mm): its
//! lines are aligned to that box's edges, so left, center and right justify against them.
//! Older designs need no rewrite: without the key a text hangs from its start, as every text
//! did before. The last release opens a design that carries the key with a warning, keeps what
//! it can't read, and sets such a text as it always did.
