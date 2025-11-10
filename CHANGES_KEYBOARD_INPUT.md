# Keyboard Input Debug Feature - Implementation Summary

## Overview
Added keyboard input handling to allow manual triggering of motion events for debugging purposes.

## Changes Made

### 1. New Dependencies
- Added `crossterm = "0.27"` to `Cargo.toml` for cross-platform keyboard input handling

### 2. New Module: `src/keyboard_input.rs`
Created a new keyboard input handler module with the following features:
- Listens for keyboard events in a non-blocking manner
- **SPACE** key triggers a motion detection event
- **Q** or **ESC** keys request graceful shutdown
- Runs in a separate blocking task to avoid blocking the async runtime
- Integrates with the event bus to publish events
- Proper lifecycle management (start/stop)

### 3. Integration with Orchestrator
Modified `src/app_orchestration.rs`:
- Added `KeyboardInputHandler` as a component
- Integrated into component lifecycle (initialize, start, stop)
- Added "keyboard" component state tracking
- Proper shutdown handling with timeout

### 4. Library Exports
Updated `src/lib.rs`:
- Added `keyboard_input` module
- Exported `KeyboardInputHandler` for public use

### 5. User Feedback
Modified `src/main.rs`:
- Added informational messages about keyboard controls when system starts

### 6. Documentation
Created `KEYBOARD_DEBUG.md` with:
- Feature description
- Usage instructions
- Implementation details
- Notes and considerations

## How It Works

1. When the orchestrator starts, it creates a `KeyboardInputHandler` instance
2. The handler spawns a blocking task that:
   - Enables terminal raw mode to capture individual key presses
   - Polls for keyboard events every 100ms
3. When SPACE is pressed:
   - A `MotionDetected` event is published with a simulated contour area of 5000.0
   - This triggers all motion-related workflows (display activation, capture, storage)
4. When Q or ESC is pressed:
   - A `ShutdownRequested` event is published
   - The system performs graceful shutdown
5. The handler properly cleans up when the system stops:
   - Disables raw mode to restore normal terminal behavior
   - Cancels the background task
   - Ensures terminal is left in a usable state

## Testing

The implementation:
- ✅ Compiles without errors or warnings
- ✅ Integrates with existing component lifecycle
- ✅ Uses proper async/await patterns
- ✅ Includes unit tests for basic functionality
- ✅ Follows the project's error handling patterns

## Usage

Run the application with the `--enable-keyboard` flag:
```bash
cargo run -- -c doorcam.toml --enable-keyboard
```

Then press SPACE to trigger motion events for debugging.

**Note**: The keyboard handler is disabled by default to avoid interfering with production deployments and systemd services.

## Benefits

- Easy testing of motion detection workflows without actual motion
- Quick debugging of capture and storage systems
- Verification of event propagation
- No need to modify code or create test scripts
- Works in both development and production builds
