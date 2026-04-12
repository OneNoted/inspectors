# AUR packaging

This directory is the source of truth for the three AUR package mirrors:

- `inspectors-desktop` — pinned source archive package
- `inspectors-desktop-bin` — prebuilt x86_64 desktop release package
- `inspectors-desktop-git` — VCS package built from the default branch

## Refresh metadata

```bash
./scripts/update-aur-srcinfo.sh
```
Before publishing a new stable release, update `packaging/aur/inspectors-desktop/PKGBUILD` with the release commit or tag archive and refresh its checksum, then regenerate `.SRCINFO`.

Build and upload the matching `inspectors-desktop-bin` release asset before publishing the `-bin` package:

```bash
./scripts/build-desktop-release-asset.sh 0.1.0-alpha.2
gh release upload v0.1.0-alpha.2 artifacts/releases/inspectors-desktop-bin-v0.1.0-alpha.2-x86_64-unknown-linux-gnu.tar.gz
```


## Validate locally

```bash
./scripts/check-aur-packages.sh
```

## Publish manually

```bash
./scripts/publish-aur-package.sh inspectors-desktop ssh://aur@aur.archlinux.org/inspectors-desktop.git
./scripts/publish-aur-package.sh inspectors-desktop-bin ssh://aur@aur.archlinux.org/inspectors-desktop-bin.git
./scripts/publish-aur-package.sh inspectors-desktop-git ssh://aur@aur.archlinux.org/inspectors-desktop-git.git
```

The GitHub Actions workflow under `.github/workflows/aur.yml` uses the same scripts for CI-assisted mirror pushes.

Tracked `.SRCINFO` files stay deterministic in this repository; the publish script refreshes the live `inspectors-desktop-git` version from the current Git HEAD immediately before pushing to the AUR mirror.

Configure the `AUR_SSH_PRIVATE_KEY` repository secret with a deploy key that has write access to the target AUR package repositories before enabling publish jobs.
