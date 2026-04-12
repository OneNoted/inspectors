# Release checklist

This repository is still source-first overall, but the Rust workspace is staged for an initial `0.1.0-alpha.4` crates.io release candidate.

## Verified locally

Run these commands from the repository root:

```bash
bun run lint
bun run build
bun run test
cargo install --path crates/guest-runtime --locked --root /tmp/acu-install
PORT=3300 GUEST_PORT=4301 ./scripts/dev-start.sh
ACU_BASE_URL=http://127.0.0.1:3300 python scripts/run-smoke-eval.py
```

Observed results for this alpha prep pass:

- lint/build/test pass in the working tree
- `cargo install --path crates/guest-runtime --locked` succeeds
- `scripts/run-smoke-eval.py` succeeds against a locally started stack
- `cargo publish -p desktop-core --dry-run --allow-dirty` succeeds

## Crates.io publish order

The Rust crates form a dependency chain:

1. `desktop-core`
2. `linux-backend` (depends on `desktop-core`)
3. `guest-runtime` (depends on both `desktop-core` and `linux-backend`)

That means the first publish must happen in the same order:

```bash
cargo publish -p desktop-core
cargo publish -p linux-backend
cargo publish -p guest-runtime
```

Until `desktop-core` is published, crates.io cannot resolve dry-runs for `linux-backend` or `guest-runtime`. That is expected for the first release and is not a local build/test failure.

## Notes

- The JavaScript workspaces remain private/source-first in this alpha.
- Repository metadata now points at GitHub; homepage metadata is still optional manual follow-up.
- Keep operator-only publish/release helpers under `.private/` (gitignored), not in tracked repo scripts.
- The Xvfb smoke eval is the current verified end-to-end regression baseline; QEMU remains the default product path documented elsewhere in the repo.
