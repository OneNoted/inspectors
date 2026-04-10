# guest-runtime

`guest-runtime` is the Rust HTTP runtime server for Agent Computer Use Platform guest sessions.

## Install

```bash
cargo install guest-runtime --locked
```

## Run from a source checkout

```bash
cargo run -p guest-runtime -- --port 4001
```

See the workspace [`README.md`](../../README.md) for the full control-plane stack, QEMU/Xvfb behavior, and smoke-test workflow.
