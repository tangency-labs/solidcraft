//! Cross-component face anchors in a part save the source-to-author component placement at
//! creation, so assembly moves do not reshape the part. Source geometry and body offsets stay
//! associative. Older anchors without `placement` retain their live placement relationship;
//! redefining their sketch plane captures a new context. The last release refuses this newer
//! format and asks for an update instead of dropping the captured placement.
