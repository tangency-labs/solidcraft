#!/bin/sh
# The image remains read-only; only nginx's optional include lives in /tmp.
set -eu
rm -f /tmp/solidcraft-folder/location.conf
if [ "${SOLIDCRAFT_SERVER_FOLDER:-0}" = 1 ]; then
    test -d /designs || { echo 'SOLIDCRAFT_SERVER_FOLDER=1 requires a writable /designs mount' >&2; exit 1; }
    test -w /designs || { echo '/designs must be writable by the container user (uid 101)' >&2; exit 1; }
    mkdir -p /tmp/solidcraft-folder
    cat > /tmp/solidcraft-folder/location.conf <<'NGINX'
location = /designs { return 308 /designs/; }
location ^~ /designs/ {
    # Preserve strong content ETags for conditional writes.
    gzip off;
    client_max_body_size 256m;
    proxy_request_buffering on;
    proxy_buffering off;
    proxy_pass http://127.0.0.1:8081;
    proxy_http_version 1.0;
    proxy_set_header Connection close;
    proxy_read_timeout 60s;
    add_header X-Content-Type-Options nosniff always;
}
NGINX
    /usr/local/bin/solidcraft-web-folder &
fi
