# Keyboard Input Troubleshooting

## Issue: Keys don't do anything

### Solution
The keyboard handler now uses **raw mode** which captures key presses immediately. Make sure you see these log messages when the app starts:

```
Keyboard input handler started - press SPACE to trigger motion
Raw mode enabled - keyboard handler active
```

If you don't see "Raw mode enabled", the handler failed to initialize.

## Issue: Terminal is messed up after the program exits

### Solution
If the program crashes or exits abnormally, raw mode might not be properly disabled. To fix your terminal:

```bash
reset
```

Or:

```bash
stty sane
```

This restores normal terminal behavior.

## Issue: Can't see what I'm typing

### Expected Behavior
This is normal! When raw mode is enabled:
- Key presses are captured immediately (no need to press Enter)
- Terminal echo is disabled (you won't see what you type)
- This is intentional for the keyboard handler to work

## Issue: Ctrl+C doesn't work

### Solution
Ctrl+C should still work to send SIGINT. If it doesn't:
1. Try pressing `q` or `ESC` to trigger graceful shutdown
2. If that doesn't work, use `Ctrl+Z` to suspend the process, then `kill %1`
3. As a last resort, from another terminal: `pkill doorcam`

## Testing the Keyboard Handler

To verify the keyboard handler is working:

1. Start the application with the keyboard flag:
   ```bash
   cargo run -- -c doorcam.toml --enable-keyboard
   ```

2. Look for these messages:
   ```
   Keyboard input handler started - press SPACE to trigger motion
   Raw mode enabled - keyboard handler active
   ```

3. Press the SPACE bar - you should see:
   ```
   Space bar pressed - triggering motion event
   Motion detected with area: 5000.00
   ```

4. Press `q` to exit gracefully:
   ```
   Quit key pressed - requesting shutdown
   Shutdown initiated: Signal("ShutdownRequested")
   ```

## Debug Mode

For more verbose output, run with debug logging:

```bash
cargo run -- -c doorcam.toml --enable-keyboard --debug
```

This will show all key presses including ignored keys:
```
Key pressed: Char('a')
Key pressed: Char('b')
Key pressed: Char(' ')  # This triggers motion
```

## Common Issues

### "Failed to enable raw mode"
- **Cause**: Not running in a proper terminal (e.g., running as a systemd service without a TTY)
- **Solution**: The keyboard handler will fail gracefully. For production deployments, consider disabling the keyboard handler or running with a pseudo-TTY

### Keys work but events don't trigger
- **Check**: Make sure the event bus is properly initialized
- **Check**: Look for "Failed to publish motion event" warnings in the logs
- **Debug**: Run with `--debug` flag to see all event bus activity

### Terminal stays in raw mode after crash
- **Quick fix**: Run `reset` command
- **Prevention**: The handler now has better cleanup on shutdown
- **Note**: Ctrl+C (SIGINT) is handled and should properly clean up

## Production Deployment

For production deployments (e.g., systemd service), simply **don't use the `--enable-keyboard` flag**. The keyboard handler is disabled by default and won't interfere with background services or non-interactive terminals.

Example systemd service file:
```ini
[Service]
ExecStart=/usr/local/bin/doorcam -c /etc/doorcam/doorcam.toml
# Note: No --enable-keyboard flag for production
```
