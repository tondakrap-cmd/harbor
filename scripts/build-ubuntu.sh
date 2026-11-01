#!/usr/bin/env bash
# Native Ubuntu 24.04 build in an isolated container, launched from a normal terminal.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "${1:-}" == --repackage ]]; then exec bash scripts/repackage-ubuntu.sh; fi
mkdir -p .build-tools dist/ubuntu-24.04
tar --exclude=./.git --exclude=./.agents --exclude=./.aws --exclude=./.codex --exclude=./.build-tools --exclude=./.test-data --exclude=./target --exclude=./dist -czf .build-tools/ubuntu-source.tar.gz .
podman run --rm --name harbor-build-ubuntu \
  -e http_proxy -e https_proxy -e no_proxy -e HTTP_PROXY -e HTTPS_PROXY -e NO_PROXY \
  -v "$PWD/.build-tools/ubuntu-source.tar.gz:/source.tar.gz:ro,z" \
  -v "$PWD/.build-tools/cargo:/cargo:z" -v "$PWD/dist/ubuntu-24.04:/out:Z" \
  docker.io/library/ubuntu:24.04 bash -euc '
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends ca-certificates curl gcc g++ make pkg-config libssl-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxkbcommon-dev libwayland-dev libegl1-mesa-dev libgl1-mesa-dev python3 dpkg-dev ripgrep
    curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup.sh
    sh /tmp/rustup.sh -y --profile minimal --default-toolchain 1.88.0
    export PATH="/root/.cargo/bin:$PATH" CARGO_HOME=/cargo
    mkdir /work
    cd /work
    tar -xzf /source.tar.gz
    cargo test --locked --offline --workspace
    bash scripts/package.sh
    cp dist/ubuntu-24.04/* /out/
    cp /etc/os-release /out/build-os-release.txt
  ' 2>&1 | tee dist/ubuntu-24.04/build.log
