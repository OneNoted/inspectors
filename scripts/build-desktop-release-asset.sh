#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
version=${1:-$(sed -n '/^version = "/s///p' "$repo_root/Cargo.toml" | head -n1 | tr -d '"')}
target=${2:-x86_64-unknown-linux-gnu}
asset_name="inspectors-desktop-bin-v${version}-${target}.tar.gz"
stage_dir=$(mktemp -d)
asset_root="$stage_dir/inspectors-desktop-bin-v${version}-${target}"
out_dir="$repo_root/artifacts/releases"

cleanup() {
  rm -rf "$stage_dir"
}
trap cleanup EXIT

mkdir -p "$asset_root/bin" "$asset_root/share/applications" "$asset_root/share/icons/hicolor/32x32/apps"

(
  cd "$repo_root"
  bun run --workspaces --if-present build
  node scripts/sync-desktop-resources.mjs
  cargo build --release --locked -p desktop-app --bin inspectors-desktop
)

install -Dm755 "$repo_root/target/release/inspectors-desktop" "$asset_root/bin/inspectors-desktop"
install -Dm644 "$repo_root/crates/desktop-app/packaging/inspectors.desktop" "$asset_root/share/applications/inspectors.desktop"
install -Dm644 "$repo_root/crates/desktop-app/icons/icon.png" "$asset_root/share/icons/hicolor/32x32/apps/inspectors.png"
mkdir -p "$out_dir"
tar -C "$stage_dir" -czf "$out_dir/$asset_name" "$(basename "$asset_root")"
printf '%s\n' "$out_dir/$asset_name"
