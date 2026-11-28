# Server edition: `solidcraft-cli serve`

Status: draft, for review. The cloud freeze still holds for everything this document doesn't
name.

The server edition is SolidCraft **self-hosted**: you run it on a machine of your own, and it
serves the browser app and your design files from a folder on that machine. It is open source like
the rest of the app (MIT or Apache-2.0, in this repository), in line with the split the project
already has: what runs on your own computers is open, and only our hosted sync service is the
closed, paid side. Its scope is one installation for one person or one team (single tenant).

## The case it is built for

A designs folder lives in a git repository on a small headless box, for example
`~/Sources/pi-case/mechanical` on a mini server on your tailnet. You start the server there when
you want it:

```sh
solidcraft-cli serve --dir ~/Sources/pi-case/mechanical --web ~/solidcraft-web
```

and open the app from a browser on any laptop. A folder browser shows that folder's subfolders and
designs; you open one, edit it in the browser, and **Save** writes it straight back to the same
file in the repository, in place, as an ordinary `.sc` file. `git status` on the box shows the
change like any other edit. Point `--dir` at one design instead, and that design opens at start:

```sh
solidcraft-cli serve --dir ~/Sources/pi-case/mechanical/case.sc --web ~/solidcraft-web
```

The same process serves MCP over HTTP, so an agent on the box or on the tailnet can model in the
same folder with the tools `solidcraft-cli mcp` gives it on stdio. The browser still models
locally, in WebAssembly, as the web app always has: the server keeps the files, and does the
modelling only for agents.

Docker is the second way to run it, for a long-running service with a volume ("Docker" below).

## Quick start

1. Get `solidcraft-cli` (it is in every desktop release package, and `cargo install --path
   apps/solidcraft-cli` builds it) and the web app, `solidcraft-web-<version>.zip` from the same
   release, unzipped into a folder (here `~/solidcraft-web`).
2. Make a token and keep it in a file only you can read:

   ```sh
   umask 077; openssl rand -hex 32 > ~/.solidcraft-token
   ```

3. Start the server on the folder:

   ```sh
   solidcraft-cli serve --dir ~/Sources/pi-case/mechanical --web ~/solidcraft-web --token-file ~/.solidcraft-token
   ```

   It listens on `127.0.0.1:8787`, this computer only.
4. **From another machine on your tailnet**, put Tailscale's HTTPS in front of it:

   ```sh
   tailscale serve --bg 8787
   ```

   and open `https://<machine>.<tailnet>.ts.net/`. The page asks for the token once and keeps it
   in that browser. (`--listen 100.x.y.z:8787`, the box's Tailscale address, also works, but it is
   plain HTTP: the browser app refuses to send the token over plain HTTP to another computer, so
   use `tailscale serve` for the browser and the address only for agents that accept the risk.)
5. **On the same computer only**, `--no-token` skips the token: it is refused with any `--listen`
   but a loopback address, and then only requests naming this computer (`localhost`,
   `127.0.0.1`, `[::1]`) in their `Host` are served.

## Shape

```text
     browser on a laptop                       agent (Claude Code, Codex, OpenCode, ...)
     app in WebAssembly, models locally        MCP over HTTP, token
            │ HTTPS (tailscale serve, Caddy, nginx)        │
┌───────────┴───────────────────────────────────────────────┴─────────────────────┐
│ solidcraft-cli serve --dir FOLDER --web DIR        listens on 127.0.0.1:8787      │
│                                                                                  │
│   GET  /                     the browser app (static files, no token)            │
│   GET  /api/v1/health        {"ok":true}  (no token; for health checks)          │
│   *    /api/v1/folder/...    the folder browser                (token)           │
│   *    /api/v1/files/...     read, save, delete design files   (token)           │
│   POST /mcp                  MCP streamable HTTP               (token)           │
│        ├── MCP session 1 ── thread ── solidcraft_mcp::Server ── headless design  │
│        └── ...  (at most --max-sessions, idle ones end after --session-idle)     │
│                                                                                  │
│   every file call (HTTP API and MCP tools) confined to FOLDER                     │
└──────────────────────────────────────────────────────────────────────────────────┘
```

