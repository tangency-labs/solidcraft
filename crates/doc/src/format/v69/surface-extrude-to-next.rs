//! A surface extrude can run To Next: its `to_next` (each wall runs on until it first meets a
//! body) is now saved and built. Older designs carry none, so they rebuild as before. The last
//! release does not know `to_next`: it warns that the design is newer, opens it, keeps the
//! feature as saved, and reads the extrude's saved distance, 0, so it shows that feature's error
//! ("extrude distance is zero") instead of its surface.
