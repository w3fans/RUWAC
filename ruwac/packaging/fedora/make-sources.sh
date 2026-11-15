#!/bin/bash
# Build the Fedora/COPR sources and SRPM for packaging/fedora/ruwac.spec.
#
#   make-sources.sh [--tag vX.Y.Z] [--outdir DIR]
#
# Source0 is the release tag (git archive), or the current checkout's tracked
# files when no tag is given; a checkout gets a Fedora snapshot version,
# <cargo version>^<date>git<commit>, which sorts after that release and before
# the next one. Source1 vendors every crate both workspaces need
# (`cargo vendor --locked`), so mock/COPR builds run offline.
set -euo pipefail
umask 022

tag=""
outdir="$PWD/fedora-srpm"
while [ $# -gt 0 ]; do
  case "$1" in
    --tag) tag="$2"; shift 2 ;;
    --outdir) outdir="$2"; shift 2 ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

top="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
if [ -n "$tag" ]; then
  version="${tag#v}"
else
  base="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$top/ruwac/Cargo.toml" | head -1)"
  version="$base^$(date -u +%Y%m%d)git$(git -C "$top" rev-parse --short=8 HEAD)"
fi
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(\^[0-9]{8}git[0-9a-f]{8})?$ ]] || { echo "Invalid version: $version" >&2; exit 1; }

mkdir -p "$outdir"
outdir="$(cd "$outdir" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
prefix="RUWAC-$version"

if [ -n "$tag" ]; then
  git -C "$top" archive --format=tar --prefix="$prefix/" "$tag" | gzip -9n > "$outdir/$prefix.tar.gz"
else
  # Tracked files as they are in the working tree, plus new packaging files.
  (cd "$top" && { git ls-files; git ls-files -o --exclude-standard -- ruwac/packaging/fedora .copr; } \
     | sort -u | while IFS= read -r f; do [ -f "$f" ] && printf '%s\0' "$f"; done \
     | tar --null -T - --transform "s,^,$prefix/," --owner=0 --group=0 --numeric-owner \
           --mtime="@$(git log -1 --format=%ct)" -cf - ) | gzip -9n > "$outdir/$prefix.tar.gz"
fi

tar -xzf "$outdir/$prefix.tar.gz" -C "$work"
(cd "$work/$prefix/ruwac" &&
  cargo vendor --locked --versioned-dirs --quiet \
    --manifest-path Cargo.toml --sync desktop/Cargo.toml cargo-vendor >/dev/null)
tar -C "$work/$prefix/ruwac" --owner=0 --group=0 --numeric-owner --sort=name \
    --mtime="@0" -cJf "$outdir/ruwac-$version-vendor.tar.xz" cargo-vendor

# The SRPM must carry its version: a --define given here is not stored in it.
sed "s/^%{!?ruwac_version:%global ruwac_version [^}]*}$/%global ruwac_version $version/" \
  "$top/ruwac/packaging/fedora/ruwac.spec" > "$outdir/ruwac.spec"
grep -qx "%global ruwac_version $version" "$outdir/ruwac.spec" || { echo "Spec version line not found" >&2; exit 1; }
rpmbuild -bs "$outdir/ruwac.spec" \
  --define "_sourcedir $outdir" --define "_srcrpmdir $outdir" >/dev/null
ls -1 "$outdir"/*.src.rpm