- **One implementation of the tools.** Each MCP session is the same `solidcraft_mcp::Server` the
  stdio command runs, over a headless session. The HTTP layer only carries JSON-RPC messages in and
  out (`Server::handle_line`); no tool is written twice.
- **One thread per MCP session.** A headless session is not shared between threads, so each MCP
  session owns a thread and its own design; HTTP connections hand it messages over a channel. Two
  agents never edit each other's open design. They share the folder, like two people sharing a
  network drive.
- **No new runtime.** It is `std::net` with a thread per connection and caps on both, the way the
  app's control channel serves. No async runtime, no TLS library, and no crate that wasn't already
  in the dependency tree (the request head is parsed by hand, strictly; SHA-256 for the token).
- **The web app is the same build** the static image and solidcraft.io serve. The server serves
  its files with the caching rules of `packaging/docker/nginx.conf` (no-cache for `index.html`,
  `sw.js`, `build.json`; a year for Trunk's hashed files).

## Configuration

```text
solidcraft-cli serve --dir FOLDER|DESIGN.sc [--listen ADDR:PORT] [--web DIR] [--token-file FILE | --no-token]
                     [--max-sessions N] [--session-idle MINUTES] [--max-upload SIZE]
                     [--allow-origin ORIGIN]... [--allow-plugins] [--param-check off|warn|strict] [--compact]
```

| Option | Default | What it does |
| --- | --- | --- |
| `--dir FOLDER` | required | The folder served: any existing folder, a git checkout included. Every design the server reads or writes is in it. `--data` is another name for it (the Docker image uses `--data /data`). |
| `--dir DESIGN.sc` | | Serve the design's folder, and open that design at start: in the browser app, and in every new MCP session. |
| `--listen ADDR:PORT` | `127.0.0.1:8787` | Where to listen. Loopback unless you say otherwise; a Tailscale address, or `0.0.0.0:8787` in a container. |
| `--web DIR` | none | The browser app's files (the release's `solidcraft-web-<version>.zip`, unzipped, or `dist/web`). Without it, `/` answers 404 and only the API and MCP are served. |
| `SOLIDCRAFT_SERVE_TOKEN` / `--token-file FILE` | required | The access token (see below). There is no `--token`: arguments are visible to every user in the process list. |
| `--no-token` | off | No token, for use on this computer only (see the quick start). |
| `--max-sessions N` | 4 | MCP sessions at once. Each has its own design in memory. |
| `--session-idle MINUTES` | 30 | An MCP session with no request for this long ends, and its unsaved design with it. |
| `--max-upload SIZE` | 256MB | The largest file the HTTP API takes in one request. |
| `--allow-origin ORIGIN` | none | Another origin allowed to call the API from a browser (see the threat model). |
| `--allow-plugins` | off | Let MCP sessions list, install and run plugins (see "MCP over HTTP"). |
| `--param-check`, `--compact` | as `mcp` | The same as for `solidcraft-cli mcp` (docs/mcp.md). |

The server runs in the folder (it is the working folder), so an agent can say `open case.sc` as
well as the full path.

### The token

The server will not start without one, unless `--no-token` on loopback. It reads
`SOLIDCRAFT_SERVE_TOKEN`, or the first line of `--token-file FILE`. A token shorter than 32
characters is refused at start-up; `openssl rand -hex 32` makes a good one.

Every request to `/api/v1/` (but `health`) and to `/mcp` must carry it as
`Authorization: Bearer <token>`. Both the token given and the server's are hashed with SHA-256 and
the digests compared without an early exit, so the time a refusal takes says nothing about how
much of a guess was right. A refused request gets `401` with `WWW-Authenticate: Bearer` and
nothing else, after a short wait that makes guessing slower still.

