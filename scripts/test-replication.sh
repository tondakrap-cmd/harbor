#!/usr/bin/env bash
# Replication diagnostics against an isolated primary + replica pair (MariaDB 11.4) on a
# private Podman network; sockets only under .test-data, no ports on the host.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="$PWD/.build-tools/cargo"
export PATH="$HOME/.cargo/bin:$PATH"
podman network exists harbor-repl || podman network create harbor-repl >/dev/null
start() {
  local name=$1 id=$2; shift 2
  local dir="$PWD/.test-data/repl-$name"
  mkdir -p "$dir/data" "$dir/run"
  if ! podman container exists "harbor-repl-$name"; then
    podman run -d --name "harbor-repl-$name" --network harbor-repl --userns=keep-id --user "$(id -u):$(id -g)" \
      -v "$dir/data:/var/lib/mysql:Z" -v "$dir/run:/run/mysqld:Z" \
      -e MARIADB_ALLOW_EMPTY_ROOT_PASSWORD=1 -e MARIADB_ROOT_HOST=localhost \
      docker.io/library/mariadb:11.4 --socket=/run/mysqld/mariadb.sock --server-id="$id" \
      --log-bin=mariadb-bin --binlog-format=ROW "$@" >/dev/null
  fi
  podman start "harbor-repl-$name" >/dev/null
  for attempt in $(seq 1 60); do
    podman exec "harbor-repl-$name" mariadb-admin --no-defaults --socket=/run/mysqld/mariadb.sock -uroot ping >/dev/null 2>&1 && return
    sleep 1
  done
  podman logs "harbor-repl-$name"; exit 1
}
sql() { podman exec -i "harbor-repl-$1" mariadb --no-defaults --socket=/run/mysqld/mariadb.sock -uroot; }
start primary 1
start replica 2 --read-only=ON
sql replica <<'SQL'
STOP SLAVE;
SQL
# Test-only: the pair starts from a fresh binlog history every run (earlier runs purge logs).
sql primary <<'SQL'
RESET MASTER;
CREATE USER IF NOT EXISTS 'repl'@'%' IDENTIFIED BY 'repl-test-heslo';
GRANT REPLICATION SLAVE ON *.* TO 'repl'@'%';
CREATE DATABASE IF NOT EXISTS harbor_test_guard;
CREATE TABLE IF NOT EXISTS harbor_test_guard.instance(marker VARCHAR(80) NOT NULL);
DELETE FROM harbor_test_guard.instance;
INSERT INTO harbor_test_guard.instance VALUES ('isolated-harbor-integration');
SQL
sql replica <<'SQL'
STOP SLAVE;
RESET SLAVE ALL;
SET GLOBAL gtid_slave_pos = '';
CHANGE MASTER TO MASTER_HOST='harbor-repl-primary', MASTER_USER='repl',
  MASTER_PASSWORD='repl-test-heslo', MASTER_USE_GTID=slave_pos;
START SLAVE;
SQL
sleep 3
HARBOR_TEST_PRIMARY_SOCKET="$PWD/.test-data/repl-primary/run/mariadb.sock" \
HARBOR_TEST_REPLICA_SOCKET="$PWD/.test-data/repl-replica/run/mariadb.sock" \
  cargo test --locked -p harbor-core --test replication -- --ignored --nocapture 2>&1 | tee .test-data/repl-test.log
printf 'Stop with: podman stop harbor-repl-primary harbor-repl-replica\n'
