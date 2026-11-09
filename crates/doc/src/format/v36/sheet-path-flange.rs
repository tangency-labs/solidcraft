//! A design can hold a path flange (`sheet_path`): a chain of edge flanges on a sheet edge
//! that follows an open chain of sketch lines drawn across it. Older designs need no
//! transformation: they have none. The last release refuses a design saved with one (the newer
//! format number).
