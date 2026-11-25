# solidcraft-format

Versioned file formats: what an app needs to read every file it ever wrote, to open a file a
newer build of it wrote without losing what it can't read, and to package a file with its binary
payloads. It holds no app types; each app registers its own families of files and their versions,
and keeps its own payload encodings.

A file starts with `"format": "<prefix>N"`. A family's versions are, in order: the steps written
out by hand for its first formats, then one folder per format (`v<N>/`, one file per change, each
with a JSON step when older files are rewritten as they open), then the changes in flight
(`pending/`), which make the next number until a release folds them into `v<N>/`. App code only
knows the current format: all translation lives in the steps.

## API

| Item | What it does |
|---|---|
| `Family { app, noun, prefix, legacy_prefix, untagged, aliases, formats }` | One kind of file: its tag, its messages, its versions |
| `Formats { hand, folded, pending }` | A family's versions: hand-written steps, folded formats and their changes, pending changes |
| `Part { name, step }` | One change: its file's name, and its JSON step if it has one |
| `Step<E>` | `fn(&mut Value) -> Result<(), E>`: a JSON-level step, returning the app's error |
| `Alias { format, name, known_as }` | Another spelling of a change name that files of one format may carry |
| `Family::current()` | The format this build writes |
| `Family::format_string(n)`, `Family::version_of(tag)` | A format's tag, a tag's format (the legacy prefix read too) |
| `Family::pending_names()` | The changes in flight, written into a file's `format_pending` |
| `Family::take_version(&mut json)` | A file's format and its `format_pending`, taken out before anything else is read; a file not of the family is refused |
| `Family::newer(version, applied)` | `Some(Newer)` when a newer build wrote the file: a number past this build's, or a change it doesn't know |
| `Family::upgrade(&mut json, version, applied)` | Run the steps that bring the file to this build's format, each once |
| `Newer { format, reads_up_to, unknown_changes }` | How a file shows a newer build wrote it |
| `FormatError` | This crate's refusals; the app's error takes it with `From` |
| `build::lists(manifest, families)` | For a build script: the generated `FOLDED` and `PENDING` lists and a module per change file |
| `newer::NewerFile` | What a file a newer build saved holds that this build doesn't read: `new(app, &newer)`, `notice`, `warnings`, `report`, `keep_section`, `keep_element`, `compare` (the file against what was read of it), `restore_into` (put it all back on saving), `mentions`, `needs_container`, and the container's unknown `parts` and `container` |
| `newer::leave_out_unreadable`, `newer::read_around` | Leave out, and keep, the top-level sections a file can't be read with |
| `newer::short`, `newer::looks_default` | A reader's error's first line; a value a writer leaves out |
| `container::Writer` | A ZIP container being written: `part`, `blob` (content-addressed, `blobs/<sha256>.<ext>`), `room`/`space` for optional caches, `finish` |
| `container::Reader` | One being read over a seekable source, its directory checked on opening: `document`, `limited_part`, `blob`, `limited_blob`, `has_part`, `part_names`, `hydrated` |
| `container::Limits` | The app's budgets: entries, expanded size, per-part limits, Deflate level, file size |
| `container::Compression`, `container::Thumbnail`, `container::digest`, `container::is_container` | Fast or compact payloads; a PNG picture keyed to its file; the SHA-256 name of bytes; the ZIP signature |

## Registering a family

The app's build script lists the folders:

```rust
use solidcraft_format::build::Family;
// format 1 is the first record, so its folded formats start at 2 (`base` is the last format before them)
const FAMILIES: [Family; 1] = [Family { folder: "src/format", base: 1, list_prefix: "" }];
let lists = solidcraft_format::build::lists(&manifest, &FAMILIES)?;
// write lists.code to OUT_DIR, rerun on each of lists.watch
```

and its format module includes them, with its error type:

```rust
type Part = solidcraft_format::Part<AppError>;
include!(concat!(env!("OUT_DIR"), "/format_parts.rs"));
pub const DESIGN: solidcraft_format::Family<'static, AppError> = solidcraft_format::Family {
    app: "App", noun: "design", prefix: "app/", legacy_prefix: None, untagged: None, aliases: &[],
    formats: solidcraft_format::Formats { hand: &[], folded: FOLDED, pending: PENDING },
};
```

A change file is `v<N>/<name>.rs` or `pending/<name>.rs`, named with lowercase letters, digits and
dashes, starting with a `//!` note (what is saved, what an older file gets, what the last release
does with a file that has it), and with `pub fn step(v: &mut Value) -> Result<(), AppError>` when
older files are rewritten as they open.
