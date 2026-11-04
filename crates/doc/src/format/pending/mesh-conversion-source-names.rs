//! Mesh conversions save `reconstruction`: `analytic_v1` uses partial arcs and combined analytic parts
//! with v4 source-bound names. An absent value or `legacy_prism` keeps the original constant-section
//! fitter, geometry-derived names and conversion feature owner, including after resaving.
//! The unreleased `source_bound_names` Boolean is accepted as an alias: true selects `analytic_v1`,
//! false selects `legacy_prism`. New commands save the canonical version name.
//! The previous release asks for an update instead of reading this format.
