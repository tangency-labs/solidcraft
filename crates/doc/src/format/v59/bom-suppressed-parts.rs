//! A numbered bill of materials view saves `show_suppressed_parts: true` in its
//! `assembly.item_numbers` entry when it was numbered listing the parts a configuration switches
//! off (no bodies, a suppressed feature of their own), which every list otherwise leaves out.
//! Older designs need no transformation: a numbering without it reads false, the list as numbered
//! before. The last release asks for an update instead of dropping it on save.
