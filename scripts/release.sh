#!/usr/bin/env bash
# Builds the release: the Arch package and a tarball of the binaries, from the committed
# state of this checkout. With --publish, tags it and uploads both to a GitHub release.
#
#   scripts/release.sh                 build only, into dist/
#   scripts/release.sh --publish 0.2.0 tag v0.2.0, push the tag, create the release
#
# Install the package with: sudo pacman -U dist/yozora-git-*.pkg.tar.zst
set -euo pipefail
cd "$(dirname "$0")/.."

publish=0 version=""
if [[ "${1:-}" == "--publish" ]]; then
  publish=1 version="${2:?usage: release.sh --publish VERSION}"
fi

[[ -z "$(git status --porcelain)" ]] || { echo "release: the working tree has changes; commit them first" >&2; exit 1; }
branch=$(git rev-parse --abbrev-ref HEAD)
if ((publish)); then
  [[ "$branch" == main ]] || { echo "release: publish from main, not $branch" >&2; exit 1; }
  git fetch -q origin main
  [[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] || { echo "release: main isn't up to date with origin" >&2; exit 1; }
  git tag -a "v$version" -m "Yozora $version"
fi

rm -rf dist build-pkg && mkdir -p dist build-pkg
cp packaging/arch/PKGBUILD packaging/arch/yozora.install build-pkg/
# Generic x86-64 rather than this machine's flags, and built on disk: makepkg's default build
# directory can be a small tmpfs.
(cd build-pkg && RUSTFLAGS="" YOZORA_SOURCE="git+file://$PWD/.." PKGDEST="$PWD/../dist" BUILDDIR="$PWD/work" makepkg -f --noconfirm --nodeps)
rm -rf build-pkg

# The package's files as a tarball, for machines that aren't Arch: extract it at /.
rev=$(git describe --tags --always)
stage=$(mktemp -d)
bsdtar -xf dist/yozora-git-*.pkg.tar.zst -C "$stage" --exclude '.*'
tar -C "$stage" -czf "dist/yozora-$rev-x86_64.tar.gz" usr etc
rm -rf "$stage"
(cd dist && sha256sum ./*.pkg.tar.zst ./*.tar.gz > SHA256SUMS)
ls -lh dist

if ((publish)); then
  git push origin "v$version"
  gh release create "v$version" dist/*.pkg.tar.zst dist/*.tar.gz dist/SHA256SUMS --title "Yozora $version" --generate-notes
fi