The browser app asks for the token once, keeps it in that browser's storage for this server's
origin, and sends it as the same header, only over HTTPS or to this computer. There are no cookies,
so a page on another site can't make the browser use the token (no CSRF).

## HTTP API

All under `/api/v1/`, JSON unless the call moves a file's bytes. Paths are relative to the
folder, `/`-separated, percent-encoded in the URL. Hidden files and folders (`.git`, `.env`, any
name starting with a dot) are never listed, read or written; links are not listed.

| Call | Answer |
| --- | --- |
| `GET /api/v1/health` | `200 {"ok": true}`. No token, no other detail. |
| `GET /api/v1/info` | `{"version", "commit", "mcp", "max_upload", "extensions", "open"}`; `open` is the design `--dir` named, or `null`. |
| `GET /api/v1/folder/{path}` | One folder, for the folder browser: `{"path", "folders": [{"name", "path"}], "files": [{"name", "path", "size", "modified", "etag"}]}`, each sorted by name. `GET /api/v1/folder` is the top. |
| `PUT /api/v1/folder/{path}` | New folder: makes `path` inside a folder that exists (`201`; `200` when it is already there). |
| `GET /api/v1/files` | Every design file in the folder and below (at most 10,000, 16 levels deep), sorted by path, for agents and scripts. |
| `GET /api/v1/files/{path}` | The file's bytes, `application/octet-stream`, with `ETag`. |
| `PUT /api/v1/files/{path}` | Save the request body as the file. With `If-Match: <etag>` it saves only when the file is still that version (`412` otherwise), so a browser never silently overwrites a save made meanwhile; `If-None-Match: *` saves only a new file (Save As to a name that is free). Answers `{"path", "size", "modified", "etag"}`. |
| `DELETE /api/v1/files/{path}` | Delete the file (`If-Match` as for `PUT`). Folders are never deleted. |

