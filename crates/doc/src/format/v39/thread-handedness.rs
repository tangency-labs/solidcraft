//! Hole and Thread features may store `handedness: "left"`. An absent value still means
//! right-handed, so older designs need no rewriting. New-format gating prevents an older
//! reader from silently dropping the direction and rebuilding a left thread as right.
