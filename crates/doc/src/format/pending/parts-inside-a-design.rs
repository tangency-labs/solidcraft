//! A design file may store parts inside itself (experimental, #6892): each is a whole part or
//! assembly file, a container part `internal/<name>` beside `document.json`, and the component that
//! shows it is linked by the path `.~internal/<name>` (`crate::internal`). The link keeps the part's
//! bodies in its Import feature, as every link does. An older design has no such part and no such
//! link and needs nothing. The last release warns about the newer format, opens the design, shows
//! those components with the bodies their links keep, reports their links as missing (it looks for
//! `.~internal/<name>` beside the design) and keeps the `internal/` parts when it saves.
