//! Split Body by sketch curves (#1552, #7492): a `split` feature's `line` may name any curve of its
//! `sketch` (an arc, a spline), not only a straight line; `curves` lists more curves joined end to
//! end with it into one chain; `extend: false` cuts with the curves as drawn instead of running an
//! open chain's ends on past the body. Each is saved only when given (`extend` only when false).
//! Older designs need no transformation: their `line` is a straight line, which still splits as
//! its plane. The last release opens such a design with a warning; it refuses a `line` that is not
//! straight ("not a straight line"), builds a chain from `line` alone, splits a straight line by
//! its plane whatever `extend` says, and keeps what it can't read when it saves.
