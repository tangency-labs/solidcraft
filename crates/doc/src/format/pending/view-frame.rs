//! A design can have its own FRONT and TOP (`view_frame: {front, top}`, principal world axes at
//! right angles; the view cube's Set as, #6455). Its standard views, Home and the orbit follow
//! them instead of the up axis preference; geometry doesn't move. Older designs need no
//! transformation: without `view_frame` they use the preference, as before. The last release
//! refuses a design saved with this (the newer format number).
