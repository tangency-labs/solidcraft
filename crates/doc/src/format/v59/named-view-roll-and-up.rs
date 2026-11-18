//! Named Views now keep their camera's roll and world up axis, so a design can restore a saved
//! view with Y up or a rotated horizon. Older files have neither field; they still open Z up with
//! zero roll and need no conversion. The previous release warns about the newer format and
//! keeps the view fields it cannot read when an updated design is opened there.
