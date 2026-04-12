#!/usr/bin/env bash
set -euo pipefail

if (($# != 2)); then
  echo "usage: $0 <package-name> <aur-remote-url>" >&2
  exit 64
fi

package_name=$1
remote_url=$2
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
source_dir="$repo_root/packaging/aur/$package_name"
source "$repo_root/scripts/aur-common.sh"

if [[ ! -f "$source_dir/PKGBUILD" || ! -f "$source_dir/.SRCINFO" ]]; then
  echo "error: missing PKGBUILD or .SRCINFO for $package_name" >&2
  exit 1
fi

tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

git clone --depth 1 "$remote_url" "$tmp_dir/repo"
rsync -a --delete --exclude '.git' "$source_dir/" "$tmp_dir/repo/"

srcinfo_mode=tracked
if [[ "$package_name" == *-git ]]; then
  srcinfo_mode=live
fi
render_aur_srcinfo "$source_dir" "$repo_root" "$srcinfo_mode" > "$tmp_dir/repo/.SRCINFO"

cd "$tmp_dir/repo"
git add -A
if git diff --cached --quiet; then
  echo "AUR repo already up to date for $package_name"
  exit 0
fi

git config user.name "${AUR_GIT_AUTHOR_NAME:-github-actions[bot]}"
git config user.email "${AUR_GIT_AUTHOR_EMAIL:-41898282+github-actions[bot]@users.noreply.github.com}"

pkgver=$(sed -n 's/^\tpkgver = //p' .SRCINFO | head -n1)
pkgrel=$(sed -n 's/^\tpkgrel = //p' .SRCINFO | head -n1)
git commit -m "Update ${package_name} to ${pkgver}-${pkgrel}"
git push origin HEAD:master
