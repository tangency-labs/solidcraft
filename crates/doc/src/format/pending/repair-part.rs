//! A design can hold Repair Part (#8371): a `repair` feature with the `body` it repairs, the
//! widest `gap` between free edges it sews (a length expression) and `patch_holes` (default false,
//! not saved: the loops of free edges left are patched and sewn in). Older designs need no
//! transformation: they have no such feature. The last release refuses a design saved with this
//! (the newer format number).
