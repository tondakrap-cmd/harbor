#!/usr/bin/env bash
# Controlled prior-package fixture for the RPM upgrade test, using native Alma binaries.
set -euo pipefail
cd "$(dirname "$0")/.."
source /etc/os-release
if [[ "$ID" != almalinux || "$VERSION_ID" != 9* ]]; then
  echo 'Prepare the RPM fixture on AlmaLinux 9 after the native package build.' >&2
  exit 1
fi
test -f target/release/harbor
test -f target/release/harbor-tui
mkdir -p .test-data/packaging
python3 - <<'PY'
from pathlib import Path
spec = Path('packaging/rpm/harbor.spec').read_text()
Path('.test-data/packaging/harbor-upgrade-baseline.spec').write_text(spec.replace(', libxkbcommon-x11,', ','))
PY
rpmbuild -bb .test-data/packaging/harbor-upgrade-baseline.spec \
  --define "_topdir $PWD/.test-data/rpm-upgrade-build" --define '_tmppath /tmp' \
  --define "harbor_repo $PWD" --define 'harbor_version 0.1.0' --define 'harbor_release 1'
python3 - <<'PY'
from pathlib import Path
import json
fixture = {'purpose':'Controlled packaging-upgrade fixture, not a previously released application build', 'version':'0.1.0-1.el9', 'binaries':'Current binaries built natively on AlmaLinux 9', 'wayland_dependency':'Corrected to libwayland-client so the baseline can install', 'omitted_dependency':'libxkbcommon-x11; revision 2 must resolve it automatically'}
Path('.test-data/rpm-upgrade-build/RPMS/x86_64/fixture.json').write_text(json.dumps(fixture,indent=2)+'\n')
PY
