#!/usr/bin/env bash
# Test the real packaging upgrade from revision 1 to 2 without touching host packages.
set -euo pipefail
cd "$(dirname "$0")/.."
for distro in ubuntu almalinux; do
  if [[ "$distro" == ubuntu ]]; then
    image=docker.io/library/ubuntu:24.04
    directory=dist/ubuntu-24.04
  else
    image=docker.io/library/almalinux:9
    directory=dist/almalinux-9.8
  fi
  test -d "$directory/previous-revision"
  baseline="$PWD/$directory/previous-revision"
  if [[ "$distro" == almalinux ]]; then
    # Revision 1's original Wayland dependency was not installable on AlmaLinux.
    # Use an explicit fixture with the correct Wayland package and without the new XKB library.
    baseline="$PWD/.test-data/rpm-upgrade-build/RPMS/x86_64"
    if [[ ! -f "$baseline/harbor-0.1.0-1.el9.x86_64.rpm" ]]; then
      bash scripts/prepare-upgrade-fixture.sh
    fi
  fi
  podman run --rm --name "harbor-upgrade-$distro" \
    -e http_proxy -e https_proxy -e no_proxy -e HTTP_PROXY -e HTTPS_PROXY -e NO_PROXY \
    -v "$PWD/$directory:/packages:ro,z" -v "$baseline:/baseline:ro,z" -v "$PWD/scripts/test-pty.py:/test-pty.py:ro,z" \
    "$image" bash -euc '
      source /etc/os-release
      trap "echo UPGRADE_TEST_FAILED >&2" ERR
      export DEBIAN_FRONTEND=noninteractive XDG_CONFIG_HOME=/tmp/harbor-upgrade-config HARBOR_TUI_BINARY=/usr/bin/harbor-tui
      if [[ "$ID" == ubuntu ]]; then
        apt-get update
        apt-get install -y --no-install-recommends python3 /baseline/harbor-tui_0.1.0_amd64.deb /baseline/harbor_0.1.0_amd64.deb
        test "$(dpkg-query -W -f="\${Version}" harbor)" = 0.1.0
      else
        if [[ -f /baseline/fixture.json ]]; then cat /baseline/fixture.json; fi
        dnf install -y python3 /baseline/harbor-tui-0.1.0-1.el9.x86_64.rpm /baseline/harbor-0.1.0-1.el9.x86_64.rpm
        test "$(rpm -q --qf "%{VERSION}-%{RELEASE}" harbor)" = 0.1.0-1.el9
      fi
      mkdir -p "$XDG_CONFIG_HOME/harbor"
      python3 -c "import json,os,pathlib; p=pathlib.Path(os.environ[\"XDG_CONFIG_HOME\"])/\"harbor/settings.json\"; p.write_text(json.dumps({\"profiles\": [{\"name\": \"Uložený profil\", \"transport\": {\"Tcp\": {\"host\": \"localhost\", \"port\": 3306}}, \"user\": \"harbor\", \"tls\": \"VerifyIdentity\", \"ca\": None}], \"desktop_theme\": \"terra\", \"tui_theme\": \"nocturne\", \"refresh_seconds\": 10}))"
      before=$(sha256sum "$XDG_CONFIG_HOME/harbor/settings.json")
      if [[ "$ID" == ubuntu ]]; then
        apt-get install -y /packages/harbor-tui_0.1.0-2_amd64.deb /packages/harbor_0.1.0-2_amd64.deb
        test "$(dpkg-query -W -f="\${Version}" harbor)" = 0.1.0-2
        test "$(dpkg-query -W -f="\${Version}" harbor-tui)" = 0.1.0-2
        dpkg-query -W libxkbcommon-x11-0
      else
        dnf upgrade -y /packages/harbor-tui-0.1.0-2.el9.x86_64.rpm /packages/harbor-0.1.0-2.el9.x86_64.rpm
        test "$(rpm -q --qf "%{VERSION}-%{RELEASE}" harbor)" = 0.1.0-2.el9
        test "$(rpm -q --qf "%{VERSION}-%{RELEASE}" harbor-tui)" = 0.1.0-2.el9
        rpm -q libxkbcommon-x11
      fi
      test "$before" = "$(sha256sum "$XDG_CONFIG_HOME/harbor/settings.json")"
      test -x /usr/bin/harbor
      test -x /usr/bin/harbor-tui
      python3 -c "import ctypes; ctypes.CDLL(\"libxkbcommon-x11.so.0\"); print(\"PASS new X11 runtime dependency resolved during upgrade\")"
      python3 /test-pty.py
      harbor --version
      if [[ "$ID" == ubuntu ]]; then apt-get remove -y harbor harbor-tui; else dnf remove -y harbor harbor-tui; fi
      test "$before" = "$(sha256sum "$XDG_CONFIG_HOME/harbor/settings.json")"
      test ! -e /usr/bin/harbor
      test ! -e /usr/bin/harbor-tui
      echo "PASS revision 1 to 2 upgrade, installed TUI, new GUI library and preserved profiles/themes after upgrade and removal"
    ' 2>&1 | tee "$directory/upgrade.log"
done
