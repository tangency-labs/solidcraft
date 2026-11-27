//! A drawing's views save which configuration of the design each draws (`settings.configuration`,
//! #13048): `"active"`, or a row of the design's configuration table by the row's id, followed
//! down its projected views until one sets its own. An older drawing has none, so each view draws
//! the design's active configuration. The last release warns that the drawing is newer, opens it
//! and keeps the setting it can't read.
