//! `assembly.rigid_groups` lists components whose contents move as one, an imported assembly's:
//! `{"id": 41, "name": "ESC_ARK4in1", "component": 7}` holds every occurrence inside component 7,
//! at any depth, where it is in its parent, as one rigid joint per placement used to (the import
//! made those before). Older designs have no `rigid_groups` and keep their rigid joints, so
//! nothing in them is rewritten. The last release opens a design that has them with a warning
//! about the newer format and keeps the key as it came; there the parts of such an import can be
//! moved apart, as after `component.unlock`.
