"""Generate an original synthetic NX-container fixture: a 10 x 20 x 30 mm box.

No commercial CAD software or commercial CAD file is used. The box and its
connectivity are constructed analytically below. Fixed record layouts and the
SPLMSSTR envelope follow cadmpeg 0.6.7's published NX format notes and its
Apache-2.0 synthetic-byte helpers, not Siemens product code. The format notes
are by the cadmpeg contributors, licensed CC-BY-4.0; no document text or
source code is copied. The generator and analytic topology are original:
https://github.com/cadmpeg/cadmpeg/blob/v0.6.7/docs/formats/siemens_nx.md
https://github.com/cadmpeg/cadmpeg/tree/v0.6.7/crates/cadmpeg-codec-nx/src/test_support
See attribution/pending/nx-import.md for provenance. This is a decoder test
fixture, not evidence that NX itself accepts files written by this generator.
Run with Python's standard library; output is next to this script.
"""

from pathlib import Path
import struct
import zlib


def ref(buffer, offset, value):
    struct.pack_into(">H", buffer, offset, value)


def vector(buffer, offset, values):
    struct.pack_into(">ddd", buffer, offset, *values)


def record(kind, identity, length, references=()):
    data = bytearray(length)
    ref(data, 0, kind)
    ref(data, 2, identity)
    for offset, value in references:
        ref(data, offset, value)
    return data


# Vertex indices are binary XYZ coordinates; all coordinates below are metres.
points = [(0.01 * (i & 1), 0.02 * ((i >> 1) & 1), 0.03 * ((i >> 2) & 1)) for i in range(8)]
# Counterclockwise rings when viewed from outside the solid.
faces = [
    ([0, 4, 6, 2], (-1, 0, 0), (0, 1, 0)),
    ([1, 3, 7, 5], (1, 0, 0), (0, 1, 0)),
    ([0, 1, 5, 4], (0, -1, 0), (1, 0, 0)),
    ([2, 6, 7, 3], (0, 1, 0), (1, 0, 0)),
    ([0, 2, 3, 1], (0, 0, -1), (1, 0, 0)),
    ([4, 5, 7, 6], (0, 0, 1), (1, 0, 0)),
]
# Assign disjoint small XMT identities; 1 is the wire null reference.
BODY, SHELL, REGION = 2, 3, 4
FACE, LOOP, PLANE, FIN, EDGE, LINE, VERTEX, POINT = 10, 20, 30, 40, 70, 90, 110, 130
edges = sorted({tuple(sorted((ring[i - 1], ring[i]))) for ring, _, _ in faces for i in range(4)})
uses = {edge: [] for edge in edges}
for face_index, (ring, _, _) in enumerate(faces):
    for i in range(4):
        uses[tuple(sorted((ring[i - 1], ring[i])))].append(FIN + face_index * 4 + i)
assert len(edges) == 12 and all(len(pair) == 2 for pair in uses.values())

stream = bytearray(b"PS")
for field in [b": TRANSMIT FILE (partition) created by SolidCraft synthetic fixture", b"SCH_TEST_1_9999"]:
    stream += struct.pack(">I", len(field)) + field
stream += b"\0"
stream += record(12, BODY, 24)
stream += record(13, SHELL, 24, [(8, 1), (10, BODY), (12, 1), (14, FACE), (16, 1), (18, 1), (20, REGION), (22, 1)])
stream += record(19, REGION, 16)
for face_index, (ring, normal, x_axis) in enumerate(faces):
    face_id, loop_id = FACE + face_index, LOOP + face_index
    face = record(14, face_id, 39, [(18, face_id + 1 if face_index < 5 else 1), (20, face_id - 1 if face_index else 1), (22, loop_id), (24, SHELL), (26, PLANE + face_index)])
    struct.pack_into(">d", face, 10, 1e-9)
    face[28] = ord("+")
    stream += face
    stream += record(15, loop_id, 16, [(10, FIN + face_index * 4), (12, face_id), (14, 1)])
    plane = record(50, PLANE + face_index, 91)
    plane[18] = ord("+")
    vector(plane, 19, points[ring[0]])
    vector(plane, 43, normal)
    vector(plane, 67, x_axis)
    stream += plane
    for i, end in enumerate(ring):
        start = ring[i - 1]
        edge = tuple(sorted((start, end)))
        edge_index = edges.index(edge)
        identity = FIN + face_index * 4 + i
        partner = next(other for other in uses[edge] if other != identity)
        fin = record(17, identity, 23, [(4, 1), (6, loop_id), (8, FIN + face_index * 4 + (i + 1) % 4), (10, FIN + face_index * 4 + (i - 1) % 4), (12, VERTEX + end), (14, partner), (16, EDGE + edge_index), (18, LINE + edge_index), (20, 1)])
        fin[22] = ord("+" if start == edge[0] else "-")
        stream += fin
for edge_index, (start, end) in enumerate(edges):
    edge = record(16, EDGE + edge_index, 32, [(18, uses[(start, end)][0]), (24, LINE + edge_index)])
    struct.pack_into(">d", edge, 10, 1e-9)
    stream += edge
    line = record(30, LINE + edge_index, 67)
    line[18] = ord("+")
    vector(line, 19, points[start])
    # Vertex-index changes identify the box's known positive X, Y and Z edges.
    direction = {1: (1.0, 0.0, 0.0), 2: (0.0, 1.0, 0.0), 4: (0.0, 0.0, 1.0)}[end - start]
    vector(line, 43, direction)
    stream += line
for i, position in enumerate(points):
    vertex = record(18, VERTEX + i, 28, [(16, POINT + i)])
    struct.pack_into(">d", vertex, 18, 1e-9)
    stream += vertex
    point = record(29, POINT + i, 40)
    vector(point, 16, position)
    stream += point

payload = zlib.compress(stream, level=1)
name = b"/Root/UG_PART/UG_PART"
header = bytearray(25)
header[:8] = b"SPLMSSTR"
header[8] = 6
header += b"HEADER" + struct.pack("<I", 1) + struct.pack("<I", len(name)) + name
payload_offset = len(header) + 16
header += struct.pack("<QQ", payload_offset, len(payload))
footer_offset = len(header) + len(payload)
header[17:23] = footer_offset.to_bytes(6, "little")
result = header + payload + b"FOOTER" + bytes(8)
Path(__file__).with_name("block.prt").write_bytes(result)
print(f"Wrote {len(result)} bytes; partition {len(stream)} bytes; 6 faces, 12 edges, 8 vertices")