Errors are `{"error": "<what went wrong, in words>"}` with `400` (a bad path or request), `401`,
`403` (an origin not allowed), `404`, `409` (a folder where a file was named, or the other way),
`411` (no `Content-Length`), `412`, `413` (too large), `415` (not a design file's extension), `503`
(too many connections or sessions).

**Saves are in place and atomic.** The body goes to a new hidden temporary file in the same folder
(`.<name>.upload-…`), is flushed to disk, then renamed over the file. A crash or a dropped
connection mid-upload leaves the old file as it was, and a failed save removes its temporary file.
The result is the same file name with new contents, so git sees an ordinary change.

Only design and model files are served and saved: `.sc` (and the older design extension), STEP
(`.step`, `.stp`), `.stl`, `.3mf`, `.obj`, `.gltf`/`.glb`, `.dxf`, `.svg`, `.fcstd`, `.3dm`.
Anything else answers `415`, so a repository's code, keys or notes are never served.

## The browser app with a server

When the page comes from `serve` (it asks `GET api/v1/health` at start; the static image and
solidcraft.io answer 404 there), it gets a **folder browser** for the server's folder, the way a
desktop app has a file dialog:

- the first time, it asks for the server's token and keeps it in this browser for this origin
  (**Forget token** removes it). It sends it only over HTTPS or to this computer;
- it shows one folder at a time: its subfolders (click to go in, a path bar to go back up) and its
  designs (click to open). `.git` and other hidden entries never show;
- **New folder** makes a folder where you are;
- **File › Open** shows the folder browser for a design, with a way on to the browser's own files
  and the computer's;
- **File › Save As** shows it to pick the folder, with the name to save under; a name already
  taken there asks before it replaces that file;
- **Save** on a design opened from the server writes it back to the same file (`PUT` with
  `If-Match`). If someone saved that file meanwhile (another browser, an agent, a `git pull`), the
  save is refused, the design stays unsaved, and the message says to Save As under another name:
  it never overwrites their change silently;
- with `--dir DESIGN.sc`, that design opens when the page starts.

To the engine such a design is the file `/server/<path>`, the way a design on the computer's disk
is `/disk/<id>/<name>` (`apps/solidcraft-web/src/disk.rs`): it saves there like anywhere, and what
it wrote is forwarded to the server (`apps/solidcraft-web/src/server.rs`). The service worker
leaves `api/` requests alone. Without a server nothing changes: designs stay in the browser's
storage, exactly as now.

## MCP over HTTP

`/mcp` follows the MCP streamable-HTTP transport (protocol revisions 2025-06-18 and 2025-03-26):

- `POST /mcp` with one JSON-RPC message (or a batch) as `application/json`. A request is answered
  with `200` and the JSON-RPC response as `application/json`; a notification or a response gets
  `202` and no body. The server never opens an SSE stream: every tool runs to its end and answers
  in the same reply, as on stdio.
- `initialize` starts a session. Its reply carries `Mcp-Session-Id`, and every later request must
  send it back. A request without it (other than `initialize`) gets `400`; an ended or unknown
  session gets `404`, which tells the client to initialize again.
- `DELETE /mcp` with `Mcp-Session-Id` ends that session at once.
- `GET /mcp` answers `405`: there is no server-to-client stream.

The tools, their arguments and their results are those of docs/mcp.md, headless. What differs:

- **Files.** Tool calls reach only the folder (and the server's private scratch folder, where
  `screenshot` without a `path` draws). It is `--allow-path FOLDER` plus `--require-allow-path`,
  with the same resolution: `..` and links that lead out are refused, and a new file must go into
  a folder that exists. `--allow-any-path` does not exist here. The recovery folder is left out,
  so the `doc.recovery_*` commands answer that it is out of reach.
- **Plugins** are off unless the server was started with `--allow-plugins`: `plugin.*` commands
  are refused, through `execute`, `batch` and scripts alike, no plugin tools are listed, and no
  plugins folder is read. With `--allow-plugins`, plugins work as in a headless `mcp` session, in
  `--plugins-dir DIR` (or the service user's own plugins folder).
- **Screenshots** are drawn as `mcp --headless` draws them: on a GPU when the server has one
  (`SOLIDCRAFT_GPU` picks), otherwise on the CPU rasterizer, which the container uses.
- **Network.** A design that names a network path reaches nothing: the server's network policy is
  the default one, which allows no host (docs/mcp.md).

### Connecting an agent

Any MCP client that speaks streamable HTTP and can send a header connects with the URL and the
token. With the token in `SOLIDCRAFT_TOKEN` in the agent's environment, and the server at
`https://box.tailnet.ts.net` (an agent on the box itself can use `http://127.0.0.1:8787/mcp`):

Claude Code (`claude mcp add`, or `.mcp.json` in a project):

```sh
claude mcp add --transport http solidcraft https://box.tailnet.ts.net/mcp \
  --header "Authorization: Bearer $SOLIDCRAFT_TOKEN"
```

```json
{
  "mcpServers": {
    "solidcraft": {
      "type": "http",
      "url": "https://box.tailnet.ts.net/mcp",
      "headers": { "Authorization": "Bearer ${SOLIDCRAFT_TOKEN}" }
    }
  }
}
```

Codex (`~/.codex/config.toml`):

```toml
[mcp_servers.solidcraft]
url = "https://box.tailnet.ts.net/mcp"
bearer_token_env_var = "SOLIDCRAFT_TOKEN"
```

OpenCode (`opencode.json`):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "solidcraft": {
      "type": "remote",
      "url": "https://box.tailnet.ts.net/mcp",
      "headers": { "Authorization": "Bearer {env:SOLIDCRAFT_TOKEN}" }
    }
  }
}
```

A client that only speaks stdio can reach it through a bridge such as `mcp-remote`; this project
doesn't ship one, and doesn't run agents itself.

## Docker

For a long-running service rather than a folder you serve on demand. The image builds from the
same `packaging/docker/Dockerfile` with `--target server`: the CLI built for Linux, the web app
baked in at `/srv/solidcraft`, running as an unprivileged user (uid 10001) as

```sh
solidcraft-cli serve --dir /data --listen 0.0.0.0:8787 --web /srv/solidcraft
```

with the token from `SOLIDCRAFT_SERVE_TOKEN`. `packaging/docker/compose.yaml` gets a second
service, `solidcraft-server`, under the `server` profile, so `docker compose up` keeps starting
only the static app:

```yaml
  solidcraft-server:
    profiles: [server]
    build: {context: ../.., dockerfile: packaging/docker/Dockerfile, target: server}
    env_file: [{path: server.env, required: true}]   # SOLIDCRAFT_SERVE_TOKEN=...
    ports: ["127.0.0.1:${SOLIDCRAFT_SERVER_PORT:-8787}:8787"]
    volumes: [solidcraft-data:/data]                  # or a bind mount of a checkout
    read_only: true
    tmpfs: [/tmp]
    security_opt: [no-new-privileges:true]
    cap_drop: [ALL]
