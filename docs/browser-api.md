# Browser command API

After the browser app starts, `window.solidcraft` controls the design open in that tab.
Calls use the same command ids, parameters, control handlers and undo history as the desktop
control channel. No separate engine or server is started.

| Call | Result |
|---|---|
| `solidcraft.request(method, params?)` | Any [control method](control-protocol.md), such as `ui.inspect`, `document.inspect` or `engine.batch` |
| `solidcraft.execute(command, params?)` | Run an engine command without opening its interactive dialog |
| `solidcraft.commands()` | Discover commands, their parameters and current enablement |
| `solidcraft.inspect(params?)` | Inspect the active document, optionally with `{measure: true}` |
| `solidcraft.buttons()` | The mouse buttons the app holds now, as a pointer event's `buttons` bitmask (1 primary, 2 secondary, 4 middle); returns at once, not a promise |

Every call but `buttons()` returns a promise. It resolves to the control reply's `result`, or rejects with an
`Error`. Omitted or `null` parameters become `{}`. Parameters must be JSON serializable and
at most 16 MiB of JSON. At most 256 requests may be pending; excess requests reject without
running. Requests run on the UI thread between frames. A hidden or suspended tab can delay
completion; keep it active while driving the app. A reload ends the session and its pending work.

In the browser's developer console, on an empty design:

```js
await solidcraft.commands();
await solidcraft.request("ui.home", {open: false});
await solidcraft.execute("solid.box", {
  length: 30, width: 20, height: 10, body_name: "Browser API box"
});
await solidcraft.inspect({measure: true});
await solidcraft.execute("edit.undo");
await solidcraft.execute("edit.redo");
await solidcraft.request("ui.inspect");
```

An agent with JavaScript evaluation access to this tab can make the same calls. The API does
not register WebMCP tools or expose a network listener. Scripts executing in the page can use
the API with the page's browser permissions. File paths refer to the browser's virtual file
system, not arbitrary files on the user's computer. Native-only operations remain subject to
the browser host's limitations; this API does not grant desktop filesystem or window access.

## A host page that keeps the designs

`?embed=editor` runs the app as an editing embed: the ribbon, browser, timeline and properties,
without the application bar or start page, and with nothing kept in the browser. A page that
frames it from the same origin reaches this API as `frame.contentWindow.solidcraft`, keeps the
designs itself and moves them with three [control methods](control-protocol.md):

```js
const app = frame.contentWindow.solidcraft;
const kept = await app.request("document.export"); // {name, revision, dirty, format: "json", text}
// ...the page stores kept.text, and says so:
await app.request("document.saved", {revision: kept.revision});
// ...and later puts it back, timeline and parameters as saved:
await app.request("document.import", {text: kept.text, name: "Bracket"});
```

The 16 MiB limit on parameters applies to `document.import`'s `text`. A design that links other
design files keeps those links as paths; the files themselves are not in `text`.

## A host page that keeps the files

`?host=parent` (in a frame) or `?host_origin=https://host.example` (naming a cross-origin parent)
hands the files to the page that frames the app, as with the other Craft apps' web bridges. The
page hears, by `postMessage` to it and only at the origin it is at (or `?host_origin=` names):

| Message | Meaning |
|---|---|
| `solidcraft:ready {version}` | The app started. |
| `solidcraft:save {name, bytes, id, export?}` | A save or export the app would have downloaded: `bytes` is an `ArrayBuffer` (a design is `.sc`; exports carry `export: true`). |
| `solidcraft:dirty {dirty}` | Unsaved changes appeared (`true`) or went away (`false`). |
| `solidcraft:failed {error}` | The app did not start, or a save was not kept in time. |

The page answers a save it kept with `solidcraft:saved {id}`; a save not answered within 30
seconds is reported as `solidcraft:failed`. The page hands a file in with
`solidcraft:open {name, bytes}`, where `bytes` is an `ArrayBuffer` or `Uint8Array` of at most
256 MiB and `name` a plain file name: the app opens it like File > Open (a design, or STEP or a
mesh as a new design). Any other message is ignored, and so are messages from any other origin.
An unframed page is unchanged; `?sample` is left to the host.

```js
const app = document.querySelector("iframe");
window.addEventListener("message", (event) => {
  if (event.origin !== HOST_ORIGIN) return;
  const m = event.data;
  if (m.type === "solidcraft:save") {
    keep(m.name, m.bytes, m.export === true);
    app.contentWindow.postMessage({type: "solidcraft:saved", id: m.id}, HOST_ORIGIN);
  }
});
function openFile(name, bytes) {
  app.contentWindow.postMessage({type: "solidcraft:open", name, bytes}, HOST_ORIGIN);
}
```

## Verify a browser build

Build the app with `trunk build` in `apps/solidcraft-web`, then run from the repository root:

```sh
cargo xtask web-api-test --site dist/web
```

The test starts a local server and headless Chrome, discovers commands, creates a box in the
live document, checks undo and redo, and verifies error handling and the pending-request limit.
It also checks that Ctrl+Tab and Ctrl+Shift+Tab are left to the browser, which switches tabs with them.
Last, it checks that the app's mouse buttons follow the browser's after chords (two buttons held
at once), a drag let go outside the window, a release the page never heard of and the window losing focus.
Set `CHROME` to a Chrome or Chromium executable if it is not installed in a usual location.
