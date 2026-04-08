# Sandbox Notes

## Providers
- `xvfb`: implemented local/dev fallback.
- `qemu`: reserved production target; current code exposes capability information and docs for the target boundary.

## Display stack
The current environment ships with `Xvfb`, `xdotool`, `xprop`, `xrandr`, `import`, and Firefox. That is sufficient for a meaningful local GUI-control loop.
