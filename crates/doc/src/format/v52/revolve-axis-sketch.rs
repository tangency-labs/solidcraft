//! `AxisRef::SketchLine` gains an optional `sketch`: the feature id of the sketch that owns the
//! axis line, so a revolve axis picked in another sketch resolves there instead of in the
//! profile sketch (#6548). Designs saved before this omit the field; they read unchanged and
//! keep resolving the line in the profile sketch, exactly as before. A design saved with it
//! carries the newer format number, which the last release refuses.
