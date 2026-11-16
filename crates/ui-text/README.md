# solidcraft-ui-text

Translated interface text: the message catalogs, lookup by stable id, label resolution,
`{name}` substitution, the language choice and the per-thread language, plus right-to-left
display order (`bidi`). It holds no app types and depends on no UI toolkit, so any front end
or shared component can draw translated text without depending on the app.

The host owns the catalogs (their JSON and the list) and installs them once. English, the
catalog with the id `en`, is the fallback for a message a translation lacks; a message neither
has reads as its id. With nothing installed every id reads as itself and the language is `en`.

The current language is per thread. `scope(id)` sets it until the returned guard drops, and
restores the caller's on the way out; outside any scope it is English. `language()` reads it, to
carry into a worker thread.

## API

| Item | What it does |
|---|---|
| `Catalog::new(id, name, json)` | A language's messages, parsed the first time they are read (`const`, for a `static`) |
| `install(locales)` | Install the host's catalogs, once, before the first message is read |
| `locales()` | The installed catalogs |
| `tr(id)` | The current language's message, else English's, else the id |
| `tr_label(label)` | A source label (English text or an id) in the current language |
| `format(id, args)` | `tr(id)` with each `{name}` replaced in one pass; values are literal, unknown names stay |
| `scope(id)` / `language()` | Set the thread's language for a scope; read it |
| `resolved_labels()` | Within the guard, `tr_label` shows labels verbatim (already translated text) |
| `Locale` | One language: `for_language`, `text`, `label`, `format`, `name`, `display_name`, `catalog` |
| `locale()` | The thread's current `Locale` |
| `SYSTEM` | The preference value that follows the system language |
| `is_supported(id)` | Whether a catalog with that id is installed |
| `system_language()` / `match_system_tags(tags)` | The installed catalog the OS (or browser) asks for |
| `effective_language(preference)` | The catalog id a preference resolves to |
| `pin_system_language()` | Make the system language English, for tests outside this crate |
| `each_english_message(f)` | Every English `(id, text)` |
| `bidi::visual`, `bidi::visual_template`, `bidi::has_rtl`, `bidi::screen_order` | Right-to-left text in the order a left-to-right layout draws correctly |

## Installing catalogs

Each catalog is a `static`, and the list is a `'static` slice with English first:

```rust
use solidcraft_ui_text::Catalog;

static ENGLISH: Catalog = Catalog::new("en", "English", include_str!("locales/en.json"));
static GERMAN: Catalog = Catalog::new("de", "Deutsch", include_str!("locales/de.json"));
pub const LOCALES: &[&Catalog] = &[&ENGLISH, &GERMAN];

fn main() {
    solidcraft_ui_text::install(LOCALES);
    // ...
}
```

Install before the first lookup (the system language is chosen from the installed list the
first time it is read). Installing the same list again does nothing; a different list is
ignored and logged. A test harness outside this crate calls `pin_system_language()` so its
tests read English on every PC.
