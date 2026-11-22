//! Amalgam Tag features (#18799): `{"type": "amalgam_tag", "remove": [..], "add": [..], "new": [..],
//! "seat": frame?}` labels bodies of the design as a tool for Amalgamate (each list left out when
//! empty); `seat`, a frame as Align's `from_frame` saves one, is the tool's origin (left out: the
//! design's origin). Older designs have none and are unchanged. The last release warns about the
//! newer format number, opens what it can read and keeps the rest.
