//! Split Body by sketch curves (#1552, #7492): a `split` feature's `line` may name any curve of its
//! `sketch` (an arc, a spline), not only a straight line; `curves` lists more curves joined end to
//! end with it into one chain; `extend: false` cuts with the curves as drawn instead of running an
//! open chain's ends on past the body. `curves` is saved only when there are more curves, and
//! `extend` only when false; a saved chain always names its first curve in `line`. Older designs
//! need no transformation: their `line` is a straight line, which still splits as its plane. The
//! last release opens such a design with a warning, ignores `curves` and `extend`, splits a straight
//! `line` by its plane, refuses any other `line` ("not a straight line"), and keeps what it can't
//! read when it saves.
