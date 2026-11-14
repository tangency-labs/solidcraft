#!/usr/bin/env bash
# Start the web app image and check that it serves the app the way browsers need it.
#
# Usage: packaging/docker/smoke-test.sh [IMAGE]     (default: solidcraft-web:test)
#
# Runs the container read-only (as compose.yaml does) and checks the page, the service worker
# and the wasm: their status, types and caching (nginx.conf).
set -euo pipefail

IMAGE="${1:-solidcraft-web:test}"
NAME="solidcraft-smoke-$$"
RUNTIME="${CONTAINER_RUNTIME:-docker}"
DATA="$(mktemp -d)"
chmod 777 "$DATA"
"$RUNTIME" run -d --rm --name "$NAME" --read-only --tmpfs /tmp --cap-drop ALL -p 127.0.0.1::8080 "$IMAGE" >/dev/null
trap '"$RUNTIME" rm -f "$NAME" >/dev/null 2>&1 || true; rm -rf "$DATA"' EXIT
PORT="$("$RUNTIME" port "$NAME" 8080/tcp | head -n1 | sed 's/.*://')"
BASE="http://127.0.0.1:$PORT"

for _ in $(seq 1 30); do
  curl -fs -o /dev/null "$BASE/" && break
  sleep 1
done

fail() { echo "error: $*" >&2; "$RUNTIME" logs "$NAME" >&2 || true; exit 1; }

# header URL NAME: the value of response header NAME for URL ("" when missing).
header() { curl -fsI "$1" | tr -d '\r' | awk -v h="$(echo "$2" | tr '[:upper:]' '[:lower:]')" -F': ' 'tolower($1) == h { print $2 }'; }

curl -fs "$BASE/" | grep -q 'solidcraft_canvas' || fail "index.html is not the app"
[ "$(header "$BASE/" Cache-Control)" = no-cache ] || fail "index.html must not be cached"
[ "$(header "$BASE/sw.js" Cache-Control)" = no-cache ] || fail "sw.js must not be cached"
curl -fs "$BASE/sw.js" | grep -q 'const VERSION = "dev"' && fail "sw.js is the unbuilt template (not a release build)"
[ "$(header "$BASE/build.json" Cache-Control)" = no-cache ] || fail "build.json must not be cached"

WASM="$(curl -fs "$BASE/" | grep -o 'solidcraft-web-[0-9a-f]\{8,16\}_bg\.wasm' | head -n1)"
[ -n "$WASM" ] || fail "index.html names no wasm"
[ "$(header "$BASE/$WASM" Content-Type)" = application/wasm ] || fail "$WASM is not served as application/wasm"
header "$BASE/$WASM" Cache-Control | grep -q immutable || fail "$WASM should be cached as immutable"
[ "$(header "$BASE/manifest.webmanifest" Content-Type)" = application/manifest+json ] || fail "wrong manifest type"
code="$(curl -s -o /dev/null -w '%{http_code}' "$BASE/no-such-file")"
[ "$code" = 404 ] || fail "a missing file answers $code, not 404"

echo "smoke test passed: $IMAGE serves $WASM"

# With no opt-in the endpoint is absent, including when probed by the web app.
[ "$(curl -s -o /dev/null -w '%{http_code}' -X OPTIONS "$BASE/designs/")" = 404 ] || fail "server folder is enabled by default"
"$RUNTIME" rm -f "$NAME" >/dev/null
"$RUNTIME" run -d --rm --name "$NAME" --read-only --tmpfs /tmp --cap-drop ALL \
  -e SOLIDCRAFT_SERVER_FOLDER=1 -v "$DATA:/designs:Z" -p 127.0.0.1::8080 "$IMAGE" >/dev/null
PORT="$("$RUNTIME" port "$NAME" 8080/tcp | head -n1 | sed 's/.*://')"
BASE="http://127.0.0.1:$PORT"
for _ in $(seq 1 30); do
  curl -fs -o /dev/null -X OPTIONS "$BASE/designs/" && break
  sleep 1
done
[ "$(header "$BASE/designs/" X-SolidCraft-Folder)" = 1 ] || fail "missing server folder marker"
curl -fs -X MKCOL "$BASE/designs/Parts/" >/dev/null || fail "MKCOL failed"
curl -fs -X PUT -H 'If-None-Match: *' --data-binary 'first design' "$BASE/designs/Parts/test.solidcraft" >/dev/null || fail "PUT failed"
curl -fs "$BASE/designs/Parts/" | grep -q 'test.solidcraft' || fail "listing omits design"
[ "$(curl -fs "$BASE/designs/Parts/test.solidcraft")" = 'first design' ] || fail "GET differs from PUT"
TAG="$(header "$BASE/designs/Parts/test.solidcraft" ETag)"
[ -n "$TAG" ] || fail "no ETag"
curl -fs -X PUT -H "If-Match: $TAG" --data-binary 'other design' "$BASE/designs/Parts/test.solidcraft" >/dev/null || fail "conditional PUT failed"
[ "$(curl -s -o /dev/null -w '%{http_code}' -X PUT -H "If-Match: $TAG" --data-binary 'stale design' "$BASE/designs/Parts/test.solidcraft")" = 412 ] || fail "stale save was accepted"
[ "$(curl -fs "$BASE/designs/Parts/test.solidcraft")" = 'other design' ] || fail "conflict overwrote design"
[ "$(curl -s -o /dev/null -w '%{http_code}' -X PUT -H 'If-None-Match: *' --data-binary 'duplicate' "$BASE/designs/Parts/test.solidcraft")" = 412 ] || fail "new save replaced existing design"
# Browsers request compression: GET must still return a strong ETag usable for If-Match.
head -c 4096 /dev/zero | tr '\0' x > "$DATA/.payload"
curl -fs -X PUT --data-binary "@$DATA/.payload" "$BASE/designs/Parts/test.solidcraft" >/dev/null || fail "large PUT failed"
curl -fs --compressed -D "$DATA/.headers" -o "$DATA/.readback" "$BASE/designs/Parts/test.solidcraft" || fail "compressed GET failed"
cmp "$DATA/.payload" "$DATA/.readback" || fail "compressed GET changed the design"
TAG="$(tr -d '\r' < "$DATA/.headers" | awk -F': ' 'tolower($1) == "etag" { print $2 }')"
[ -n "$TAG" ] || fail "compressed GET has no ETag"
curl -fs -X PUT -H "If-Match: $TAG" --data-binary 'final design' "$BASE/designs/Parts/test.solidcraft" >/dev/null || fail "GET ETag cannot be used for saving"
curl -fs -X DELETE "$BASE/designs/Parts/test.solidcraft" >/dev/null || fail "DELETE failed"
[ "$(curl -s -o /dev/null -w '%{http_code}' "$BASE/designs/Parts/test.solidcraft")" = 404 ] || fail "deleted design still exists"
curl -fs -X DELETE "$BASE/designs/Parts/" >/dev/null || fail "folder DELETE failed"
echo "server folder smoke test passed"
