# Saved HDR resources

A named CPU or GPU render setup can carry its Radiance HDR environment inside the
design. Saving works without an image host or physical GPU. The saved design is
self-contained in both JSON and the design container: reopening or rendering does
not need the original HDR file, a working directory or a temporary extracted file.

This adds resource persistence to the existing numerical job contract. It does not
add a new renderer, colour conversion, texture format, material assignment or
durable body/face identity.

## Inspect, capture, then reuse

`render.hdr.inspect` reads one bounded file through the caller's existing VFS scope
and validates its entire HDR stream. It returns only metadata:

```json
{"sha256":"<64 lowercase hex digits>","width":2048,"height":1024,"encoded_bytes":1234567,"primaries":[0.64,0.33,0.30,0.60,0.15,0.06,0.3127,0.3290]}
```

`primaries` is null when the source does not declare them. Inspect does not change
the document, geometry, revision, dirty state or undo history. Source bytes and
local paths are absent from the reply.

`render.settings.capture_hdr` captures bytes and a complete setup edit atomically:

```json
{
  "name":"CPU HDR studio",
  "set":{
    "width":1600,"height":900,"exposure_ev":1.25,
    "job":{"method":"cpu_raytrace","options":{"primary_samples":4,"lights":[],"shadows":true}}
  },
  "path":"studio.hdr",
  "expected_sha256":"<digest returned by inspect>",
  "rotation_rad":0.3,
  "intensity":1,
  "samples":16
}
```

The command takes the same top-level `set` patch as `render.settings.set`. Omission
preserves existing fields; a supplied `job` replaces the entire job. The resulting
job must be typed CPU/GPU, with `hdr` and `environment` absent/null. All numerical,
method, name and setup-count validation happens before reading the file. The
capture then verifies the optional expected digest, complete source, quotas and
method's colour policy before publishing anything. A changed file, denied path,
bad parameter, invalid source or exceeded quota leaves the document and history
unchanged. The picker workflow always supplies `expected_sha256`.

The reply is `{name, setup, resource}`, where `resource` is the metadata above.
One effective capture is one undo step; repeating identical bytes, controls and
setup is a no-op that preserves redo. A SHA-256 digest deduplicates the original
encoded bytes. Different rotation, intensity or colour-policy choices belong to
each setup, so several CPU/GPU setups can share one source.

After capture, edit the saved setup using `render.settings.set`; there is no file
read. An inline setup with an embedded reference resolves against its current
document's resources. Copying that setup alone into another document without the
resource fails clearly. A complete design carries both.

## Saved controls and colour policy

| Method | Required `options.hdr` fields | Bounds and interpretation |
|---|---|---|
| CPU raytrace | `resource`, `rotation_rad`, `intensity`, `samples` | Rotation magnitude ≤1,000,000 radians; intensity 0..64; samples 1/4/16/64; source must declare exact sRGB/D65 primaries |
| GPU path | `resource`, `rotation_rad`, `intensity`, `assume_linear_srgb` | Rotation magnitude ≤1,000,000 radians; intensity 0..100; declared primaries must match the existing GPU sRGB/D65 tolerance; an undeclared source requires explicit `true` |

Capture defaults to rotation 0, intensity 1, CPU samples 16 and GPU assumption
false. The CPU command rejects the GPU-only assumption key, including false/null;
GPU rejects the CPU-only samples key, including null. The saved HDR DTO requires
all fields, making its intent explicit on reopen. Neither method accepts a source
with other declared primaries. The flag records an interpretation, not a colour
conversion. Original header exposure and channel corrections remain unchanged.

`RenderSetup.exposure_ev` remains the only saved exposure. CPU lowering copies it
into raytrace options and sets legacy outer EV to zero. GPU uses the common setup
EV. An HDR replaces the ambient term; directional, point, sun and area lights keep
their existing semantics, and the background remains a display backplate.

## Storage, admission and reopening

Each resource is immutable and keyed by the SHA-256 of its encoded source. The
document retains at most 64 distinct resources and 64 MiB of encoded capacity;
each source is nonempty and at most 16 MiB, at most 2048×1024 pixels, 2:1 and the
existing standard `-Y +X` orientation. Headers, every scanline and finite corrected
radiance are checked before insertion. Invalid types, hashes and required
references fail; none is treated as an optional cache.

Plain JSON stores `{kind:"radiance_hdr",data:"canonical base64"}`. Before the
migration reader builds a JSON value tree, a borrowed RawValue probe limits the
resource count, detects duplicate keys and counts base64 characters, including
legal JSON escapes, against per-source and aggregate quotas. It does not allocate
an unescaped HDR string or decoded payload to measure them. The typed reader then
checks canonical padding, source content and digest. This is specific HDR payload
admission; it is not a new general allocation bound for all document JSON keys.

The container writes one required `blobs/<sha256>.hdr` entry per resource. Small
metadata contains `kind`, an empty `data`, and `hdr_blob`. A separate bounded raw
probe rejects duplicate references, inline payloads and oversized metadata strings
before constructing the value tree. Opening checks blob size before allocation,
the container hash and the resource key, then hydrates the strict JSON form before
normal document validation. Missing, corrupt, wrongly typed or oversized blobs
prevent opening the authoritative design. No entry is extracted to disk. Existing
archive, expanded-size and overall 256 MiB design-file limits remain in force.

The new pending descriptor is `render-hdr-resources`. Historical format numbers
and migration steps are unchanged. Old setups without resources or HDR references
retain their meaning; a build that lacks the new pending descriptor refuses the
newer design rather than discarding required content. No third-party image asset
or dependency is added; `raw_value` enables a feature of the existing serde_json.

Document clones, undo/redo and both CPU/GPU lowering share the immutable source
buffer through Arc. Each job still charges its full encoded capacity and existing
decode/table/worker budget. Sharing does not relax admission. Check and Start use
the same preparation path and source digest, with no file IO for saved resources.

## Existing runtime combinations

Transient body/face bindings remain available without pretending they are durable.
For a legacy request (`settings.job` absent/null), CPU environment accepts
`{resource,rotation_radians,intensity,samples}`. GPU accepts
`{kind:"embedded_hdr",resource,rotation_rad,intensity,assume_linear_srgb}`. These
references use the document's saved bytes; missing resources fail. CPU refuses
`resource` combined with `path` or `expected_sha256`. GPU's tagged object rejects
unknown fields. Existing file variants also accept `expected_sha256`, so an
inspected file changed before Check or Start is refused.

The format has no resource removal or garbage-collection command yet. Replacing a
setup retains old resources; the quota fails explicitly when exhausted. Saved
designs and autosave document snapshots remain independent of the original file.
A command-only crash journal before a checkpoint still needs the path recorded by
`capture_hdr` and, when supplied, its exact digest. It is not self-contained.

Regression sources cover offline save, no-op/undo/redo, file change, path scope,
JSON and container reopen after deleting the source, CPU colour policy, strict
hash/type/reference errors, escaped payload and count/byte limits, shared buffers,
and Check/Start lowering. Native CPU coverage compares actual geometry lighting;
the GPU protocol fixture checks admission and captured inputs without claiming
physical GPU image validation.
