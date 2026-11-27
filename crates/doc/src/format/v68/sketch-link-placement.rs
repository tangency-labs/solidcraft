//! A sketch link to geometry of another component can keep the exact placement it reads
//! (`placement: {source: [occurrence ids], author: [occurrence ids], seen?: 4×4}`): the
//! occurrence path of the source (`A:2`, not whichever copy of the part comes first), the
//! placement of the sketch's own component it was made in, and the transform its geometry was
//! read with. A link can also be frozen (`frozen: true`: its geometry stays as it is until it is
//! resumed), hidden (`hidden: true`: not drawn or picked, nothing else changes), and a face link
//! can leave out members that were deleted (`omit_edges: [edge family names]`: the persistent
//! names of their source edges, so exactly those stay deleted when the source moves or is
//! resized). Older designs need no transformation: a link without `placement` reads its source
//! where the sketch's component sees it, as before, and one without the other fields is live,
//! shown and whole. The last release refuses a design with this format number and asks for an
//! update, rather than resolving a placed link against the wrong copy of a part.
