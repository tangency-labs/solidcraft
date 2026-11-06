//! A design's `visibility` can also keep the rest of the browser's view of it: the Origin
//! folder's eye (`origin_hidden`), finished sketches whose profiles are not shaded or whose
//! dimensions are shown (`hidden_profiles`, `shown_dimensions`, sketch ids) and locked bodies
//! (`locked_bodies`, body names). Older designs need no transformation: without them profiles
//! are shaded, dimensions hidden and nothing is locked, as before, and the Origin follows the
//! Hide Origin by Default preference until the design records its own. The last release asks
//! for an update; it would ignore them and lose only that view.
