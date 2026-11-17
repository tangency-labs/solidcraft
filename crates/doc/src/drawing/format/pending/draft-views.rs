//! A view can be a draft view (`"kind": "draft"`, #5425): a view with no model and no source, for
//! the sketches it owns, drawn on the paper's own axes at its scale. It saves its scale and its place
//! and none of a view's model settings. An older drawing has none. The last release can't read a
//! view kind it doesn't know: a drawing with a draft view tells it to update SolidCraft to open it.
