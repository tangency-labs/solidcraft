# Privacy

SolidCraft keeps your designs on your computer. This page lists everything the app sends over
the network by itself, and how to turn each one off.

| What | When | Off switch |
|---|---|---|
| [Update check](book/src/guide/updates.md) | At start and once a day | Preferences › General › Updates, `SOLIDCRAFT_UPDATE_CHECK=0`, policy `CheckForUpdates` |
| Anonymous usage statistics (below) | At most once a day | Preferences › Privacy, `DO_NOT_TRACK=1`, `SOLIDCRAFT_NO_TELEMETRY=1`, policy `UsageStatistics` |

Everything else that reaches the network does so only when you ask: sending a
[problem report](problem-reports.md), the Assistant with an online model, opening a design link.

## Anonymous usage statistics

SolidCraft sends anonymous usage statistics so we know how many installations there are, how many
are used each day, for how long, and which tools and operations people use. They are on unless
you, your environment or your organization turns them off. The first start shows a notice,
**SolidCraft shares anonymous usage statistics.**, with **Turn off** and **Details**. Nothing is
counted before that notice has been shown.

### What is sent

One report per day of use, with exactly these fields and nothing else:

```json
{
  "v": 1,
  "install_id": "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed",
  "day": "2026-11-20",
  "version": "0.46.0",
  "os": "linux",
  "arch": "x86_64",
  "platform": "desktop",
  "active_minutes": 37,
  "operations": { "ui": { "solid.extrude": 3, "sketch.line": 12 }, "agent": { "solid.extrude": 9 } },
  "tools": { "ui": { "solid.extrude": 4, "sketch.create": 2 } },
  "failed": { "ui": { "solid.fillet": { "kernel.blend.7": 1 } } },
  "compute_ms": { "solid.extrude": { "0-100ms": 9, "100ms-1s": 3 } },
  "dialog_s": { "solid.extrude": { "applied": { "5-30s": 3 }, "cancelled": { "0-5s": 1 } } }
}
```

- `install_id`: a random number made the first time, not derived from your computer, account
  or anything else, and not part of the settings you export. **Reset ID** makes a new one and
  deletes what was kept under the old one.
- `day`: the calendar day (UTC) the report counts.
- `version`, `os`, `arch`, `platform`: the SolidCraft version, `windows`, `macos`, `linux`,
  `freebsd` or `web`, the processor architecture, and `desktop` or `web`.
- `active_minutes`: the minutes in which you used the keyboard, mouse or touch in the SolidCraft
  window. Time the window sits open unused does not count.
- `operations`: how many times a command changed your design (an action you can undo, an edit
  of an existing feature included), by the command's id (the ids in
  [the feature list](features.md)). Rebuilding a design when it opens, features recomputing after
  a change, undo and redo, and previews are never counted.
- `tools`: how many times you started a tool or dialog from the app, even if you cancelled it.
- `failed`: how many times a command failed, by the kind of error only: its category (such as
  `bad_params` or `refused`) or, for a geometry kernel failure, the kernel's call and numbered
  kind (`kernel.blend.7`). Never the error's message, the parameters or anything from the design.
- `compute_ms`: how long operations and failures took, counted in four buckets (`0-100ms`,
  `100ms-1s`, `1s-10s`, `10s+`).
- `dialog_s`: how long a tool or dialog stayed open until you applied or cancelled it, counted in
  four buckets (`0-5s`, `5-30s`, `30s-2m`, `2m+`).
- These are split by who acted: `ui` (you, in the app), `agent` (an assistant over MCP or the
  control channel) and `script` (a plugin; every plugin command counts as `plugin`). Selection,
  showing and hiding, views and the camera are never counted.

Never sent: command parameters, design or file names, paths, geometry, sketches, text you type,
plugin data, your name, account or email, hardware ids, locale or settings. We store no IP
addresses (see On the server). **Preferences › Privacy › Show what's sent** shows the exact JSON
of the next report.

### When and where

- Only release builds send reports; a build you compile yourself never does.
- A finished day is sent once, the next time SolidCraft runs (a minute after it starts), in the
  background: the window never waits on it.
- Reports go only to `https://metrics.solidcraft.io/v1/report`, over HTTPS, without cookies. They
  pass through Cloudflare's network on the way to our server. That service is separate from the
  ticket server and keeps its own database.
- Offline, a report waits and is tried once per start and once a day; a report not sent within
  7 days is deleted.
- Turned off (any switch below), nothing is counted or kept, and reports still waiting are
  deleted.

### On the server

The service stores reports without IP addresses or anything else about the sender. It keeps
reports for 90 days, and after that only daily totals (installations, active installations,
minutes, per-command counts, failures and time buckets). Only the SolidCraft maintainers see them.

We store no IP addresses. To limit floods of fake reports, the server keeps, in memory only, a
keyed hash of the sender's address that changes every day, and forgets it at the end of the day.
Like any web request, a report passes through Cloudflare's network, and our hosting provider,
Railway, keeps request logs that include the sender's IP address for up to 30 days; we don't use
them.

### Deleting your data

**Preferences › Privacy** shows your installation ID. **Delete my data** asks the server to delete
every report it holds for that ID, and the app uses a new random ID from then on. The request
carries only the ID; it is sent in the background, works with statistics turned off, and is tried
again (once an hour, and at the next start) until the server answers. The server deletes that
installation's reports and its first-seen date, and answers the same way whether or not it had
any, so nobody can use it to find out whether an ID exists.

The daily totals are kept: they are counts (how many installations, minutes, runs of each
command) with no installation ID in them, so they can't be traced back to you. **Reset ID** alone
starts a new ID without asking the server to delete anything.

### Turning it off

- **Preferences › Privacy › Share anonymous usage statistics**, or **Turn off** on the notice.
- The environment: `DO_NOT_TRACK=1` or `SOLIDCRAFT_NO_TELEMETRY=1` (any value but empty, `0` or
  `false`). Preferences then says it is turned off by the environment, and no notice is shown.
- Your organization's policy `UsageStatistics` = `0`
  ([Managing SolidCraft in an organization](book/src/guide/managed-deployment.md)): the Windows
  registry or Group Policy/Intune template, a macOS configuration profile, or
  `/etc/solidcraft/policy.json` on Linux and FreeBSD. Preferences shows **Set by your
  organization**, and no notice is shown.

## Builds without usage statistics

Usage statistics are the `usage-stats` cargo feature of the `solidcraft` and `solidcraft-web`
packages, on in their default features. A build without it leaves all of it out: no counting, no
notice, no network code for it and no endpoint in the binary, and Preferences › Privacy says
**Usage statistics: not included in this build**. Distribution packagers, universities and
anyone else who builds from source can build it so:

```sh
cargo build --release -p solidcraft --no-default-features --features raytrace-cpu,bundled-render-library,profile
```

(the default features without `usage-stats`). CI builds the app without the feature on every
change and checks that its binary holds no trace of the endpoint.
