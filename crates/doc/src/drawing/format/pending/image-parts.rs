//! A drawing's image files are saved as parts of its container of their own: each image record
//! names its part (`part`, `blobs/<sha256>.<png|jpeg>`) and keeps an empty `data`, where the bytes
//! were base64 before. An older drawing has them in `data` and reads as before; nothing is
//! rewritten as it opens. The last release warns about the newer drawing format, opens what it can
//! read of the drawing and keeps the rest as it came when it saves.