volumes:
  solidcraft-data:
```

The port is published on the host's loopback, for a reverse proxy or `tailscale serve` on the same
host. The existing `solidcraft-web` image and service are unchanged. The book's "Self-hosting with
Docker" gets a section on the server, with a Caddy example.

## Threat model

**What it protects.** The design files in the folder, everything else on the machine (the rest
of the repository included: code, keys, `.git`), and the machine itself.

**Who it trusts.** Whoever holds the token can do everything the server can: read, change and
delete every design in the folder, and run any modelling command. There is one token and no
users. The operator of the machine and the reverse proxy are trusted.

**Who it doesn't.** Everyone else who can reach the port: the tailnet or LAN, the internet if it
is exposed, other web pages open in a token holder's browser, and the contents of design files
(they are data from other people).

| Threat | Answer |
| --- | --- |
| A request without the token | Refused (`401`) before its body is read, on every route but the static files and `health`. |
| Guessing the token | 256 bits from `openssl rand`; at least 32 characters required; digests compared without an early exit; a wait after each refusal. |
| Sniffing the token or the designs | The server speaks plain HTTP and binds to loopback by default. Reach it through HTTPS (`tailscale serve`, Caddy, nginx); the browser app refuses to send the token over plain HTTP to anything but loopback. |
| `--no-token` reached from elsewhere | Refused at start with any `--listen` but loopback; then only requests whose `Host` names this computer are served, so a web page that reached it through a DNS name it controls (DNS rebinding) is refused. |
| Reading or writing outside the folder | Every path is judged as text: no `..`, `.`, empty, hidden (`.x`) or backslash segments, no NUL or `:`, a design file's extension. Then it is resolved (links followed) and must still lie in the folder (`vfs::PathScope`, the same check `--allow-path` uses), and a save never writes through a link at the final name (it renames a new file over it). MCP tools go through the same scope. Tests cover `..`, encoded `..`, absolute paths, hidden files, `.git`, and links inside the folder that lead out. |
| The rest of a repository | Hidden entries (`.git`, `.env`) are never listed, read or written; files without a design extension (source, keys, notes) are never served. |
| A malicious design file | It is opened by the same engine as on the desktop, in a session that can reach only the folder and no network host. A failure in one MCP session's thread ends that session, not the server. |
| A web page in the user's browser calling the server (CSRF, DNS rebinding) | No cookies, so the browser never adds the token on its own. Requests with an `Origin` header must come from the server's own origin (its `Host`) or an `--allow-origin`, as the MCP transport requires. |
| Memory or disk exhaustion | Request heads at most 16 KiB and 64 headers; MCP bodies at most the stdio line limit; uploads at most `--max-upload`; no chunked request bodies (`411`), which also rules out request smuggling through a proxy; at most 64 connections and `--max-sessions` sessions; 30 s for each read. The disk is the operator's to size. |
| Running programs | No shell, no subprocesses: `serve` runs nothing but its own engine. Plugins are off unless `--allow-plugins`. |
| Designs leaving the server | They go only to the client that asked, in the reply to its request. The server makes no outgoing connections: no telemetry, no update check, no crash upload. |
| The container | Read-only root file system, all capabilities dropped, `no-new-privileges`, an unprivileged user, `/tmp` as tmpfs, the data volume the only writable path. |

**Not in scope.** Several people with different rights, audit logs, per-design permissions,
TLS inside the process, and protection from someone who has the token: see the open questions.
A local process on the box that swaps a folder for a link between the check and the write could
redirect one call (the same limit `--allow-path` documents); it already has the user's access.

SECURITY.md says MCP's allowed folders aren't a security boundary, because an agent there has a
shell of its own. A network client of `serve` has no shell, so its folder *is* a boundary, and
getting a tool call or an API call past it is a security issue; SECURITY.md says so.

## Testing

`apps/solidcraft-cli/tests/serve.rs` starts the real binary on a free loopback port with a
temporary folder and checks, over real HTTP:

- a request with no token, a wrong one and another scheme is refused (`401`), on the API and on
  `/mcp`; a start without a token, with a short one, or with `--token` is refused;
- path escapes are refused: `..`, percent-encoded `..`, an absolute path, a hidden file, and links
  inside the folder that lead out, through the HTTP API and through the MCP `save` and `open`
  tools;
- the folder browser on a git checkout: `.git`, dotfiles and non-design files never show, folders
  list and are made, and a save in place leaves the same file name with no temporary file beside it;
- an MCP session over HTTP: `initialize`, then a tool call that builds a box and `save`s it into
  the folder; the file is then on disk, and plugins are refused;
- `--dir DESIGN.sc` serves the design's folder, `info` names it, and MCP sessions start with it;
- `--no-token` serves loopback, refuses another `Host` and a foreign `Origin`, and won't start on
  `0.0.0.0`;
- a save and load round trip: `PUT`, `GET` the same bytes, `If-Match` with a stale ETag refused;
  an upload over `--max-upload` (`413`) and a chunked body (`411`) write nothing.

The Docker image gets `packaging/docker/smoke-test-server.sh`: it runs read-only, the app and its
caching headers are served, `health` answers, the API and MCP refuse a request without the token,
and a design saved with it is listed and reads back.

## Upgrade path

- **The folder is just files**: `.sc` designs in folders, often in git. Back it up, move it, or
  open the same files in the desktop app. A newer release opens older designs as the desktop app
  does; a design saved by a newer release opens in an older one with the desktop app's warning (it
  never refuses to open).
- **The API is versioned in its path** (`/api/v1/`); a change that breaks a client gets
  `/api/v2/`, with v1 kept for at least a release. The browser app and the server come from the
  same release, so they agree; agents speak MCP, which negotiates its own protocol version.
- **The Docker image** updates with `docker compose pull && docker compose up -d`. MCP sessions
  end on restart; saved designs don't.
- **Tokens** change by editing the file or env file and restarting the server.

## Open questions for the owner

1. **Several users.** One token means one shared identity. Do we want users (each with their own
   token, and a name on each save) in a first release, or is one token per server enough for now?
2. **Auth beyond one token.** Leave SSO and OIDC to the reverse proxy (`tailscale serve` already
   knows who is on the tailnet; oauth2-proxy, Authelia), or build some of it into `serve`?
3. **Read-only tokens.** A second, read-only token (browse and open only, MCP sessions that can't
   save) would cover viewers. Worth it in the first cut?
4. **Git.** Saves are plain files, so git sees them. Should the server ever commit, or show what
   changed since the last commit? This draft leaves git to the user.
5. **Shared editing.** Two people editing the same design: last save wins, with `If-Match`
   stopping a silent overwrite. Locking a design while someone has it open, as the desktop app's
   "open elsewhere" lock does, is a possible next step.
6. **Image name and channel.** `ghcr.io/tangency-labs/solidcraft-server`, published with each
   release like `solidcraft-web`, or built from source only until it is out of draft?
7. **TODO (your message was cut off):** "we'd need a way to do the…": to be filled in with the
   rest of what you asked for.
