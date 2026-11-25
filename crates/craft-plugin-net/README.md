# craft-plugin-net

App-independent connections for [`craft-plugin`](../craft-plugin/README.md) plugins that ask for
the `net@0.1` capability. The app owns the user's approval; this crate owns the sockets. It has no
SolidCraft or CAD dependency.

## Public API

- `Connections::new(endpoints, options)`: the connections of one plugin call, limited to
  `endpoints` (the manifest's `permissions.connect` the user allowed). Dropping it closes them all.
- `Connections::open(host, port, request, wait) -> Result<u32>`: checks the endpoint against
  `endpoints` and `Options::allowed_hosts`, then connects and returns a handle. `Request` is
  `Tcp`, `Tls` or `Ssh { user, command }`.
- `read(id, max, wait)`, `write(id, data, wait)`, `finish(id, wait)`, `close(id)`: the bytes of
  one connection. A read returns as soon as some bytes arrive, empty at the end; `finish` ends this
  side's writing (TCP half-close, TLS close-notify, an SSH command's end of input).
- `Options { known_hosts, ssh_keys, ssh_agent, tls_roots, allowed_hosts }`, with
  `Options::user()` for the user's own known hosts, keys, agent and the system's trusted roots.
  Tests point them at their own files.
- `Wait`: called every 200 ms while a connection waits for the network; an error (the call was
  cancelled or ran out of time) stops the wait and is returned.
- `MAX_CONNECTIONS` (16 at once) and `MAX_CHUNK_BYTES` (1 MiB per read or write).

Errors are `craft_plugin::HostError`: `PermissionDenied` for an endpoint not allowed,
`InvalidInput` for a bad host, port or handle, `LimitExceeded`, and `CommandFailed` for a
connection that failed, including an SSH command that exited with an error (its message is what
the command printed).

## Transports

- **TCP**: plain bytes.
- **TLS**: rustls with the ring provider; the server's certificate is checked against the
  system's trusted roots with `rustls-platform-verifier`.
- **SSH**: russh with ring. The server's key is checked against the user's known hosts (an unknown
  or changed key is refused with a message saying so); sign-in uses the SSH agent
  (`SSH_AUTH_SOCK`, Unix), then `~/.ssh/id_ed25519` and `~/.ssh/id_ecdsa` without a passphrase.
  One command runs once the server accepts it; the connection carries its standard input and
  output. A command the server refuses, or one a signal stopped, is an error.

A TLS connection's data ends with TLS's goodbye (`close_notify`); a connection dropped without it
is an error, since the reply may be cut short. Name lookup and each connection attempt run on a
thread of their own, so `Wait` is asked throughout and Cancel works while a server doesn't answer.

On `wasm32` there are no sockets. `open` still checks the request first (a bad host is
`InvalidInput`, an endpoint the manifest or the allowlist doesn't name is `PermissionDenied`);
a valid, permitted request returns `HostError::Unsupported`.

## Validation

`cargo test -p craft-plugin-net` runs a local TCP, TLS (a test certificate authority) and SSH
server (test keys made at run time) on 127.0.0.1; nothing reaches another machine.
`cargo xtask shared` checks the crate builds and tests on its own.
