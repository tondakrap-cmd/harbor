#!/usr/bin/env bash
# A private sshd only inside each test container; no published port or host service changes.
set -euo pipefail
cd "$(dirname "$0")/.."
for distro in ubuntu almalinux; do
  if [[ "$distro" == ubuntu ]]; then image=docker.io/library/ubuntu:24.04; directory=dist/ubuntu-24.04; else image=docker.io/library/almalinux:9; directory=dist/almalinux-9.8; fi
  podman run --rm --name "harbor-ssh-$distro" \
    -e http_proxy -e https_proxy -e no_proxy -e HTTP_PROXY -e HTTPS_PROXY -e NO_PROXY \
    -v "$PWD/$directory:/packages:ro,z" -v "$PWD/scripts:/tests:ro,z" \
    "$image" bash -euc '
      source /etc/os-release
      export DEBIAN_FRONTEND=noninteractive HARBOR_TUI_BINARY=/usr/bin/harbor-tui
      if [[ "$ID" == ubuntu ]]; then
        apt-get update
        apt-get install -y --no-install-recommends python3 passwd openssh-server openssh-client tmux /packages/harbor-tui_0.1.0-2_amd64.deb
      else
        dnf install -y python3 shadow-utils openssh-server openssh-clients tmux /packages/harbor-tui-0.1.0-2.el9.x86_64.rpm
      fi
      test ! -e /usr/bin/harbor
      # Unlock only the disposable container account; password login remains disabled.
      # The random password travels through stdin and is never printed or passed as an argument.
      python3 -c "import secrets; print(\"root:\" + secrets.token_urlsafe(32))" | chpasswd
      mkdir -p /tmp/harbor-ssh /run/sshd /root/.ssh
      chmod 700 /tmp/harbor-ssh
      chmod 700 /root/.ssh
      ssh-keygen -A
      ssh-keygen -q -t ed25519 -N "" -f /tmp/harbor-ssh/client
      cp /tmp/harbor-ssh/client.pub /root/.ssh/authorized_keys
      chmod 600 /root/.ssh/authorized_keys
      read -r keytype keydata comment </etc/ssh/ssh_host_ed25519_key.pub
      printf "[127.0.0.1]:2222 %s %s\n" "$keytype" "$keydata" > /tmp/harbor-ssh/known_hosts
      printf "%s\n" "ListenAddress 127.0.0.1" "Port 2222" "PidFile /tmp/harbor-ssh/server.pid" "HostKey /etc/ssh/ssh_host_ed25519_key" "AuthorizedKeysFile /root/.ssh/authorized_keys" "PermitRootLogin prohibit-password" "PasswordAuthentication no" "KbdInteractiveAuthentication no" "UsePAM no" "LogLevel ERROR" > /tmp/harbor-ssh/server.conf
      /usr/sbin/sshd -D -f /tmp/harbor-ssh/server.conf >/tmp/harbor-ssh/server.log 2>&1 &
      server_pid=$!
      trap "kill $server_pid 2>/dev/null || true" EXIT
      sleep 1
      kill -0 "$server_pid"
      env -u DISPLAY -u WAYLAND_DISPLAY python3 /tests/test-ssh-tmux.py
      echo "PASS TUI-only installation, local tmux, SSH PTY, tmux over SSH, resize and terminal cleanup"
    ' 2>&1 | tee "$directory/ssh-tmux.log"
done
