# syntax=docker/dockerfile:1
# SolidCraft in the browser as a container: the static site (the one packaging/web/package.sh
# zips) served by nginx on port 8080, as an unprivileged user. From the repository root:
#
#   docker build -f packaging/docker/Dockerfile -t solidcraft-web .
#   docker run --rm -p 8080:8080 solidcraft-web          then open http://localhost:8080/
#
# Or `docker compose up` with packaging/docker/compose.yaml. Saving designs needs HTTPS or
# localhost (the browser's private file storage), so put a TLS reverse proxy in front of it on a
# server; the site works under any URL path.
#
# The site is wasm, the same on every platform, so it is built once on the build machine's own
# platform even for a multi-platform image. CI builds it outside Docker (with its Rust cache and
# git commit for Help › About) and passes it in as the `site` stage: --build-context site=dist/web.

ARG RUST_VERSION=1.99.1
ARG TRUNK_VERSION=0.21.14

FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-slim-bookworm AS build
ARG TRUNK_VERSION
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown \
 && curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-$(uname -m)-unknown-linux-gnu.tar.gz" \
    | tar -xz -C /usr/local/bin trunk
WORKDIR /src
COPY . .
WORKDIR /src/apps/solidcraft-web
# Trunk downloads the wasm-bindgen and wasm-opt its build needs; its post-build hook writes sw.js.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    trunk build --release

FROM scratch AS site
COPY --from=build /src/dist/web/ /

# The server edition (docs/server-edition.md): `solidcraft-cli serve` with the web app baked in,
# on port 8787, designs in /data (a volume, or a bind mount of a folder or git checkout). Built with `--target server`; the default target is
# still the static image below.
#
#   docker build -f packaging/docker/Dockerfile --target server -t solidcraft-server .
#   docker run --rm -p 127.0.0.1:8787:8787 -e SOLIDCRAFT_SERVE_TOKEN="$(openssl rand -hex 32)" \
#     -v solidcraft-data:/data --read-only --tmpfs /tmp --cap-drop ALL solidcraft-server
FROM rust:${RUST_VERSION}-slim-bookworm AS cli-build
# Parallel jobs for the build (empty: cargo's default, one per CPU).
ARG CARGO_BUILD_JOBS=
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target-docker \
    CARGO_TARGET_DIR=/src/target-docker cargo build --release --locked ${CARGO_BUILD_JOBS:+-j $CARGO_BUILD_JOBS} -p solidcraft-cli --bin solidcraft-cli \
 && install -D -m 0755 /src/target-docker/release/solidcraft-cli /out/solidcraft-cli

FROM debian:bookworm-slim AS server
RUN useradd --system --uid 10001 --home-dir /nonexistent --shell /usr/sbin/nologin solidcraft \
 && install -d -o 10001 -g 10001 -m 0700 /data
COPY --from=cli-build /out/solidcraft-cli /usr/local/bin/solidcraft-cli
COPY LICENSE-MIT LICENSE-APACHE NOTICE THIRD-PARTY-LICENSES.txt /usr/share/doc/solidcraft/
COPY --from=site / /srv/solidcraft/
# Only /data and /tmp (a tmpfs in compose.yaml) are written; HOME there keeps the app's own
# folders (recovery, preferences) off the read-only image.
ENV HOME=/tmp SOLIDCRAFT_GPU=cpu
USER 10001:10001
VOLUME /data
EXPOSE 8787
# No curl or wget in the image: bash's /dev/tcp asks the health check.
HEALTHCHECK --interval=30s --timeout=5s CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/8787 && printf 'GET /api/v1/health HTTP/1.1\\r\\nHost: localhost\\r\\n\\r\\n' >&3 && head -n1 <&3 | grep -q ' 200 '"]
ENTRYPOINT ["solidcraft-cli", "serve"]
CMD ["--dir", "/data", "--listen", "0.0.0.0:8787", "--web", "/srv/solidcraft"]

FROM nginxinc/nginx-unprivileged:1.29-alpine
COPY packaging/docker/nginx.conf /etc/nginx/conf.d/default.conf
COPY LICENSE-MIT LICENSE-APACHE NOTICE THIRD-PARTY-LICENSES.txt /usr/share/doc/solidcraft/
COPY --from=site / /srv/solidcraft/
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD wget -q --spider http://127.0.0.1:8080/ || exit 1
