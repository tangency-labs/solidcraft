//! A design may carry review markup pictures: the bytes of each picture attached to a markup
//! (`markup.image`), a part of its own at `markups/<guid>.png` or `.jpg` in the container
//! (`markup_images`, never written as text). The markup set beside the design names each one by
//! that entry. An older design has none. The last release warns about the newer format number,
//! opens the design, and keeps those parts as they are when it saves.
