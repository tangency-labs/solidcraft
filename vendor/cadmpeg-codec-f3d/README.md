# cadmpeg-codec-f3d

`cadmpeg-codec-f3d` decodes Autodesk Fusion `.f3d` archives into `CadIr` and
encodes supported `CadIr` documents back to `.f3d`. The codec covers ZIP
container metadata, ASM B-rep topology, analytic and cached NURBS geometry,
body transforms, design and sketch records, construction history, and
appearances. Multi-document `.f3z` archives decode into one merged document.

<!-- generated: capability f3d -->

Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d)).

<!-- /generated: capability f3d -->

## Install

```sh
cargo add cadmpeg-codec-f3d cadmpeg-ir
```

## Decode

```rust,no_run
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.f3d")?;
    let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;

    for loss in &result.report().losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    println!("{} bodies", result.ir().model.bodies.len());
    Ok(())
}
```

The result holds the decoded `CadIr` and a `DecodeReport`. Read
`report.losses` before trusting geometry. Set
`DecodeOptions::depth` to `DecodeDepth::Container` for archive metadata
without B-rep decoding, or to `DecodeDepth::Geometry` for geometry and
products without design intent.
`F3dCodec::inspect` returns classified ZIP entries and B-rep header facts.

For an F3Z archive, `F3dCodec::open_archive` accepts a `DecodeContext` and its
root `View`. Use `cadmpeg-core` to create the context and its `DecodeArena`.
It scans the directory, manifest, and reference data without
model decoding. Keep the input and the context's expansion arena alive while
the returned `F3zSession` is in use. `members()` lists stable archive paths,
roles, uncompressed lengths, and outgoing references. The model root is first.
Drawing and skipped entries have no member model.

Call `decode_member(id)` on caller-owned threads. Each call uses private decode
counters and a private scan cache. Its result holds the document, report body,
and losses, or the member error. Submit one result for every listed entry to
`compose`, in any arrival order. Composition follows the root's occurrence
reference order and returns the same finalized `DecodeResult` as `Codec::decode`.

Member charges are speculative until composition. Composition replays them
at their original traversal positions against one cumulative archive budget.
Unreachable members add no decode charges. If a changed budget alters optional
history enrichment, a charge refuses, or derived address spaces need rebinding,
composition runs that member through the same decoder with the current budget.
This keeps the output and resource refusal site independent of arrival order.
Install a thread-local observer with `observe_f3z_members`. Its
`F3zMemberObserver` callbacks bracket `F3zPhase::Scan`, `Classify` and
`SessionOpen`, then each member decode. Callers measure elapsed time between
callbacks. The preparation callbacks have default implementations. The outer
scan callbacks also occur for a single-document F3D input, before its kind is
known.

Classification inflates and scans each F3D member once. The same pass reads the
dialect layers, the member references and the placement role. The session keeps
those owned facts, charged as entities, and drops the scan. Opening the member
list reads no member again.

The session retains the compressed source archive and directory metadata.
Member scans use temporary arenas. Reopened scans preserve their original
address-space identities. Raw member expansions, parsed streams and scan caches
are released after each operation. Member results retain only the decoded
payload, source fidelity and compact resource trace. Normal `Codec::decode`
decodes reachable members during composition and moves each definition into
the output once. Composition releases template caches and unused member
results after reference traversal.

`F3zDecodedMember::retained_bytes_estimate()` counts compact JSON bytes for the
document, report fields and source fidelity, plus the member ID and trace
storage. Failed results use their error text length; skipped results have no
payload. The estimate excludes allocator overhead and unused capacity.
`F3zSession::largest_member_working_bytes_estimate()` gives the largest readable
model archive length plus its inner directory's declared expanded lengths;
it excludes parsed records and output. These deterministic estimates are not
RSS bounds. Callers can gate later parallel decode by the working estimate plus
retained result estimates.

## Encode

```rust,no_run
use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.f3d")?;
    let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;

    // Edit supported fields in result.ir().

    let mut output = File::create("part-edited.f3d")?;
    F3dCodec
        .plan(
            EncodeInput::new(result.ir(), Some(result.source_fidelity())),
            TargetRequest::Inherit,
        )?
        .write_to(&mut output)?;
    Ok(())
}
```

Decode retains the source archive and a semantic baseline. Encoding with the
retained `source_fidelity` sidecar replays an unchanged archive byte for byte
and patches supported edits in place. Encoding `CadIr` without that sidecar
writes a canonical archive for the supported source-less profile.

## Data model

The Design body map selects every B-rep blob that contributes bodies.
Extension does not choose the model: `.smb` and `.smbh` are both ASM streams.
History streams are selected independently by the ASM header flag, and every
selected history graph is decoded. Without body-map bindings, a unique
history-bearing stream or a single BREP is the only fallback. The decoder
frames each selected SAB slice and builds each topology chain from bodies
through vertices and points. ASM model-space lengths become millimetres in
`CadIr`. Directions, ratios, angles, knots, weights, and UV parameters keep
their native scale.

Typed transfer covers analytic carriers, cached NURBS, selected procedural
definitions, design and sketch records, typed ASM history, source attributes,
and Protein appearances. Records that block faithful transfer land in
`DecodeReport::losses`. Useful passthrough carrier bytes stay as
`UnknownRecord` values. Failed SAB framing or geometry decoding still returns
container metadata and retained source data, with blocking geometry and
topology losses.

Generic appearance transparency maps to the opacity channel of the CADIR base
colour. The RGB channels stay unchanged. An absent transparency property keeps
the colour carrier's alpha. Prism transmission colour, attenuation distance,
and IOR do not define a scalar opacity.

## Documentation

- [API documentation][docs]
- [Format support][support]
- [Format notes][spec]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

Autodesk and Autodesk Fusion and other product names are trademarks of their
respective owners. cadmpeg uses them only to identify the file formats this
codec targets and is not affiliated with, endorsed by, or sponsored by any CAD
vendor. See the [clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-f3d
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d
