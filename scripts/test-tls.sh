#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="$PWD/.build-tools/cargo"
export PATH="$HOME/.cargo/bin:$PATH"
directory="$PWD/.test-data/tls"
mkdir -p "$directory"
chmod 700 "$directory"
if [[ ! -f "$directory/server.pem" ]]; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 7 -subj /CN=Harbor-Test-CA -keyout "$directory/ca.key" -out "$directory/ca.pem" 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -subj /CN=localhost -keyout "$directory/server.key" -out "$directory/server.csr" 2>/dev/null
  printf 'subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n' > "$directory/server.ext"
  openssl x509 -req -days 7 -in "$directory/server.csr" -CA "$directory/ca.pem" -CAkey "$directory/ca.key" -CAcreateserial -extfile "$directory/server.ext" -out "$directory/server.pem" 2>/dev/null
  openssl req -x509 -newkey rsa:2048 -nodes -days 7 -subj /CN=Wrong-Harbor-CA -keyout "$directory/wrong-ca.key" -out "$directory/wrong-ca.pem" 2>/dev/null
  chmod 600 "$directory/"*.key
fi
port=33316
for version in 10.11 11.4; do
  container="harbor-tls-${version/./-}"
  instance="$PWD/.test-data/tls-$version"
  mkdir -p "$instance/data" "$instance/run"
  if podman container exists "$container"; then
    mounted_data=$(podman inspect --format '{{range .Mounts}}{{if eq .Destination "/var/lib/mysql"}}{{.Source}}{{end}}{{end}}' "$container")
    if [[ "$mounted_data" != "$instance/data" ]]; then
      # Replace only this script's obsolete isolated test container, never another service.
      podman rm -f "$container"
    fi
  fi
  if ! podman container exists "$container"; then
    podman run -d --name "$container" --userns=keep-id --user "$(id -u):$(id -g)" \
      -p "127.0.0.1:$port:3306" -v "$directory:/run/harbor-tls:ro,z" \
      -v "$instance/data:/var/lib/mysql:Z" -v "$instance/run:/run/mysqld:Z" \
      -e MARIADB_ALLOW_EMPTY_ROOT_PASSWORD=1 -e MARIADB_ROOT_HOST=% \
      "docker.io/library/mariadb:$version" \
      --socket=/run/mysqld/mariadb.sock --ssl-ca=/run/harbor-tls/ca.pem --ssl-cert=/run/harbor-tls/server.pem --ssl-key=/run/harbor-tls/server.key
  else podman start "$container" >/dev/null
  fi
  ready=false
  for attempt in $(seq 1 60); do
    # The entrypoint starts a temporary socket-only server first. Wait for final TCP, not that temporary server.
    if podman exec "$container" mariadb-admin --no-defaults --protocol=tcp --host=127.0.0.1 --port=3306 -uroot ping >/dev/null 2>&1; then ready=true; break; fi
    sleep 1
  done
  if [[ "$ready" != true ]]; then podman logs "$container"; exit 1; fi
  HARBOR_TEST_CA="$directory/ca.pem" HARBOR_TEST_TLS_PORT="$port" cargo test --locked -p harbor-core --test tls -- --ignored --nocapture 2>&1 | tee "$directory/test-$version.log"
  port=$((port+1))
done
printf 'TLS test containers are bound only to 127.0.0.1. Stop: podman stop harbor-tls-10-11 harbor-tls-11-4\n'
