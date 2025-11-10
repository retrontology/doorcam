# Keyboard Debug Controls

The doorcam system now includes keyboard input handling for debugging purposes.

## Features

When running the doorcam application, you can use the following keyboard shortcuts:

### Space Bar - Trigger Motion Event
Press the **SPACE** bar to manually trigger a motion detection event. This will:
- Publish a `MotionDetected` event with a simulated contour area of 5000.0
- Activate the display (if configured)
- Start video capture (if configured)
- Trigger all motion-related workflows

This is useful for:
- Testing motion detection workflows without actual motion
- Debugging capture and storage systems
- Verifying display activation
- Testing event propagation through the system

### Q or ESC - Request Shutdown
Press **Q** or **ESC** to gracefully shutdown the system. This will:
- Publish a `ShutdownRequested` event
- Trigger graceful shutdown of all components
- Exit the application cleanly

## Implementation Details

The keyboard input handler:
- Enables terminal raw mode to capture individual key presses
- Runs in a separate blocking task to avoid blocking the async runtime
- Polls for keyboard events every 100ms
- Only responds to key press events (not release)
- Publishes events to the event bus asynchronously
- Integrates with the component lifecycle management
- Properly disables raw mode on shutdown to restore normal terminal behavior

## Usage

To enable the keyboard handler, use the `--enable-keyboard` flag:

```bash
cargo run -- -c doorcam.toml --enable-keyboard
```

Or with the compiled binary:

```bash
./target/debug/doorcam -c doorcam.toml --enable-keyboard
```

When enabled, you'll see log messages:
```
Keyboard input handler enabled
Keyboard input handler started - press SPACE to trigger motion
Raw mode enabled - keyboard handler active
```

Once raw mode is enabled, your key presses will be captured immediately without needing to press Enter. You won't see what you type on screen (echo is disabled in raw mode).

### Running Without Keyboard Handler (Default)

By default, the keyboard handler is **disabled**. Simply run without the flag:

```bash
cargo run -- -c doorcam.toml
```

This is the recommended mode for production deployments and systemd services, as it:
- Doesn't interfere with terminal input/output
- Doesn't require a TTY
- Avoids raw mode complications
- Is safer for background services

## Notes

- The keyboard handler requires a terminal with stdin available
- It enables terminal raw mode, which means:
  - Key presses are captured immediately (no need to press Enter)
  - Terminal echo is disabled while the handler is active
  - Raw mode is automatically disabled on shutdown
- It works in both debug and release builds
- The handler gracefully shuts down when the system stops
- All keyboard events are logged for debugging purposes
- If the program crashes, you may need to run `reset` in your terminal to restore normal behavior
