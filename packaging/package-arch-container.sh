#!/bin/sh
# Build the binary Arch wrapper in a disposable, pinned tool environment.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT HUP INT TERM
install -d "$stage/packaging/arch" "$stage/dist"
cp "$root/packaging/package-arch.sh" "$stage/packaging/"
cp "$root/packaging/arch/PKGBUILD" "$stage/packaging/arch/"
cp "$root/dist/codex-dock-linux-$(uname -m).tar.gz" "$stage/dist/"
docker run --rm \
  -e BUILD_UID="$(id -u)" -e BUILD_GID="$(id -g)" \
  -v "$stage:/work" \
  archlinux:base-devel@sha256:fa2d4ce3310ddd90a4208e5168be8ad367da5129af1245f38fd2d803bcbe3239 \
  bash -euc '
    groupadd -g "$BUILD_GID" builder
    useradd -m -u "$BUILD_UID" -g "$BUILD_GID" builder
    chown -R "$BUILD_UID:$BUILD_GID" /work
    cd /work
    runuser -u builder -- sh packaging/package-arch.sh
  '
cp "$stage"/dist/*.pkg.tar.zst "$root/dist/"
(cd "$root/dist" && sha256sum -- ./*.pkg.tar.zst > SHA256SUMS.arch)
