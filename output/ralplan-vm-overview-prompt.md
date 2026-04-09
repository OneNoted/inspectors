# Ralplan Prompt: Full VM Oversight View

```text
$ralplan

Plan the work needed to make this repo deliver a proper Cursor-like live VM oversight view.

Context:
I tested the system on April 9, 2026.

Observed behavior:
- `xvfb` sessions do accept actions and record action history, but they do not provide a real live human-facing desktop view.
- `xvfb` observation payloads include screenshot data, but `/api/sessions/:id/screenshot` returned `404` during testing.
- `qemu` sessions do expose a `viewer_url` and reach `runtime_ready`.
- In practice, the human-facing experience is still wrong: I can see "a browser", but not a trustworthy full-desktop / whole-DE / Cursor-style live overview where I can clearly watch the agent operating the VM.
- I need to be able to watch the agent move the mouse, click, type, switch windows, and interact with the full desktop environment in real time.

Goal:
Create a plan for a proper oversight experience where the whole VM desktop is viewable while the agent is acting inside it. The live view must feel like a real operator console, not a partial browser-specific pane.

What I want from this ralplan run:
1. A root-cause analysis of the current viewer pipeline for both `qemu` and `xvfb`.
2. A concrete implementation plan to make the oversight UI show the full desktop environment live.
3. A test strategy and acceptance criteria that prove a human can actually observe agent actions.
4. Explicit commit boundaries up front.
5. A recommendation on whether the right fix is:
   - repairing and standardizing the existing `viewer_url` path,
   - adding a proper desktop streaming path for `xvfb`,
   - changing the web UI embedding/refresh logic,
   - or introducing a canonical "live desktop view" abstraction across providers.

Constraints:
- Use `jj`, not `git`.
- Use Conventional Commits and lore-style commit bodies/trailers.
- Frequent small commits.
- No history rewriting for published work unless explicitly required.
- No new dependencies unless clearly justified.
- Keep action history / auditable receipts intact.
- Prefer deletion and reuse over new abstraction layers.
- This is planning only. Do not implement in this mode.

Acceptance criteria for the eventual implementation:
- The oversight UI shows a live full-desktop view for a session, not just browser content.
- A human can watch pointer motion, clicks, typing, window changes, and app launches.
- The desktop view remains available while the agent performs non-browser actions.
- Session state clearly indicates whether the live view is actual live desktop, fallback screenshot mode, or unavailable.
- For `qemu`, the canonical viewer path is verified end-to-end.
- For `xvfb`, either a true live desktop path exists or the plan explicitly states that `xvfb` is non-goal and why.
- Verification includes at least one visible demo sequence such as launching `xmessage`, moving the mouse, typing into a visible app/window, and showing that a human can see it happen.
- The plan includes both backend and web UI work, if needed.

Please produce:
- A short diagnosis summary.
- A phased implementation plan.
- Commit boundaries.
- Test/verification plan.
- Risks and tradeoffs.
- A plan rating: over-engineered, under-engineered, or perfectly-engineered.
```

## Expected Plan Shape

1. `chore: document current viewer pipeline and add failing acceptance checks`
2. `feat: define canonical live-desktop session contract`
3. `feat: make qemu full-desktop viewer the primary oversight surface`
4. `feat: add or explicitly scope xvfb live-desktop support`
5. `test: add operator-facing verification and regression coverage`
6. `docs: explain live-view states, limitations, and proof workflow`

This is a perfectly-engineered plan target: narrow enough to be actionable, broad enough to fix the actual product gap instead of patching symptoms.
