# solidcraft-control-egui

The window's own control requests (docs/control-protocol.md), which any egui app answers the same
way: `ui.screenshot` and `ui.resize`. The token files, the loopback server and the request types
are `solidcraft-control`, which has no UI toolkit, so a headless engine, CLI or MCP bridge uses the
control channel without compiling egui. No app types and no engine.

## API

| Item | What it does |
|---|---|
| `screenshot::Shots` | `ui.screenshot`: frames asked for, taken when the view settles, saved and answered, or refused when hidden, cancelled or late; the app gives the reply for no frame |
| `screenshot::encode` | A frame in the format its path names |
| `window::resize`, `window::hidden` | `ui.resize` over `egui::Context`; whether the window presents frames |
