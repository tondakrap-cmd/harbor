#!/usr/bin/env bash
# Rebuild DEB metadata on Ubuntu using binaries already built on Ubuntu.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_HOME="${CARGO_HOME:-$PWD/.build-tools/cargo}"
output="$PWD/dist/ubuntu-24.04"
version=$(cargo metadata --offline --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
revision=$(cat packaging/revision)
[[ "$revision" =~ ^[1-9][0-9]*$ ]]
test -f "$output/BUILD-INFO.json"
# Refuse to reuse binaries after changes to application code or dependency lockfile.
python3 scripts/build-info.py /tmp/harbor-repackage-source.json
python3 - "$output/BUILD-INFO.json" <<'PY'
import json, sys
previous = json.load(open(sys.argv[1]))
current = json.load(open('/tmp/harbor-repackage-source.json'))
assert previous['source_sha256'] == current['source_sha256'], 'Application sources changed; perform a full Ubuntu build.'
assert 'VERSION_ID="24.04"' in previous['os_release'] and previous['architecture'] == 'x86_64'
PY
podman run --rm --name harbor-repackage-ubuntu \
  -e HARBOR_VERSION="$version" -e HARBOR_REVISION="$revision" \
  -v "$output:/packages:Z" -v "$PWD/packaging:/recipes:ro,z" \
  docker.io/library/ubuntu:24.04 bash -euc '
    for package in harbor harbor-tui; do
      original="/packages/${package}_${HARBOR_VERSION}_amd64.deb"
      if [[ ! -f "$original" ]]; then original="/packages/previous-revision/${package}_${HARBOR_VERSION}_amd64.deb"; fi
      stage=$(mktemp -d)
      dpkg-deb --raw-extract "$original" "$stage"
      original_hash=$(sha256sum "$stage/usr/bin/$package")
      sed -e "s/@VERSION@/$HARBOR_VERSION/g" -e "s/@REVISION@/$HARBOR_REVISION/g" "/recipes/deb/$package.control" > "$stage/DEBIAN/control"
      result="/packages/${package}_${HARBOR_VERSION}-${HARBOR_REVISION}_amd64.deb"
      dpkg-deb --root-owner-group --build "$stage" "$result"
      verify=$(mktemp -d)
      dpkg-deb --extract "$result" "$verify"
      test "${original_hash%% *}" = "$(sha256sum "$verify/usr/bin/$package" | cut -d" " -f1)"
      if [[ "$original" == /packages/* && "$original" != /packages/previous-revision/* ]]; then
        mkdir -p /packages/previous-revision
        mv "$original" /packages/previous-revision/
      fi
      dpkg-deb --field "$result" Package Version Depends
      rm -rf "$stage" "$verify"
    done
    cd /packages
    sha256sum ./*.deb > SHA256SUMS
  ' 2>&1 | tee "$output/repackage.log"
python3 - "$output/BUILD-INFO.json" <<'PY'
import datetime, json, pathlib, sys
path = pathlib.Path(sys.argv[1])
info = json.loads(path.read_text())
current = json.load(open('/tmp/harbor-repackage-source.json'))
info.update({key: current[key] for key in ['packaging_revision', 'packaging_sha256']})
info['repackaged_at_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
info['repackage_note'] = 'DEB metadata rebuilt on Ubuntu 24.04; original Ubuntu binaries preserved byte for byte.'
path.write_text(json.dumps(info, indent=2)+'\n')
PY
