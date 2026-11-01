#!/usr/bin/env bash
# Host access (SSH, systemd, journal, option file) against an isolated AlmaLinux 9
# container with systemd, sshd and MariaDB. Needs rootless Podman; listens on 127.0.0.1 only.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="$PWD/.build-tools/cargo"
export PATH="$HOME/.cargo/bin:$PATH"
dir="$PWD/.test-data/host"
mkdir -p "$dir" && chmod 700 "$dir"
[[ -f "$dir/id_ed25519" ]] || ssh-keygen -q -t ed25519 -N "" -C harbor-host-test -f "$dir/id_ed25519"
cat > "$dir/Containerfile" <<'CONTAINER'
FROM docker.io/library/almalinux:9
RUN dnf -y install mariadb-server openssh-server sudo procps-ng && dnf clean all
RUN useradd -m harbor && mkdir -p /home/harbor/.ssh && chmod 700 /home/harbor/.ssh
COPY id_ed25519.pub /home/harbor/.ssh/authorized_keys
RUN chown -R harbor:harbor /home/harbor/.ssh && chmod 600 /home/harbor/.ssh/authorized_keys \
 && printf 'harbor ALL=(root) NOPASSWD: /usr/bin/systemctl, /usr/bin/journalctl, /usr/bin/tee, /usr/bin/cp, /usr/bin/mv, /usr/bin/rm, /usr/bin/cat, /usr/bin/tail, /usr/bin/test, /usr/bin/mktemp, /usr/libexec/mariadbd, /usr/sbin/mariadbd\n' > /etc/sudoers.d/harbor \
 && chmod 440 /etc/sudoers.d/harbor && systemctl enable mariadb sshd \
 && sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config
CMD ["/sbin/init"]
CONTAINER
podman build -q -t harbor-host-test "$dir" >/dev/null
if ! podman container exists harbor-host-test; then
  podman run -d --name harbor-host-test --systemd=always \
    -p 127.0.0.1:2222:22 -p 127.0.0.1:33318:3306 harbor-host-test >/dev/null
fi
podman start harbor-host-test >/dev/null
for attempt in $(seq 1 60); do
  podman exec harbor-host-test systemctl is-active --quiet mariadb sshd && break
  sleep 1
done
podman exec harbor-host-test mysql -uroot -e "CREATE USER IF NOT EXISTS 'harbor'@'%' IDENTIFIED BY 'host-test-heslo'; GRANT ALL ON *.* TO 'harbor'@'%' WITH GRANT OPTION;"
ssh -i "$dir/id_ed25519" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$dir/known_hosts" \
  -o BatchMode=yes -p 2222 harbor@127.0.0.1 true
HARBOR_TEST_SSH_PORT=2222 HARBOR_TEST_HOST_DB_PORT=33318 \
HARBOR_SSH_IDENTITY="$dir/id_ed25519" HARBOR_SSH_KNOWN_HOSTS="$dir/known_hosts" \
  cargo test --locked -p harbor-core --test host -- --ignored --nocapture 2>&1 | tee "$dir/test.log"
printf 'Stop with: podman stop harbor-host-test\n'
