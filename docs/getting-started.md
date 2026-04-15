# Getting started

This guide covers the shortest path from a source checkout to a running local stack.

## Prerequisites

- Bun 1.3.11+
- Node.js 22+
- Python 3.11+
- stable Rust 1.85+

If you are on Linux with Nix enabled, you can skip the host-level installs above and use the repo flake instead:

```bash
nix develop
```

The flake shell provides Bun, Node 22, Python 3.11, Rust, Firefox, Xvfb/X11 helpers, ImageMagick, and the Docker client needed for the source-first workflow. It does not replace host virtualization setup for the QEMU product path.

## Install and build

```bash
bun ci
bun run build
```

That installs the JavaScript workspaces and builds both the TypeScript apps and the Rust workspace.

With Nix, the same flow is:

```bash
nix develop -c bun ci
nix develop -c bun run build
```

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

## Default operator + agent loop

Use this as the primary workflow:

1. Start the default session path (`qemu` + `product`)
2. Wait until the session is ready
3. Create a task or issue actions through the API/SDK
4. Watch the returned live desktop view (or truthful screenshot fallback)
5. Delete the session when done unless you explicitly export artifacts

The UI keeps provider overrides, attach-existing-session, and manual actions under Advanced / Debug so the happy path stays simple.

If you want a quick local confidence check, run the explicit Xvfb smoke eval:

```bash
ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-smoke-eval.py
```

## Install the Rust binaries locally

Install the runtime:

```bash
cargo install --path crates/guest-runtime --locked
```

Or install it from the flake:

```bash
nix profile install .#guest-runtime
```

Install the schema exporter:

```bash
cargo install --path crates/desktop-core --bin export-schemas --locked
export-schemas ./schemas
```

Or install it from the flake:

```bash
nix profile install .#export-schemas
export-schemas ./schemas
```

## Try the examples

- `python/examples/browser_research.py`
- `python/examples/file_terminal.py`
- `python/examples/code_editor.py`

## Minimal AGENTS.md guidance

If you want an agent to discover the default workflow from a short instruction block, use something like:

```md
You have access to inspectors. Start a session with default settings, wait for readiness, run the task, use the live desktop view or screenshot fallback to observe progress, and delete the session when done unless you explicitly export artifacts.
```

## Read next

- [Architecture](architecture.md)
- [API reference](api-reference.md)
- [QEMU guest bridge](qemu-guest-bridge.md)
- [Security model](security-model.md)
- [Eval tasks and fixtures](../evals/README.md)
