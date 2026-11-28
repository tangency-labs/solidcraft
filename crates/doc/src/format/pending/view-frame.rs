//! A design can have its own FRONT and TOP: `view_frame` (set by Set as Front / Top on the axes
//! navigator), the frame a saved render camera's angles are in, the frame each named view's angles
//! are in (`named_views[].frame`), and the frame of the Home view (`home_view.frame`). Older designs
//! have none: their views, Home and the orbit follow the up axis preference, as before, and a named
//! view or Home with no frame turns into the design's own frame when it is applied. The last release
//! asks for an update instead of meeting it: it opens a design that has these with a newer-format
//! warning and keeps the frames for the next save.
