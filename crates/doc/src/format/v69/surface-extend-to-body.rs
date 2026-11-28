//! An extended surface can stop at a whole body: its `up_to` can now be `body`, a body's name.
//! Older designs carry none, so they rebuild as before. The last release does not know it: it warns
//! that the design is newer, opens it and keeps the feature as saved, without building it.
