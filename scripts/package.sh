#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
source /etc/os-release
if [[ $(uname -m) != x86_64 ]]; then echo 'Only the x86_64 matrix is configured.' >&2; exit 1; fi
if [[ "$ID" == ubuntu && "$VERSION_ID" == 24.04 ]]; then format=deb
elif [[ "$ID" == almalinux && "$VERSION_ID" == 9* ]]; then format=rpm
else echo 'Build packages on Ubuntu 24.04 or AlmaLinux 9, respectively.' >&2; exit 1; fi
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_HOME="${CARGO_HOME:-$PWD/.build-tools/cargo}"
test -f Cargo.lock
cargo build --locked --offline --release -p harbor-tui -p harbor-desktop
if cargo tree --locked --offline -p harbor-tui --prefix none | rg '^(eframe|egui|winit|glutin|wayland-client|x11-dl) '; then
  echo 'Unexpected desktop dependency in TUI' >&2; exit 1
fi
version=$(cargo metadata --offline --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
revision=$(cat packaging/revision)
[[ "$revision" =~ ^[1-9][0-9]*$ ]]
python3 scripts/licenses.py
mkdir -p "dist/$ID-$VERSION_ID"
output="$PWD/dist/$ID-$VERSION_ID"
if [[ "$format" == rpm ]]; then
  mkdir -p "$PWD/dist/rpmbuild"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
  rpmbuild -bb packaging/rpm/harbor.spec --define "_topdir $PWD/dist/rpmbuild" --define '_tmppath /tmp' --define "harbor_repo $PWD" --define "harbor_version $version" --define "harbor_release $revision"
  for package in harbor harbor-tui; do
    cp "dist/rpmbuild/RPMS/x86_64/${package}-${version}-${revision}.el9.x86_64.rpm" "$output/"
    previous="$output/${package}-${version}-1.el9.x86_64.rpm"
    if [[ "$revision" != 1 && -f "$previous" ]]; then
      mkdir -p "$output/previous-revision"
      mv "$previous" "$output/previous-revision/"
    fi
  done
else
  for package in harbor harbor-tui; do
    stage=$(mktemp -d "$PWD/dist/deb-stage.XXXXXX")
    mkdir -p "$stage/DEBIAN" "$stage/usr/bin" "$stage/usr/share/doc/$package"
    sed -e "s/@VERSION@/$version/g" -e "s/@REVISION@/$revision/g" "packaging/deb/$package.control" > "$stage/DEBIAN/control"
    install -m 755 "target/release/$package" "$stage/usr/bin/$package"
    install -m 644 LICENSE "$stage/usr/share/doc/$package/copyright"
    install -m 644 README.md "$stage/usr/share/doc/$package/README.md"
    cp -r "target/licenses/$package" "$stage/usr/share/doc/$package/third-party"
    if [[ "$package" == harbor ]]; then
      mkdir -p "$stage/usr/share/applications" "$stage/usr/share/icons/hicolor/scalable/apps"
      install -m 644 packaging/harbor.desktop "$stage/usr/share/applications/harbor.desktop"
      install -m 644 packaging/harbor.svg "$stage/usr/share/icons/hicolor/scalable/apps/harbor.svg"
      cat crates/harbor-desktop/assets/FONT-LICENSE.txt >> "$stage/usr/share/doc/$package/copyright"
    fi
    dpkg-deb --root-owner-group --build "$stage" "$output/${package}_${version}-${revision}_amd64.deb"
    previous="$output/${package}_${version}_amd64.deb"
    if [[ -f "$previous" ]]; then
      mkdir -p "$output/previous-revision"
      mv "$previous" "$output/previous-revision/"
    fi
    rm -rf "$stage"
  done
fi
(cd "$output" && sha256sum ./*."$format" > SHA256SUMS)
python3 scripts/build-info.py "$output/BUILD-INFO.json"
printf 'Packages created in %s. Installation and runtime validation are separate checks.\n' "$output"
