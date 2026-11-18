#!/usr/bin/env bash
# Start the server edition image and check it the way compose.yaml runs it: read-only, no
# capabilities, the data folder a volume. Checks the browser app and its caching, the health
# check, that the API and MCP refuse a request without the token, and that a design saved with
# it is listed and reads back.
#
# Usage: packaging/docker/smoke-test-server.sh [IMAGE]     (default: solidcraft-server:test)
set -euo pipefail

IMAGE="${1:-solidcraft-server:test}"
NAME="solidcraft-server-smoke-$$"
VOLUME="solidcraft-server-smoke-$$"
TOKEN="$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"
docker volume create "$VOLUME" >/dev/null
docker run -d --rm --name "$NAME" --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges:true \
  -e SOLIDCRAFT_SERVE_TOKEN="$TOKEN" -v "$VOLUME:/data" -p 127.0.0.1::8787 "$IMAGE" >/dev/null
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true; docker volume rm "$VOLUME" >/dev/null 2>&1 || true' EXIT
PORT="$(docker port "$NAME" 8787/tcp | head -n1 | sed 's/.*://')"
BASE="http://127.0.0.1:$PORT"

for _ in $(seq 1 30); do
  curl -fs -o /dev/null "$BASE/api/v1/health" && break
  sleep 1
done

fail() { echo "error: $*" >&2; docker logs "$NAME" >&2 || true; exit 1; }
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
header() { curl -fsI "$1" | tr -d '\r' | awk -v h="$(echo "$2" | tr '[:upper:]' '[:lower:]')" -F': ' 'tolower($1) == h { print $2 }'; }
AUTH="Authorization: Bearer $TOKEN"

[ "$(curl -fs "$BASE/api/v1/health")" = '{"ok":true}' ] || fail "health does not answer"
curl -fs "$BASE/" | grep -q 'solidcraft_canvas' || fail "index.html is not the app"
[ "$(header "$BASE/" Cache-Control)" = no-cache ] || fail "index.html must not be cached"
WASM="$(curl -fs "$BASE/" | grep -o 'solidcraft-web-[0-9a-f]\{8,16\}_bg\.wasm' | head -n1)"
[ -n "$WASM" ] || fail "index.html names no wasm"
[ "$(header "$BASE/$WASM" Content-Type)" = application/wasm ] || fail "$WASM is not served as application/wasm"

[ "$(code "$BASE/api/v1/files")" = 401 ] || fail "the file list answers without the token"
[ "$(code -X POST -H 'Content-Type: application/json' -d '{}' "$BASE/mcp")" = 401 ] || fail "MCP answers without the token"
[ "$(code -H "$AUTH" -X PUT --data-binary 'smoke' "$BASE/api/v1/files/smoke.sc")" = 200 ] || fail "a save with the token failed"
curl -fs -H "$AUTH" "$BASE/api/v1/files" | grep -q '"path":"smoke.sc"' || fail "the saved design is not listed"
[ "$(curl -fs -H "$AUTH" "$BASE/api/v1/files/smoke.sc")" = smoke ] || fail "the saved design does not read back"
[ "$(code -H "$AUTH" "$BASE/api/v1/files/..%2F..%2Fetc%2Fpasswd")" != 200 ] || fail "a path out of /data was served"

INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"1"}}}'
curl -fs -H "$AUTH" -H 'Content-Type: application/json' -d "$INIT" "$BASE/mcp" | grep -q '"serverInfo"' || fail "MCP initialize failed"
echo "server image OK: $IMAGE"
