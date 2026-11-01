#!/usr/bin/env bash
# Run from a normal user terminal: needs rootless Podman, not a production server.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="$PWD/.build-tools/cargo"
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p .test-data
chmod 700 .test-data
for version in 10.11 11.4; do
  instance="$PWD/.test-data/mariadb-$version"
  container="harbor-test-${version/./-}"
  mkdir -p "$instance/data" "$instance/run"
  if podman container exists "$container"; then
    printf 'Existing test container %s: use podman start if stopped, or remove it explicitly.\n' "$container"
    podman start "$container" >/dev/null
  else
    podman run -d --name "$container" --userns=keep-id --user "$(id -u):$(id -g)" \
      --network=none \
      -v "$instance/data:/var/lib/mysql:Z" -v "$instance/run:/run/mysqld:Z" \
      -e MARIADB_ALLOW_EMPTY_ROOT_PASSWORD=1 -e MARIADB_ROOT_HOST=localhost \
      "docker.io/library/mariadb:$version" \
      --skip-networking --socket=/run/mysqld/mariadb.sock
  fi
  ready=false
  for attempt in $(seq 1 60); do
    if podman exec "$container" mariadb-admin --no-defaults --socket=/run/mysqld/mariadb.sock -uroot ping >/dev/null 2>&1; then ready=true; break; fi
    sleep 1
  done
  if [[ "$ready" != true ]]; then podman logs "$container"; exit 1; fi
  podman exec -i "$container" mariadb --no-defaults --socket=/run/mysqld/mariadb.sock -uroot <<'SQL'
CREATE DATABASE IF NOT EXISTS harbor_test_guard;
CREATE TABLE IF NOT EXISTS harbor_test_guard.instance(marker VARCHAR(80) NOT NULL);
DELETE FROM harbor_test_guard.instance;
INSERT INTO harbor_test_guard.instance VALUES ('isolated-harbor-integration');
SQL
  podman exec "$container" mariadb --no-defaults --socket=/run/mysqld/mariadb.sock -uroot -N -e 'SELECT VERSION()' > "$instance/server-version.txt"
  HARBOR_TEST_SOCKET="$instance/run/mariadb.sock" cargo test --locked -p harbor-core --test mariadb -- --ignored --nocapture 2>&1 | tee "$instance/test.log"
  podman inspect --format '{{.ImageDigest}}' "$container" > "$instance/image-digest.txt"
done
printf 'Both isolated instances are running without TCP. Stop with podman stop harbor-test-10-11 harbor-test-11-4\n'
