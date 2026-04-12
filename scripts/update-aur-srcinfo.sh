#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
aur_root="$repo_root/packaging/aur"
source "$repo_root/scripts/aur-common.sh"

mode=tracked
if (($# > 0)) && [[ "$1" == '--live-git-version' ]]; then
  mode=live
  shift
fi

declare -a pkg_dirs=()
if (($# == 0)); then
  for pkg_dir in "$aur_root"/*; do
    [[ -f "$pkg_dir/PKGBUILD" ]] || continue
    pkg_dirs+=("$pkg_dir")
  done
else
  for pkg in "$@"; do
    if [[ "$pkg" != */* ]]; then
      pkg="$aur_root/$pkg"
    fi
    [[ -f "$pkg/PKGBUILD" ]] || {
      echo "error: missing PKGBUILD under $pkg" >&2
      exit 1
    }
    pkg_dirs+=("$pkg")
  done
fi

for pkg_dir in "${pkg_dirs[@]}"; do
  echo "==> generating $(realpath --relative-to="$repo_root" "$pkg_dir")/.SRCINFO"
  render_aur_srcinfo "$pkg_dir" "$repo_root" "$mode" > "$pkg_dir/.SRCINFO"
done
