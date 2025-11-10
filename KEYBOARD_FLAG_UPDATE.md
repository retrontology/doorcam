# Keyboard Handler - CLI Flag Update

## Summary

The keyboard input handler is now **opt-in** via the `--enable-keyboard` CLI flag, making it safe for production deployments.

## Changes Made

### 1. Added CLI Flag
- New flag: `--enable-keyboard`
- Description: "Enable keyboard input handler for debugging motion events"
- Default: **disabled** (safe for production)

### 2. Orchestrator Updates
- Added `keyboard_enabled: bool` field to track state
- Added `set_keyboard_enabled(bool)` method to enable/disable
- Modified `initialize()` to only register keyboard component if enabled
- Modified `start()` to only start keyboard handler if enabled
- Modified `shutdown()` to only stop keyboard handler if enabled

### 3. Main Application Updates
- Reads `--enable-keyboard` flag from CLI args
- Calls `orchestrator.set_keyboard_enabled(true)` when flag is present
- Only shows keyboard controls message when enabled

### 4. Documentation Updates
- Updated `KEYBOARD_DEBUG.md` with flag usage
- Updated `KEYBOARD_TROUBLESHOOTING.md` with flag examples
- Updated `CHANGES_KEYBOARD_INPUT.md` with new usage
- Created `KEYBOARD_QUICK_START.md` for quick reference

## Usage Examples

### Development/Debugging (with keyboard)
```bash
cargo run -- -c doorcam.toml --enable-keyboard
```

### Production/Systemd (without keyboard)
```bash
cargo run -- -c doorcam.toml
```

Or in systemd service:
```ini
[Service]
ExecStart=/usr/local/bin/doorcam -c /etc/doorcam/doorcam.toml
```

## Benefits

1. **Safe by Default**: No keyboard interference in production
2. **No TTY Required**: Works in systemd services without modification
3. **No Raw Mode Issues**: Terminal stays normal unless explicitly enabled
4. **Explicit Intent**: Developer must opt-in to debugging features
5. **Clean Logs**: No keyboard-related messages unless enabled

## Backward Compatibility

This is a **breaking change** if anyone was relying on automatic keyboard handling:
- Old behavior: Keyboard handler always enabled
- New behavior: Keyboard handler disabled by default, requires `--enable-keyboard` flag

However, since this is a debugging feature, the impact should be minimal.

## Testing

Verified that:
- ✅ Compiles without errors
- ✅ Help text shows the new flag
- ✅ Default behavior (no flag) doesn't start keyboard handler
- ✅ With `--enable-keyboard` flag, keyboard handler starts correctly
- ✅ Component lifecycle properly handles enabled/disabled states
- ✅ No diagnostics or warnings

## Files Modified

- `src/main.rs` - Added CLI flag and orchestrator configuration
- `src/app_orchestration.rs` - Added keyboard_enabled field and conditional logic
- `KEYBOARD_DEBUG.md` - Updated usage instructions
- `KEYBOARD_TROUBLESHOOTING.md` - Updated examples
- `CHANGES_KEYBOARD_INPUT.md` - Updated usage section
- `KEYBOARD_QUICK_START.md` - New quick reference guide
