#!/usr/bin/env bash
set -euo pipefail

aur_repo_root() {
  cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd
}

aur_git_pkgver() {
  local repo_root=${1:-$(aur_repo_root)}
  local base_version
  base_version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -n1)
  base_version=${base_version//-alpha./alpha}
  base_version=${base_version//-beta./beta}
  base_version=${base_version//-rc./rc}
  base_version=${base_version//-/.}
  printf '%s.r%s.g%s\n' \
    "$base_version" \
    "$(git -C "$repo_root" rev-list --count HEAD)" \
    "$(git -C "$repo_root" rev-parse --short HEAD)"
}

render_aur_srcinfo() {
  local pkg_dir=$1
  local repo_root=${2:-$(aur_repo_root)}
  local mode=${3:-tracked}
  local rendered
  rendered=$(cd "$pkg_dir" && env CARCH=x86_64 makepkg --printsrcinfo)

  if [[ "$mode" == live && $(basename "$pkg_dir") == *-git ]]; then
    local computed_pkgver
    computed_pkgver=$(aur_git_pkgver "$repo_root")
    rendered=$(printf '%s\n' "$rendered" | sed "0,/^[[:space:]]*pkgver = .*/s//\tpkgver = ${computed_pkgver}/")
  fi

  printf '%s\n' "$rendered"
}
