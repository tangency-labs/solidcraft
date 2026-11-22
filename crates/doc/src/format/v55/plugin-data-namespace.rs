//! A design may carry plugin data: files a plugin keeps with the design, under
//! `plugins/<plugin-id>/` in the container (`plugin_data`, base64 by `<plugin-id>/<path>`, in a
//! plain JSON design). They are kept and saved back untouched whether or not the plugin is
//! installed, and opening a design never needs the plugin. An older design has none. The last
//! release warns about the newer format number, opens the design, and keeps those parts (and the
//! JSON field) as they are when it saves.
