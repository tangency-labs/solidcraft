//! Surface-first corner joins record partial or full overlap on a rip's lap.
//!
//! A corner is identified by its two bend joints within a build. Saved corner
//! parameters carry named sheet rims, `<panel>:rim.<rim>:<top|bottom>`, not
//! kernel edge names, picked points or build-local joint indices. Panel maker
//! IDs follow feature copies. Length expressions preserve an
//! explicit gap or partial overlap; a missing gap uses the rule, and full overlap
//! follows the current thickness.
//! New corner parameter records write an explicit kind, including Butt. A missing
//! or null kind reads as Butt; this does not rewrite existing miter lap records.
//!
//! Edge flanges may also save a `corners` list using the same named sources and
//! length expressions. An absent or empty list keeps the rule-driven corner
//! closing; empty lists are omitted when saved. Each override includes a bend
//! made by that flange feature, without saving a future maker ID.
//!
//! Existing miter laps keep their Butt behavior and need no rewrite. The last
//! release opens newer designs with a warning and preserves data it cannot read.
