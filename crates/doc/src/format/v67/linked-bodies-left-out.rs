//! A design may save `"linked_bodies_left_out": true` (`file.save`'s `linked_bodies: false`).
//! The Import feature of each linked design then has `"step": ""` and no `"records"`: the file
//! keeps the link, and opening the design reads the bodies from the linked design files again.
//! Without the field (every older design) each link's bodies are in the file as before, so
//! nothing is rewritten. The last release warns about the newer format number and opens such a
//! design with each linked component in error until Update Links reads its file; it keeps the
//! field it does not know when saving.
