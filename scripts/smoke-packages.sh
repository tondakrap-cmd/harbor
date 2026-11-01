#!/usr/bin/env bash
# Independent clean containers; installs, upgrades, runs TUI in PTY, removes, checks profiles.
set -euo pipefail
cd "$(dirname "$0")/.."
targets=("$@")
if [[ ${#targets[@]} == 0 ]]; then targets=(ubuntu almalinux); fi
for distro in "${targets[@]}"; do
  if [[ "$distro" != ubuntu && "$distro" != almalinux ]]; then echo 'Use ubuntu or almalinux.' >&2; exit 1; fi
  if [[ "$distro" == ubuntu ]]; then image=docker.io/library/ubuntu:24.04; directory=dist/ubuntu-24.04; else image=docker.io/library/almalinux:9; directory=dist/almalinux-9.8; fi
  test -d "$directory"
  mkdir -p "$directory/smoke"
  podman run --rm --name "harbor-smoke-$distro" \
    -e http_proxy -e https_proxy -e no_proxy -e HTTP_PROXY -e HTTPS_PROXY -e NO_PROXY \
    -v "$PWD/$directory:/packages:ro,z" -v "$PWD/scripts/test-pty.py:/test-pty.py:ro,z" \
    "$image" bash -euc '
      source /etc/os-release
      export XDG_CONFIG_HOME=/tmp/harbor-user-config HARBOR_TUI_BINARY=/usr/bin/harbor-tui
      mkdir -p "$XDG_CONFIG_HOME/harbor"
      echo "profile-preservation-sentinel" > "$XDG_CONFIG_HOME/harbor/keep.txt"
      if [[ "$ID" == ubuntu ]]; then
        apt-get update
        DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends python3 /packages/harbor-tui_*.deb
        test ! -e /usr/bin/harbor
        dpkg-query -W -f="\${Depends}\n" harbor-tui
      else
        dnf install -y python3 /packages/harbor-tui-*.rpm
        test ! -e /usr/bin/harbor
        rpm -q --requires harbor-tui
      fi
      env -u DISPLAY -u WAYLAND_DISPLAY harbor-tui --version
      python3 /test-pty.py
      if [[ "$ID" == ubuntu ]]; then
        apt-get install -y --reinstall /packages/harbor-tui_*.deb
        apt-get install -y /packages/harbor_*.deb
        harbor --version
        apt-get install -y --no-install-recommends xvfb libgl1-mesa-dri
        apt-get install -y --reinstall /packages/harbor_*.deb
      else
        dnf reinstall -y /packages/harbor-tui-*.rpm
        dnf install -y /packages/harbor-0*.rpm
        harbor --version
        dnf install -y xorg-x11-server-Xvfb mesa-dri-drivers
        dnf reinstall -y /packages/harbor-0*.rpm
      fi
      python3 -c "import ctypes; libraries = [\"libX11.so.6\", \"libXcursor.so.1\", \"libXrandr.so.2\", \"libXi.so.6\", \"libxkbcommon.so.0\", \"libxkbcommon-x11.so.0\", \"libwayland-client.so.0\", \"libEGL.so.1\", \"libGL.so.1\"]; [ctypes.CDLL(library) for library in libraries]; print(\"PASS GUI runtime libraries\")"
      # Exercise the installed GUI with Mesa software rendering. This is an Xvfb check,
      # not a replacement for desktop/VM, Wayland or HiDPI validation.
      Xvfb :99 -screen 0 1440x960x24 -ac >/tmp/xvfb.log 2>&1 &
      display_pid=$!
      trap "kill $display_pid 2>/dev/null || true" EXIT
      sleep 1
      set +e
      DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1 timeout 5 harbor --demo >/tmp/gui.log 2>&1
      gui_result=$?
      set -e
      cat /tmp/gui.log
      test "$gui_result" = 124
      if [[ "$ID" == ubuntu ]]; then
        apt-get remove -y harbor harbor-tui
      else
        dnf remove -y harbor harbor-tui
      fi
      test "$(cat "$XDG_CONFIG_HOME/harbor/keep.txt")" = profile-preservation-sentinel
      test ! -e /usr/bin/harbor
      test ! -e /usr/bin/harbor-tui
      echo "PASS clean install, PTY, GUI startup in Xvfb, same-version reinstall and remove; cross-version upgrade not tested"
    ' 2>&1 | tee "$directory/smoke/test.log"
done
