#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
aur_root="$repo_root/packaging/aur"
source "$repo_root/scripts/aur-common.sh"
status=0

for pkg_dir in "$aur_root"/*; do
  [[ -f "$pkg_dir/PKGBUILD" ]] || continue
  echo "==> validating $(basename "$pkg_dir")"
  generated=$(mktemp)
  render_aur_srcinfo "$pkg_dir" "$repo_root" > "$generated"
  if ! cmp -s "$generated" "$pkg_dir/.SRCINFO"; then
    echo "error: $pkg_dir/.SRCINFO is out of date"
    diff -u "$pkg_dir/.SRCINFO" "$generated" || true
    status=1
  fi
  rm -f "$generated"

  if command -v namcap >/dev/null 2>&1; then
    (cd "$pkg_dir" && namcap PKGBUILD)
  else
    echo "note: namcap not installed; skipping PKGBUILD lint"
  fi
  echo
 done

exit "$status"
