// SPDX-License-Identifier: Apache-2.0
// Shared by the difftest shims of units `topology.brep_local_operation` and
// `heal.stitch_p1` (issue #1462): per-object attribute tags before an edit and the tags
// the edited B-rep's faces and edges carry after it. Mirrored by `tag_objects` /
// `put_tags` in crates/difftest/src/units/heal_stitch_p1.rs. Layout of the tail:
//   n_faces, per face of GetFaces: n, then its attribute ids ascending
//   n_edges, per edge of GetEdges: the same
// The ids of a set are sorted, so the order of an object's attribute list (which of two
// merged objects survives first) never enters the comparison.
#ifndef SC_ATTRIBUTE_TAGS_H
#define SC_ATTRIBUTE_TAGS_H

#include <SmAttribute.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmTArray.h>
#include <algorithm>
#include <vector>

namespace
{
// The first tag id: face `i` (in the given order) gets `SC_TAG_BASE + i`, edge `i`
// `SC_TAG_BASE + 500 + i`, each its own `SmLongAttribute` (value the id, SM_AB_COPY).
const ULONG SC_TAG_BASE = 10007;

void tag_objects(const SmContext& ctx, const SmTArray<SmFace*>& faces, const SmTArray<SmEdge*>& edges)
{
    for (ULONG i = 0; i < faces.GetSize(); ++i)
        faces[i]->AddAttribute(new (ctx) SmLongAttribute(SC_TAG_BASE + i, (long)(SC_TAG_BASE + i), SM_AB_COPY));
    for (ULONG i = 0; i < edges.GetSize(); ++i)
        edges[i]->AddAttribute(
            new (ctx) SmLongAttribute(SC_TAG_BASE + 500 + i, (long)(SC_TAG_BASE + 500 + i), SM_AB_COPY));
}

// `n id...` of an object's attribute ids, ascending.
void put_attribute_ids(std::vector<double>& o, const SmAObject* obj)
{
    SmTArray<SmAttribute*> attrs;
    obj->GetAttributes(attrs);
    std::vector<double> ids;
    for (ULONG i = 0; i < attrs.GetSize(); ++i)
        ids.push_back((double)attrs[i]->GetAttributeID());
    std::sort(ids.begin(), ids.end());
    o.push_back((double)ids.size());
    o.insert(o.end(), ids.begin(), ids.end());
}

// The tail: the attribute ids of every face (GetFaces order), then of every edge.
void put_tags(std::vector<double>& o, SmBrep* b)
{
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    o.push_back((double)faces.GetSize());
    for (ULONG i = 0; i < faces.GetSize(); ++i)
        put_attribute_ids(o, faces[i]);
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    o.push_back((double)edges.GetSize());
    for (ULONG i = 0; i < edges.GetSize(); ++i)
        put_attribute_ids(o, edges[i]);
}
} // namespace

#endif
