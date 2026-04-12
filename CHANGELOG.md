# Changelog

## 0.1.0-alpha.4 - 2026-04-12

Fix packaged desktop installs again by making the bundled control-plane honor its packaged UI root instead of assuming a repo checkout.

### Fixes
- Respect `ACU_UI_ROOT` and `ACU_ARTIFACT_ROOT` at runtime in the control-plane.
- Recut the stable and `-bin` release artifacts so desktop launches no longer render a JSON 404.

## 0.1.0-alpha.3 - 2026-04-12

Fix the packaged desktop launch path so release installs no longer require a separate `playwright-core` npm install just to start.

### Fixes
- Lazy-load Playwright only when browser automation is explicitly enabled.
- Recut the desktop release artifacts for the stable and `-bin` AUR packages.

## 0.1.0-alpha.2 - 2026-04-12

Fix the published desktop package so the AUR release builds the restored `desktop-app` crate again.

### Fixes
- Restore the Tauri desktop app crate, packaged assets, and resource sync script to the workspace.
- Re-enable workspace build/test lanes that prepare the desktop app resources before Rust builds.
- Publish a replacement release tag so the stable AUR package targets a buildable source archive.

## 0.1.0-alpha.1 - 2026-04-10

Initial public alpha release candidate for the repository and Rust workspace crates.

### Highlights
- Linux sandbox sessions with Xvfb-backed regression coverage and Docker-managed QEMU session support.
- Rust guest runtime plus TypeScript control-plane, web UI, sandbox runner, and TypeScript/Python SDK entry points.
- Canonical live desktop view metadata for product QEMU sessions, explicit bridge lifecycle states, and browser trust-boundary routing.
- Crates.io-oriented Rust package metadata, crate-local READMEs, and documented local/runtime installation guidance.
- Architecture, API, security, and QEMU bridge documentation, plus eval task scaffolds and CI basics.

### Known limitations
- The QEMU bridge still uses the current forwarded TCP/HTTP phase transport rather than a VM-native transport.
- DOM-aware browser actions remain explicitly gated behind `ACU_ENABLE_PLAYWRIGHT=1`.
- Replay video, richer accessibility metadata, and broader automation coverage are still future work.
- The JavaScript workspaces remain private and source-distributed for this alpha.
