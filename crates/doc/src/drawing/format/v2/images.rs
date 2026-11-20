//! Drawings save their images (#5426): `images` holds each file once, with its format, pixel size and
//! bytes (base64), and `sheet_images` each place one sits, with its sheet, its rectangle and the file's
//! id. The file's path is never saved. An older drawing has neither. The last release refuses a drawing
//! saved with them, as a newer format.
