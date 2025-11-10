# Keyboard Debug - Quick Start

## Enable Keyboard Input

Add the `--enable-keyboard` flag when starting doorcam:

```bash
cargo run -- -c doorcam.toml --enable-keyboard
```

## Controls

| Key | Action |
|-----|--------|
| **SPACE** | Trigger motion detection event |
| **Q** or **ESC** | Graceful shutdown |
| **Ctrl+C** | Force shutdown (SIGINT) |

## What to Expect

When you start with `--enable-keyboard`, you'll see:
```
Keyboard input handler enabled
Keyboard input handler started - press SPACE to trigger motion
Raw mode enabled - keyboard handler active
```

Press SPACE and you'll see:
```
Space bar pressed - triggering motion event
Motion detected with area: 5000.00
```

This will trigger:
- Display activation
- Video capture
- Storage of the event
- All motion-related workflows

## Production Use

**Don't use `--enable-keyboard` in production!**

For systemd services or background processes, run without the flag:
```bash
doorcam -c /etc/doorcam/doorcam.toml
```

The keyboard handler is disabled by default and won't interfere with non-interactive terminals.

## Troubleshooting

If your terminal gets messed up after a crash, run:
```bash
reset
```

See `KEYBOARD_TROUBLESHOOTING.md` for more details.
