# GitHub client reuse for cloud saves

Dependency verification date: 2026-10-22.

## Recommendation

Use **Octocrab 0.54.5**, with default features disabled, for GitHub request
types, Git database endpoints, response models, and GitHub errors. On the web,
provide a small browser transport through Octocrab's existing public
`with_service` and `with_executor` interfaces. Keep this transport in the web/UI
platform layer. Keep stored credentials and publication/conflict policy outside
the client. An optional `github` feature can enable the API client without adding
it or a Tokio runtime to ordinary native Git saves.

A temporary probe compiled the actual Git read/write builders for
`wasm32-unknown-unknown`, including `.update_ref(...).force(false)`.
[Octocrab package metadata](https://crates.io/api/v1/crates/octocrab/0.54.5),
[public builder and executor interfaces](https://github.com/XAMPPRocky/octocrab/blob/b15540cf8c644f90b6d89971da5e6c54d634cd14/src/lib.rs),
[typed Git database API](https://github.com/XAMPPRocky/octocrab/blob/b15540cf8c644f90b6d89971da5e6c54d634cd14/src/api/git.rs).

Native checkout commit/push operations should continue to use the external Git
CLI, preserving SSH, credential helpers, arbitrary remotes and the local index. GitLab/Gitea account adapters can later share the
browser transport and publication abstraction, while using their own clients.

## Why the Octocrab adapter is needed

The new `reqwest` connector exists in current releases, including 0.54.2. The
0.54.5 WASM probe failed inside that connector with three concrete errors:

- Browser `reqwest::Body` has no `Body::wrap` constructor.
- Browser `reqwest::Response` cannot convert into `http::Response<reqwest::Body>`.
- Browser `reqwest::Body` does not implement the `http_body::Body` interface used
  by that connector.

Separately, Octocrab's service requires a `Send` service future, while Fetch
futures contain JavaScript values and cannot satisfy that bound directly.
Neither the crate's WASM feature references nor successful compilation of an
unused library alone proves that a network client works.
[Released connector](https://github.com/XAMPPRocky/octocrab/blob/b15540cf8c644f90b6d89971da5e6c54d634cd14/src/service/middleware/reqwest_connector.rs),
[service bounds](https://github.com/XAMPPRocky/octocrab/blob/b15540cf8c644f90b6d89971da5e6c54d634cd14/src/lib.rs).

The successful adapter uses existing components:

1. A stateless `tower::service_fn` starts each actual Fetch future with
   `wasm_bindgen_futures::spawn_local`.
2. A `futures::channel::oneshot` returns an owned `http::Response<Full<Bytes>>`.
   Only owned Rust data crosses this channel; JavaScript futures remain local.
   This satisfies Octocrab's `Send` bounds without `unsafe` or `send_wrapper`.
3. `with_executor(Box::new(wasm_bindgen_futures::spawn_local))` drives the Tower
   `Buffer::pair` worker. The normal constructor would start a Tokio worker;
   this public executor hook avoids requiring a Tokio runtime in the browser.
4. Existing `BaseUriLayer`, `ExtraHeadersLayer`, and `AuthHeaderLayer` configure
   the API host, accept headers, and a validated sensitive bearer header.
   Authentication is scoped to the allowed API authority rather than attached
   indiscriminately to external URLs. Octocrab already supplies the API version
   header: do not append another through `ExtraHeadersLayer`. A browser probe
   initially found GitHub rejects the combined value
   `2022-11-28, 2022-11-28` with HTTP 400.

For PAT-only web requests, the verified dependency configuration is:

```toml
octocrab = { version = "0.54.5", default-features = false }
reqwest = { version = "0.13", default-features = false, features = ["stream"] }
tower = { version = "0.5", default-features = false, features = ["util"] }
http = "1"
http-body-util = "0.1"
bytes = "1"
futures = "0.3"
wasm-bindgen-futures = "0.4"
```

Keep this configuration target/feature scoped. Do not enable Octocrab's stock
`reqwest` connector when using this adapter. Do not enable JWT, Rustls/OpenSSL,
retry timers, or native default-client features merely to use a PAT. In 0.54.5
JWT is optional; older 0.54.2 still had an unconditional `jsonwebtoken`
dependency. The latest default JWT backend is AWS-LC, whereas `jwt-rust-crypto`
adds another dependency graph; neither is needed here. Octorust and Octocrate
core still include JWT dependencies even for this PAT use case.
[Published feature/dependency manifest](https://docs.rs/crate/octocrab/0.54.5/source/Cargo.toml),
[Octocrate core manifest](https://docs.rs/crate/octocrate-core/0.1.9/source/Cargo.toml),
[Octorust manifest](https://docs.rs/crate/octorust/0.10.0/source/Cargo.toml).

### Bounded responses and behavior still owned by SolidCraft

The final probe enables Reqwest's browser `bytes_stream`, checks the accumulated
length before appending each chunk, and errors above a 32 MiB transport limit.
It does **not** call `bytes().await` on the entire response before checking its
length. The limit is a probe setting: production should derive appropriate
limits from cloud chunk/manifest limits, allowing for Base64 and JSON overhead.
Decoded lengths, hashes, decompression limits, branch validation, and tree
truncation handling still belong to SolidCraft.

A custom `new_empty` builder does not automatically recreate the normal retry,
timeout, or cache stack. Preserve the application's bounded retry/backoff,
cancellation/deadline, and explicit-save semantics; do not add unconditional
retries around commit/ref publication. The adapter should also avoid retaining
large response bodies or credentials in logs. These are integration policies,
not reasons to maintain handwritten GitHub endpoint implementations.

## Endpoint and execution proof

The isolated probe type-checks these actual Octocrab methods:

- `git(owner, repo).get_ref(&Reference::Branch(...))`
- `get_commit`, `get_tree(...).recursive(true).send()`, and `get_blob`
- `create_blob(...).encoding("base64").send()`
- `create_tree(Vec<CreateTreeEntry>).base_tree(...).send()`
- `create_commit(message, tree).parents(...).send()`
- `update_ref(&reference, commit).force(false).send()`

No write builder was executed against GitHub. Temporary probe files and build
logs are retained at:

```text
/tmp/user/1000/solidcraft-github-crates-cw8xdiae/
  octocrab-browser-adapter-probe/Cargo.toml
  octocrab-browser-adapter-probe/src/lib.rs
  octocrab-adapter-wasm.log
  octocrab-adapter-wasm-build.log
  octocrab-wasm.log
  octocrate-wasm.log
  octorust-wasm.log
  browser_harness.py
  browser-result.json
```

The successful check/build use `--target wasm32-unknown-unknown`, an isolated
Cargo workspace, and a target directory under the same temporary folder. No
workspace Cargo manifest or lockfile was changed. The alternative probes were
rerun with the installed `/usr/lib/llvm-17/bin/clang` and JS randomness enabled,
so their reported failures are client incompatibilities rather than a missing
C compiler.

An **actual headless Chromium browser run passed** using the bounded adapter,
Reqwest 0.13.5, and wasm-bindgen 0.2.129. The probe loaded a WASM module from an
isolated temporary localhost page and performed real, unauthenticated public
GitHub requests through Octocrab for the reference, Git commit, recursive tree,
and README blob of `XAMPPRocky/octocrab`. Its result was:

```text
PASS: Octocrab 0.54.5 browser Git ref, commit, tree, blob; 14053 bytes; truncated=false
```

This proves the supported custom executor runs the browser service without a
Tokio runtime, including real GitHub CORS and typed response decoding. It does
not claim authenticated write publication, retry/cancellation behavior, or the
full cloud save flow was exercised; those remain implementation acceptance
tests. The browser used a new temporary profile and no account credentials.

## Integrated adapter checks

The optional `solidcraft-cloud/github` adapter now uses these typed operations.
Its focused suite passes 19 tests, including simulated ref races, conflict
copies, idempotent publication, asset reuse, corrupt existing assets, and refusal
to write future formats or managed symlinks. Existing assets are compared with
their Git blob identities locally, avoiding a download merely to check reuse.

The app's browser transport compiles for `wasm32-unknown-unknown` with bounded
streamed responses and a Reqwest timeout. The standalone browser probe above
remains the evidence for real public GitHub requests; authenticated publication
still needs an end-to-end acceptance test with a dedicated repository.
