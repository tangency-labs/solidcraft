//! A point cloud can save `"up": "y"` (#8609): its file has Y up, and its points (and their
//! `origin`) are saved already turned a quarter turn about X, so the file's Y is the design's Z.
//! `pointcloud.insert` and `pointcloud.edit` with `up: y` set it; `up: z` turns the points back
//! and saves nothing, which is what every older design has, so nothing in them is rewritten. The
//! last release does not know the key: it opens a design that has it with a warning about the
//! newer format and shows the cloud standing up as saved, but drops the key, so after it saves the
//! design the cloud no longer remembers that its file had Y up, and a reload reads it lying down.
