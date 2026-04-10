# Getting started

This guide covers the shortest path from a source checkout to a running local stack.

## Prerequisites

- Bun 1.3.11+
- Node.js 22+
- Python 3.11+
- stable Rust 1.85+

## Install and build

```bash
bun ci
bun run build
```

That installs the JavaScript workspaces and builds both the TypeScript apps and the Rust workspace.

## Start the stack

Use the sandbox runner:

```bash
bun run --filter @acu/sandbox-runner start
```

Or start the same flow through the helper script:

```bash
./scripts/dev-start.sh
```

Then open `http://127.0.0.1:3000`.

## Choose the right session path

- `qemu` + `product` is the main product path.
- `xvfb` is the lighter local fallback and the current regression lane.

If you want a quick local confidence check, run the explicit Xvfb smoke eval:

```bash
ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-smoke-eval.py
```

## Install the Rust binaries locally

Install the runtime:

```bash
cargo install --path crates/guest-runtime --locked
```

Install the schema exporter:

```bash
cargo install --path crates/desktop-core --bin export-schemas --locked
export-schemas ./schemas
```

## Try the examples

- `python/examples/browser_research.py`
- `python/examples/file_terminal.py`
- `python/examples/code_editor.py`

## Read next

- [Architecture](architecture.md)
- [API reference](api-reference.md)
- [QEMU guest bridge](qemu-guest-bridge.md)
- [Security model](security-model.md)
- [Eval tasks and fixtures](../evals/README.md)
