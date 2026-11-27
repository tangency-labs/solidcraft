//! A design saved with `"linked_bodies_left_out": true` may also leave out the bodies of its
//! linked STEP files and the mesh bodies of its linked designs: a STEP link's Import feature
//! then has `"step": ""` and no `"records"`, and the Mesh Import feature beside a linked
//! design's Import feature has `"meshes": []`. Opening the design reads them from the linked
//! files again. Older designs keep these bodies in the file, so nothing is rewritten. A build
//! from before this change warns about the newer format number and opens such a design with
//! each linked STEP component in error and each linked design without its mesh bodies, until
//! Update Links reads its file; it keeps what it does not know when saving.
