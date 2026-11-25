# RFC 0018: Send to App, sending the open design to a local program in its own format

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-25 |
| Apps | SolidCraft (the target list and the launch rules live in the shared plugin crate, so other Craft apps can use them) |
| Discussion | [#22333](https://github.com/tangency-labs/solidcraft/issues/22333) |
| Extends | [RFC 0002](0002-plugins.md) (plugins: a new `launch` permission), RFC 0017 (marketplace: one amendment and one tier rule, section 7), proposed in [#22332](https://github.com/tangency-labs/solidcraft/pull/22332) and not yet on main, so its file is linked there until that PR merges |
| Related | [RFC 0015](0015-plugin-ui-workspaces.md) (where a plugin puts its button), [#16089](https://github.com/tangency-labs/solidcraft/issues/16089) (send STEP to slicers, remember the format per slicer), [#16440](https://github.com/tangency-labs/solidcraft/issues/16440) and [#17078](https://github.com/tangency-labs/solidcraft/issues/17078) (the name of the sent file), [#10980](https://github.com/tangency-labs/solidcraft/issues/10980) (open in a print server), [#21070](https://github.com/tangency-labs/solidcraft/issues/21070) (manufacturing intent and export), [#2951](https://github.com/tangency-labs/solidcraft/issues/2951) (link to Drive or S3), [#22311](https://github.com/tangency-labs/solidcraft/issues/22311) (plugin permissions) |

A person should be able to click one button and have the design they have open handed to another program on the same computer: an app that uploads it to a file-sharing site, a viewer, a CAM tool, a converter. SolidCraft already does this for slicers, with a 3MF or STL file. This RFC says how to do it for any local program, in the format that program wants, and how a plugin can offer such a button without ever starting a program itself.

The request that started this: someone wants to publish their designs to an online file-sharing site from SolidCraft. Plugins have no network access (RFC 0002 reserves `net`), so a small desktop app on the same computer does the upload, and SolidCraft's job is one click that hands it the file.

**One idea runs through every section: the host launches, the plugin only names.** A plugin may say "here is a program and how to call it". SolidCraft exports the file, shows the user the exact command, asks once, and starts the program itself. A plugin never starts a process, never writes the command line at run time and never sees the result of the launch except through a status the host chooses to give it.

The RFC has two parts. **Part 1** (sections 3 to 5) is the quick solution: what works today, what the first host change is, and the smallest change that lets a plugin declare a target. **Part 2** (sections 6 to 12) is the full solution.

## Decisions

These are proposed here and are open for comment until the RFC is accepted.

1. **Slicer becomes one kind of target** in a list of local targets, and "Send to App" is a second kind (section 6). The existing slicer pipeline is reused; slicers do not change for their users.
2. **A target is data:** a program, an argument template, and the formats it takes. Never a script SolidCraft runs on the user's behalf (section 4).
3. **Launch is argv only, never a shell.** No `sh -c`, no `cmd /c`, no string pasted into a command line (section 4.3).
4. **The first change needs no plugin:** a host "Send to App" the user sets up themselves (section 4).
5. **A plugin declares targets in its manifest; the host does the export and the launch.** This needs a new `launch` permission, approved by the user once per target, shown with the exact command template (section 5). Each send shows the real command (section 8).
6. **Tiers apply.** In the marketplace, `launch` is a high-risk grant: Listed and Custom items get the second confirmation (section 7).
7. **Nothing comes back by default.** Reporting back is opt-in, uses a one-use token, and adds no new inbound listener (section 9).
8. **Authentication belongs to the local app, never to SolidCraft** (section 10).
9. **The web app does not launch anything;** it offers a download (section 11).
10. **`net` stays reserved.** This RFC does not need it (section 12).
11. **No new Preferences rows.** Targets are managed in their own dialog.

## 1. Goals and non-goals

**Goals**
- One click from the open design to a local program, in a format that program accepts, including SolidCraft's own file and STEP, not only a mesh.
- A plugin can add a "Send to <App>" button without any new power to start programs on its own.
- The user sees the exact command, as a template with sample values, before approving a target, and nothing runs before they approve it. Every send then shows the actual command with its real values: before it starts when the target is set to ask each time (the default for an agent's request and for a Listed or Custom plugin's target), and in the result toast and the send history for every send. The only things that can differ between sends are the four placeholders (section 4.2).
- Everything is a command with a schema, so an agent can list targets and send, and no programmatic call opens a dialog.
- Existing slicer behaviour, files and settings keep working.

**Non-goals**
- A shell, a scripting language or a general "run this program" command for plugins or agents.
- Network access for plugins (`net` stays reserved; section 12).
- Building the upload app. It is not ours (section 10 describes what such an app would look like).
- Replacing Save As, Export or the marketplace.
- Sending to a program on another computer.

## 2. What exists today

This is how the code on main behaves; the RFC builds on it.

- **Commands:** `file.send_to_slicer` and `file.slicers`, plus a `slicer` parameter on `file.save_mesh`. The pipeline is one function in `crates/engine/src/cmd/file_send.rs`; the programs are in `crates/engine/src/slicers.rs`.
- **Extension point:** the code already names it. `TargetKind` has one value, `Slicer`, and a comment says a later Send to App would add a kind and send through the same pipeline.
- **Launch:** a program is started with `std::process::Command` and a list of arguments, with no shell, and its input and output closed. A slicer that must be started first can be waited for until it is ready (up to ten seconds). The program is then left running; the exit status is not read.
- **Which programs:** only the slicers SolidCraft detects (desktop entries, app bundles, program folders, `PATH`) and programs the user added under Custom… in the send window. Scripts and the control channel can start only those; any other path is refused, and an agent's request asks first and names the program.
- **A user-added program** is kept in an unpaged setting (`custom_slicers`), must be an existing executable or app bundle, and gets the file path as its last argument. There is no argument template.
- **The file:** 3MF (default), STL or 3MF with face tags: meshes only. By default it is written into a per-user folder (`~/.cache/solidcraft/slicer` on Linux, the matching cache folder on macOS and Windows), readable only by the user, under a name that stays the same for the same design, so a second send replaces it. Files older than seven days are removed at start.
- **A folder of the user's choice:** `copy_to` (and "Keep in a folder" in the window) saves `<design name>-v1.3mf`, `-v2.3mf`, … and never overwrites; `copy_overwrite` keeps one file instead (#18311).
- **Web:** disabled, "can't be started from the browser". The one exception is the EasyPrint row, which uploads after the user agrees.
- **Plugins:** there is no permission that starts a program or opens a link. `net` is reserved and refused at install. A plugin gets command groups, folder grants (`grant:<name>/<path>`), `clock` and `input_folder`. RFC 0017 says an installable is sandboxed WebAssembly or data, with "no native executables, no separate processes".
- **A way back:** there is no `solidcraft://` link handler. The only thing a local program can talk to is the control channel, which listens on 127.0.0.1 only, needs a token from a file, is on by default on desktop and can be turned off under Preferences > Assistant.

## 3. Part 1: the quick solution that works today

### 3.1 Registering a local app as a user-defined slicer

Nothing needs to change in SolidCraft for the simplest case. A local app can already be the target of "Send to Slicer", with these limits stated up front: it is labelled a slicer, it gets a mesh (3MF or STL) only, it gets no argument template, and nothing comes back.

1. Install the app, and note the path of its program (on macOS the `.app` bundle).
2. In SolidCraft open **Insert > Files > Send to Slicer** (or Save Mesh, with a slicer chosen). Choose **Custom…**, and pick the app's program. It must exist and be executable. It is added to the top of the list and kept between runs. Hover it and press Delete to remove it.
3. Choose a format: 3MF or STL. In this window tick **Keep in a folder** and **Choose…** the folder the app watches. Leave **Replace file** unticked to get `<design name>-v1.3mf`, `-v2.3mf` and so on, or tick it to keep one file with a stable name.
4. Press Send. SolidCraft writes the file, then starts the app with the file's full path as the **last argument**: `<program> <file path>`. If the app is already running, whether the file reaches the running window depends on the app; SolidCraft only starts the program.
5. Scripts and the control channel can then send without a dialog: `file.send_to_slicer {"slicer": "<program path or name from file.slicers>", "format": "3mf", "copy_to": "<folder>"}`. They may start only programs the user added in step 2. An agent's request asks first and names the program.

What the app receives: one argument, an absolute path to a file that exists and is complete (it is written to a temporary name and renamed). What it does not get: the design name as a separate value, a format other than 3MF or STL, any other argument, or a way to tell SolidCraft the result.

For an upload app, step 3's watched folder is the reliable route, because the app does not have to be started by SolidCraft at all: it sees a new `-vN` file and offers to upload it.

### 3.2 The change this implies

Each limit above has a small fix. They are sections 4 and 5: a "Send to App" kind that is not called a slicer and takes other formats and arguments, and then a plugin-declared target that reuses it.

## 4. Part 1: Send to App in the host, with no plugin

This ships first, is useful by itself, and is the base for everything else. It is one topic and one pull request.

### 4.1 What the user sees

A **Send to App…** command next to Send to Slicer, with the same window shape: a list of targets with their icons (read from the installed program, as slicers are), the last one used preselected, and **Add…** to add a program. A target the user adds has:

- a **name** (shown in the list and the toast),
- a **program** (an existing executable or app bundle, chosen with Browse),
- an **argument template** (section 4.2), default `{file}`,
- the **formats** it takes, from the formats SolidCraft can already write: the SolidCraft design file (`sc`), STEP, 3MF, 3MF with face tags, STL (meshes also take the existing print-face and quality options), and a default one,
- a **folder to keep copies in**, optional, with the existing never-overwrite numbering (`<name>-v1.<ext>`, `-v2.<ext>`), or "Replace file".

This also answers #16089 for slicers: a per-target format, remembered, with STEP allowed when the program takes it.

### 4.2 The argument template

A target's template is a list of arguments, not a string. Each argument may contain these placeholders, replaced inside the argument and never split:

| Placeholder | Becomes |
|---|---|
| `{file}` | the absolute path of the exported file |
| `{name}` | the design's name as a safe file name (letters, digits, `-`, `_`, spaces, at most 60 characters, as the copy name is today) |
| `{dir}` | the absolute path of the folder holding the file |
| `{format}` | the format name, such as `step` |

An unknown placeholder is refused when the target is saved. The default `["{file}"]` is exactly today's behaviour. A program that wants `--import {file}` or `--title={name} {file}` gets it.

### 4.3 The launch rules

These hold for every target, however it was declared.

- **argv only.** The program and the arguments are passed to the operating system as a list. No shell is ever started, and no argument is built by joining strings.
- **The program is a file, found once.** The path is made absolute and canonical at the moment the user approves the target. It must exist, be a regular executable file or an app bundle, and not be a script interpreter or a shell. Windows batch files (`.bat`, `.cmd`) are refused because the command processor re-reads their arguments.
- **Arguments are checked when the template is saved.** An argument that starts with `{name}` is refused, because the substituted name could start with `-`, and a design called `-rf` must not turn into a flag. A value the program takes after an option is written joined to it (`--title={name}`), so the argument starts with the option, not with the placeholder. The rule applies to every target, and the example in section 5.1 follows it. A program that only accepts the name as a separate argument cannot be sent the name. `{file}` and `{dir}` are absolute paths and start with a separator or a drive letter.
- **No environment from outside.** The program gets the user's normal environment plus the variables SolidCraft documents (section 9), and nothing a plugin chose. The working folder is the folder holding the file.
- **Input closed.** The program's standard input is closed. Its output is read only as far as section 8 says (a short tail for the toast) and is never executed or shown as markup.
- **Nothing runs unapproved.** A target the user added is approved by adding it. A plugin's target needs the `launch` approval (section 5). A scripted or agent call can start only an approved target, and an agent's request asks first and names the program and the command.
- **Hostile input never panics.** A missing program, a bad path, a full disk or a refused launch is an error in the reply, as it is for slicers.

### 4.4 Commands

- `file.send_to_app`: `{target, format?, copy_to?, copy_overwrite?, bodies?, wait?}` sends the open design (or the chosen bodies) and replies with the file path, its size, the format, the version number if a copy was kept, and the launch result (section 8).
- `file.apps`: lists the targets (id, name, kind, formats, default format, whether a copy folder is set) and, for each, whether its program was found on this computer.
- Both have input and output schemas from the start. Adding or removing a target is a user action in the window, as Custom… is today; no command adds a program. That keeps "a script can start only what the user chose" true.
- `file.send_to_slicer` and `file.slicers` stay, and stay slicer-only.

### 4.5 Storage

Targets are saved in an unpaged field of their own, in the same place as the custom slicers, not as a Preferences row. Existing custom slicers are not migrated or changed. The window's "Add…" opens the same program picker as Custom…, with the extra fields. A design remembers its copy folder, as it does for slicers.

## 5. Part 1: a plugin-declared target, the smallest change

A plugin gets a "Send to <App>" button without any new ability to run anything, by declaring the target and letting the host do the work.

### 5.1 The manifest

A plugin's manifest may list targets:

```json
"send_targets": [{
  "id": "upload",
  "name": "Sync app",
  "icon": "icons/sync.svg",
  "executable": { "names": ["syncapp", "SyncApp.exe"], "bundle_id": "com.example.syncapp" },
  "args": ["--publish", "{file}", "--title={name}"],
  "formats": ["sc", "step"],
  "default_format": "sc",
  "copy_folder": "default"
}]
```

and asks for the permission that covers it:

```json
"permissions": { "launch": ["upload"] }
```

- **`executable` is how to find the program, not a path to run.** It lists program names, an app bundle id, and (optionally) a desktop entry id or program folder name. The host looks for it on this computer. If it is not found, the button says "Choose SyncApp…" and the user picks it with Browse. The plugin never learns the path.
- **`args`, `formats`, `default_format`** are as in section 4. The same rules apply (4.3).
- **`icon`** is a file in the plugin's package, shown on the button. Or the program's own icon is used.
- **`copy_folder`** may be `"default"` (a folder SolidCraft names under the user's data folder), or absent (the per-user send folder). The user can choose another when approving.

### 5.2 The approval

- `launch` is a new permission in RFC 0002's list, with the target ids it covers. A target not listed under `launch` is ignored.
- The approval screen shows, for each target: the target's name, **the program that was found, as a full path**, **the exact command with sample values** (`/usr/bin/syncapp --publish "/…/My part-v1.sc" --title="My part"`), the formats, and where the file is kept. The user approves each target separately and may approve fewer than the manifest asks for.
- The approval is stored with the plugin's other approvals and is bound to the plugin's `package_hash`, and also to the resolved program path and the argument template. A changed package, a different program path or a different template asks again. A program that has moved or vanished disables the button until the user chooses it again.
- Nothing runs at install. Nothing runs unless the user presses the button (or an agent's request is approved, as for every command that acts).

### 5.3 What the plugin does

The plugin adds a button in one of its own places (a ribbon group or menu slot, RFC 0015) whose action calls a host command, `file.send_to_app` with its own target id. The host checks that the plugin holds `launch` for that id and that the user approved it, then runs sections 4.3 and 8. The plugin cannot pass a program path, an argument, a folder or an environment. The `file` command group, by itself, never covers any command that starts a program.

### 5.4 What this gives the original request

The upload app is installed on the same computer. A plugin whose only job is a menu entry declares one target for it. One click on "Send to <App>" saves the design as a SolidCraft file or STEP into the app's watched folder (or hands it as an argument), starts the app if it is not running, and shows a toast. The app does the upload, with its own sign-in. The plugin has no network and no folder it does not need.

## 6. Part 2: one list of local targets (generalising Send to Slicer)

The full design makes the slicer a kind of target, not a separate feature.

- **A target** has an id, a name, a kind (`slicer` or `app`), an icon, **how it is started**, an argument template, the formats it takes, and an optional copy folder.
- **How it is started** is one of: a **program** with an argument template (section 4), or the **operating system's file association** for the exported file (`xdg-open`, `open`, the Windows shell). The second is for a user who wants "open it with whatever opens this file type", and is available only to targets the user added themselves, never to a plugin's target: an association can be changed by any program, so it is not something to approve once for a plugin.
- **Kinds** differ in defaults: a slicer has the slicer-only options already built (print faces, single-instance, version-specific launchers, ready-wait); an app has none of them unless asked.
- **Commands:** `file.send_to_app` and `file.apps` (section 4.4) become the general commands. `file.send_to_slicer` and `file.slicers` are kept as the slicer-only views, with the same names, parameters and replies, so no script breaks. Both pairs share one pipeline and one set of schemas.
- **Formats:** every target lists the formats it takes. `file.apps` says which. This is #16089: remember the format for each target, and let STEP go to a program that reads it.
- **The sent file's name** follows #16440 and #17078 when they land; this RFC does not decide it, and uses the design's name for copies as today.
- **Where the list shows:** the Send window shows both kinds, slicers first; a plugin's targets appear with the plugin's badge (RFC 0015), so one can't pass as built in.
- **Export rules:** exports use the same code as Save As and Export for each format. A format SolidCraft can't write for this design (a mesh-only body to STEP, say) is refused in the reply with the reason, and the button says why.

## 7. Part 2: plugin targets and the marketplace tiers

RFC 0017's rule is that a tier changes what we say and how much friction a risky grant carries, not what the sandbox allows. `launch` fits that, with two additions.

**An amendment to RFC 0017.** Its section 1 (non-goals) says there are no native executables and no separate processes. We would reword it: *an installable is sandboxed WebAssembly or data. It never starts a process. It can, with the user's approval, name a local program that **the host** starts, as in RFC 0018; the program is the user's own and is not part of the package.* The Home Assistant add-on model stays out: nothing is installed or run as part of a package.

**A tier rule.** In RFC 0017 section 6, `launch` joins the high-risk grants (write access to a folder, the `file` group, settings, other plugins, raised limits, `input_folder`):

| | Verified | Listed | Custom |
|---|---|---|---|
| `launch` approval shows the program, the exact command and the formats | yes | yes | yes |
| Confirmation | one | a second step: the user ticks the `launch` line on its own | same as Listed |
| Headless install | `approval` and `archive` from a dry run, as today | plus `"accept_unreviewed": true` | same |

The listing check in the index build (RFC 0017 section 4.3) would add: a target's argument template parses; no placeholder is unknown; `executable` names no shell or interpreter; and the target's `formats` are known. The review checklist for Verified adds "does the declared program match what the plugin says it is for".

This RFC does not edit RFC 0017's file. Its amendments are applied by the pull request that accepts this RFC, so each topic stays in one pull request.

## 8. Part 2: one click, and what the user sees

1. **Choose the file.** The target's default format, or the one the user picked in the window. By default the file goes in the per-user send folder under a stable name (replaced on resend). If the target has a copy folder, the design is saved there with the next free `-vN` number, never overwriting anything, or one file with a stable name if the user chose Replace. The user's open design file is never touched: Send writes a copy.
2. **Check.** The target is approved and its program still exists and is the approved one. The format is allowed. Otherwise: an error in the reply, a plain message in the window, nothing started. **Show the actual command.** A target set to ask each time (the default for an agent's request and for a target a Listed or Custom plugin declared) shows the real command here, with the real program path, file path and name, and starts only when the user confirms it. A target the user turned the question off for starts at once. In both cases the real command is in the result toast and in the send history, so the user can always see what ran.
3. **Start.** The program is started on a background thread, so the window never waits for it. If the target says it has to be started first and waited for (the slicer case), the existing ready-wait runs.
4. **Report.** The window shows a toast: "Sent My part to Sync app" with the file's name, and **Show in folder**, which reveals the file in the system's file manager. If the program could not start, the toast says so and keeps the file path. If the program exits while SolidCraft is still waiting for it, its **exit status** is shown: "Sync app finished" or "Sync app stopped with code 2", with, when it printed any, the last few lines of its error output as plain text (at most 2 KB, never markup, never a link). SolidCraft waits for exit for at most 30 seconds by default (the target may ask for less) and then lets go: a program that keeps running, as a sync app does, is reported as "started", which is a success.
5. **The reply** of `file.send_to_app` carries `delivery`: `started`, `finished` (with `exit_code`), `error`, or `timeout`, in the shape slicers already use, plus the `reveal` path.
6. **Never blocks, never crashes.** A program that hangs, floods its output or exits at once is handled the same way as a missing one.

## 9. Part 2: an optional round trip, so the app can say "uploaded"

A toast that says "started" is the least that can be said. An upload app can know more: "uploaded: <link>" or "failed: not signed in". We would like to show that, as an option, without a new door into SolidCraft.

### 9.1 Principles

- **Off unless asked.** A target says `"report_back": true`, the user approves it with the target, and nothing else about the send changes.
- **No new inbound listener by default.** SolidCraft does not open a port or a pipe for this.
- **A report is only a message.** It can set a short status line (`ok`, `failed`, `info`), a message of at most 200 characters and one `https` link, for that one send. It cannot run a command, open or change a design, read anything, or change a setting.
- **One use, one send.** For each send, SolidCraft makes a random token (128 bits or more) and a send id, valid for one report and for ten minutes. A second report, a late one or one with another token is ignored.
- **The token reaches the app by environment variable** (`SOLIDCRAFT_REPLY_TOKEN`, `SOLIDCRAFT_SEND_ID`, and the address in option A below), not on the command line, so other users' programs can't read it from the process list.
- **Shown as text.** The message is plain text. The link is shown as its host name and opened only if the user clicks it, after one more click that shows where it goes. A report can never open a link by itself.

### 9.2 The two ways back

**A. The existing local control channel (recommended first).** The app calls one new command, `file.app_report {send_id, token, status, message, url?}`, over the channel that already exists: 127.0.0.1 only, with the access token the channel already needs. The app is given a **scoped, one-use report token, never the channel's own access token**, which would let it run every command. The command accepts only the report token, and does nothing but set the status. So:
- Nothing new listens. If the user turned the control channel off (Preferences > Assistant), reporting back is unavailable and the toast says "started".
- The trade-off: a channel that is on becomes reachable by one more kind of caller, with one scoped, expiring ability. The access token never leaves SolidCraft.

**B. A `solidcraft://` link, opt-in, later.** The app opens `solidcraft://app-report?send=<id>&token=<token>&status=ok&url=<https link>`. For this we register a link handler with the operating system (a desktop entry on Linux, the app's bundle on macOS, the registry on Windows). That is a bigger step, so:
- It is **off until the user turns it on** in the Send to App window, which also says what it does.
- **Any web page or program can open such a link,** so the handler does exactly what A does and nothing more: it checks the send id and the token, sets a status, and drops anything else. It never opens a design, runs a command, installs anything or starts SolidCraft from nothing; if SolidCraft is not running, the link is ignored.
- Forwarding the link to the running window is different on each operating system, and the installers are in packaging's area (`skills/solidcraft-packaging/SKILL.md`). This is why B comes later and is the open question in section 14.

### 9.3 Security in one list

- The report token is random, single-use, ten minutes, bound to one send, and useless for any other command.
- A process that can read the child's environment is the same user's process, which could already read the exported file.
- A hostile page opening a `solidcraft://` link can only try tokens it cannot guess; failures are ignored silently and are rate limited.
- No report text is rendered as markup, as a path or as a command, and the link needs two deliberate clicks.
- With `report_back` off (the default), nothing here is active.

## 10. Part 2: what the local app would look like (not ours to build)

The upload app is separate software that someone writes. This is the shape that works with the above, so a developer can build to it and we can test against it.

- **Watch a folder, or take the file as an argument.** Either the app watches the copy folder SolidCraft writes into (`<design name>-v1.sc`, `-v2.sc`, …, never overwritten), or it is started with `{file}`. Watching is the more robust: it works when the app is closed and later started. Files appear complete (they are written to a temporary name and renamed), so the app may read a file as soon as it sees it.
- **Upload on demand.** The app shows the new version and lets the user press Upload, or uploads at once if the user chose that in the app. SolidCraft never decides.
- **Authentication stays in the app.** SolidCraft never holds the site's password, key or token, never passes one to a plugin and never stores one in a design. A plugin never sees any of it.
- **Conflicts and versions.** The `-vN` number is a local counter, not an identity on the site. The app keeps its own record, keyed by the design (its name, and the `.sc` file's own stable identifier if it has one), and decides whether a new file is a new version of the same item or a new item, and what to do when the site has a newer one. It asks the user rather than overwriting on the site.
- **What it may read of a `.sc` file.** The `.sc` file is a SolidCraft design, and its internals can change between releases. The app should treat it as an opaque file to upload, and take the name from the argument (`{name}`) rather than opening it. Anything more, such as a thumbnail or the design's name and version, should come from a small, documented, read-only part of the format that we add, if needed, as one new file under `crates/doc/src/format/pending/` as every format change is. This RFC does not add it.
- **Reporting.** If it supports section 9, it reads `SOLIDCRAFT_REPLY_TOKEN` and `SOLIDCRAFT_SEND_ID`, and makes one call.
- **Licence.** Separate software is the author's own and may have any licence; SolidCraft's rules about GPL code apply to code in this repository only.

## 11. The web app

The web app does not start programs (as for slicers today). A target's button is replaced with **Download**, which offers the same file in the same format through the browser's download. A plugin's target works the same way: the declaration still shows, the action is the download, and the toast says the file was saved to the user's downloads. `file.send_to_app` called in the browser replies with a typed "needs the desktop app" error that names the download alternative. No `launch` approval is shown in the browser, since nothing could use it.

## 12. The `net` permission stays reserved

None of this needs the network. The upload app has its own, and `net` stays reserved and refused, as in RFC 0002 and RFC 0015. If it is ever granted, it is per plugin and is made by the host (RFC 0015). Sending to a local app is the answer for people who need a site's upload now: it needs no change to what plugins can reach.

## 13. Phases

| Phase | What |
|---|---|
| 0 | This RFC, discussed in its pull request. |
| 1 | Section 4: the Send to App kind, user-added targets with argument templates and per-target formats including `sc` and STEP, `file.send_to_app` and `file.apps` with schemas. Not a slicer, no plugin. One topic, one PR. Per-target format also closes the STEP half of #16089. |
| 2 | Section 6: slicers as a kind in the same list, the file-association start, one set of schemas. |
| 3 | Sections 5 and 7: the `launch` permission, `send_targets` in the manifest, the approval screen, the tier rule, and the amendment to RFC 0017. |
| 4 | Section 8's exit status and reveal, if they are not already in phase 1. |
| 5 | Section 9, option A, then B, each its own PR. |
| 6 | The web download (section 11), with phase 1 if it is small. |

A phase is done when its tests pass and the screenshots (English and German, landscape) are in its pull request. Phases 1 and 3 get a hostile-input test: a missing program, a path that is a directory, a template with an unknown placeholder, a design name that starts with `-`, and a program that never exits.

## 14. Open questions

1. **Option B (`solidcraft://`).** Is registering a link handler worth it, given the installers and the single-window forwarding it needs, or is A enough?
2. **Pinning a program.** Is the canonical path enough, or should the approval also pin the program's size and hash and ask again when it changes (which would ask on every update of the app)?
3. **Which placeholders.** `{file}`, `{name}`, `{dir}` and `{format}` are proposed. Is a design version or a stable design id a placeholder worth adding?
4. **One program, two kinds.** Does a "Send to App" entry that takes the same program as a slicer confuse people? We propose one program may be in the list as both.
5. **Exit status.** Is 30 seconds the right default wait for exit, or should a program only be reported as started?

**Defaults this RFC takes.** The maintainers can override any of these in the pull request.

- Targets live in their own dialog and an unpaged field, never a Preferences row.
- A plugin's target never uses the file association start.
- Reporting back is off unless the target and the user turn it on, and uses the control channel first.
- A copy sent to the app is always a copy, never the user's open file, and is never overwritten except when the user chose Replace.
