# Problem reports

Report a bug (the title bar's button), Help › Report an Issue and a dialog's Create Issue open
**Report a problem**: an in-app form that files a private ticket on
[tickets.solidcraft.io](https://tickets.solidcraft.io). Only the reporter, triagers and admins can
see a ticket.

The form has a title, a description, the app the ticket is for (SolidCraft unless another is
picked) and its type (bug, feature or question). Each attachment is a checkbox that says what it
holds, and the dialog shows exactly what goes:

| Attachment | Default | What it holds |
| --- | --- | --- |
| System info | on | App version and build, OS, renderer, graphics card and driver, window size. Nothing about the design. |
| What you were doing | on | The last 30 commands that changed the design or failed, with their parameters and results, and the failing command with its error. |
| Log | on | The last 300 lines of the design's command log and the Messages, with the home folder shown as `~`. |
| Screenshot of the 3D view | on | The view as it was when the report opened (Retake updates it). |
| Your design file (.sc) | **off** | The open design as it is now, including unsaved changes. Never sent unless ticked. |
| Crash report | off | Offered for a report made from a crash report. |

The ticket carries a `consent` record listing exactly which boxes were ticked.

## Network use

SolidCraft talks to the ticket server only when the user sends a report or opens My tickets, or to
send a report already waiting in the queue. There is no background telemetry.

- **Sign-in** uses a device code: the first report opens the server's sign-in page in the browser
  and shows the code in the dialog. The token is kept in the system keyring (Windows Credential
  Manager, macOS Keychain, Secret Service on Linux and in the Flatpak, service "SolidCraft
  tickets"); it is never written to a file. Preferences › Security › Problem reports signs out.
- **Sending** files the ticket, then uploads each ticked attachment in resumable chunks, with a
  progress bar and Cancel.
- **Offline**: a report that cannot reach the server waits in `<data folder>/report-queue` (see
  `solidcraft_engine::recovery::data_dir`) and is sent when SolidCraft starts or every five
  minutes while it runs, once signed in. The queue holds the report and its files, never the
  token.

## The old behaviour

Preferences › Security › "Report a bug opens a GitHub draft instead" (`report_on_github`) brings
back the public GitHub issue draft the button opened before.

## Server API

Every path the app uses is in `crates/ui-egui/src/tickets/api.rs`: device sign-in
(`/api/device/code`, `/api/device/token`, a `tickets:report` token), `/api/apps`,
`POST /api/tickets` (with `system`, `repro` and `consent`), and resumable uploads (`POST
/api/uploads` with the file's size and SHA-256, `PUT /api/uploads/:id/parts/:n` in the parts the
server asks for, `GET /api/uploads/:id` to resume, `POST /api/uploads/:id/complete`).
`SOLIDCRAFT_TICKETS_URL` points a desktop build at a test server (`https://`, or
`http://localhost`).
