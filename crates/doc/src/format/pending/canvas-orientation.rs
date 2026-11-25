//! A canvas or decal saves the EXIF orientation it shows its picture by (#19112): `orientation`, 2 to
//! 8, read from the file when it is inserted, so a photo a phone stored on its side shows upright and
//! at its shape. A picture with no turn saves no field. Older designs need no transformation: without
//! the field a canvas shows its picture as stored, as it always did, so a design opens looking exactly
//! as it was saved. The last release opens the design with a notice that it comes from a newer format
//! and asks for an update, shows every canvas as stored, and keeps the field when it saves the design.
