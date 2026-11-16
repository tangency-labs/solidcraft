# solidcraft-control

The control channel any app may share (docs/control-protocol.md). An app that listens for
control makes a random token per launch, writes it to a file only the user can read and refuses
every connection whose first request doesn't carry it; a client finds the token the same way.
Which app the files belong to, its environment, its name in messages and its default port come
from an `App` value the app defines. No app types and no engine.

## API

| Item | What it does |
|---|---|
| `auth::App` | An app's identity: `name`, `data_dir`, `env` (its prefixed variables), `utc`, `default_port` |
| `App::token_path(port)`, `App::current_port_path()` | `<data folder>/control/<port>.token` and the discovery file (`CONTROL_DIR` replaces the folder) |
| `App::write_token_file(path, token)` | Write a token readable only by the user, with its `<port>.pid` file beside it |
| `App::write_current_port`, `App::read_current_port`, `App::remove_current_port` | The discovery file of the most recently started app |
| `App::env_token()`, `App::client_token(port, explicit)` | The token a client sends: explicit, `CONTROL_TOKEN`, else the token file |
| `auth::ClientToken::explain_error(error)` | A refusal with the token file named and whether it is stale; never the token |
| `auth::plan`, `auth::Listen`, `auth::bind_with_fallback`, `auth::PORT_TRIES` | Where a launch listens; a loopback listener on the first free port |
| `auth::tokens_match`, `auth::remove_token_file`, `auth::TokenFile`, `auth::pid_path`, `auth::MIN_TOKEN_LEN` | Constant-time compare and token file housekeeping |
| `Effect` | What a request still running does when it finishes: reads only, writes a file, or changes; `when_done(document)` says it |
| `server::Hub`, `server::Protocol`, `server::Summary`, `server::Waits`, `server::new_token` | The loopback JSON-lines server behind the token (desktop only); the app says what its requests mean |
| `client::Connection`, `client::Request`, `client::CallError`, `client::Waits` | The client (desktop only): the app's token on every request, one bounded round trip, a fresh connection after a lost one, the wait for a request still running (`control.pending`); "not running" told from "lost during the call" |
| `ControlRequest`, `ControlResponse`, `DocumentWatch`, `DOCUMENT_WATCH` | A request as the window gets it; the document id and revision subscribed connections wait on |
