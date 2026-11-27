//! A component that holds a KiCad board opened from a `.kicad_pcb` is linked to that file, and its
//! link names the built-in reader (importer format `kicad`, no plugin) with the options the board
//! was read with, so Update Links reads the file again with the same ones. Older designs have no
//! such link and keep their boards as they are. The last release reads the record as an importer
//! plugin's that is not installed: it opens the design and keeps the board's bodies, Update Links
//! says it can't read the file again, and saving the design there keeps the record.
