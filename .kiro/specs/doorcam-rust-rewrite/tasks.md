# Implementation Plan

- [x] 1. Clean up existing Python implementation
  - Remove Python source files from src/doorcam directory
  - Remove Python tools and utilities from src/tools directory
  - Keep only essential configuration files and documentation
  - _Requirements: Project cleanup for Rust rewrite_

- [x] 2. Set up project structure and core configuration






  - Create Rust project with proper Cargo.toml dependencies
  - Implement TOML configuration system with environment variable overrides
  - Create example configuration file with documented defaults
  - Set up structured logging with tracing
  - _Requirements: 5.1, 5.2, 5.3, 5.5, 7.4_

- [x] 3. Implement ring buffer and frame management





  - [x] 3.1 Create lock-free circular buffer for frame storage


    - Implement RingBuffer struct with atomic operations
    - Add thread-safe frame push and retrieval methods
    - Implement preroll frame collection based on timestamp
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5_

  - [x] 3.2 Define frame data structures and formats


    - Create FrameData struct with metadata
    - Implement frame format enum (MJPEG, YUYV, RGB24)
    - Add frame processing utilities for rotation and conversion
    - _Requirements: 11.1, 11.4_

- [ ] 4. Implement camera interface with V4L2
  - [ ] 4.1 Create V4L2 camera capture
    - Implement CameraInterface using v4l crate
    - Configure camera resolution, FPS, and format settings
    - Add hardware-accelerated capture support
    - _Requirements: 5.4, 12.1_

  - [ ] 4.2 Integrate camera with ring buffer
    - Connect camera capture loop to ring buffer
    - Implement frame rate control and timing
    - Add error handling and reconnection logic
    - _Requirements: 7.3, 12.5_

- [ ] 5. Create event system for component coordination
  - [ ] 5.1 Implement async event bus
    - Create DoorcamEvent enum with all event types
    - Implement EventBus with broadcast channels
    - Add event publishing and subscription methods
    - _Requirements: 1.1, 1.5_

  - [ ] 5.2 Define event handling patterns
    - Create event receiver patterns for components
    - Implement event filtering and routing
    - Add event logging and debugging support
    - _Requirements: 7.1, 7.2_

- [ ] 6. Implement motion detection analyzer
  - [ ] 6.1 Create OpenCV-based motion analyzer
    - Implement background subtraction using MOG2
    - Add contour detection and area filtering
    - Configure motion detection thresholds
    - _Requirements: 1.2, 1.3, 1.4, 12.3_

  - [ ] 6.2 Integrate motion analyzer with ring buffer and events
    - Connect analyzer to ring buffer frame stream
    - Publish motion detection events to event bus
    - Add motion detection logging and metrics
    - _Requirements: 1.1, 1.5_

- [ ] 7. Implement MJPEG streaming server
  - [ ] 7.1 Create HTTP server with Axum
    - Implement MJPEG streaming endpoint
    - Add concurrent client connection support
    - Handle client disconnections gracefully
    - _Requirements: 3.1, 3.2, 3.3, 3.5_

  - [ ] 7.2 Integrate streaming with ring buffer
    - Connect stream server to ring buffer frames
    - Implement frame rate synchronization
    - Add JPEG encoding for non-MJPEG formats
    - _Requirements: 3.4, 11.4_

- [ ] 8. Implement display controller for HyperPixel 4.0
  - [ ] 8.1 Create framebuffer display interface
    - Implement framebuffer file operations
    - Add backlight control through sysfs
    - Create frame format conversion for display
    - _Requirements: 4.2, 4.3, 4.5, 12.4_

  - [ ] 8.2 Add event-driven display activation
    - Connect display to motion and touch events
    - Implement auto-deactivation timer
    - Add display state management
    - _Requirements: 4.1_

- [ ] 9. Implement touch input handling
  - [ ] 9.1 Create evdev touch input handler
    - Implement touch device monitoring
    - Parse touch events and publish to event bus
    - Add touch input error handling and recovery
    - _Requirements: 4.4_

- [ ] 10. Implement video capture system
  - [ ] 10.1 Create motion-triggered recording
    - Implement event-driven capture start/stop
    - Add preroll frame retrieval from ring buffer
    - Implement postroll recording duration
    - _Requirements: 2.1, 2.2, 2.3_

  - [ ] 10.2 Add frame processing and storage
    - Implement JPEG frame saving to timestamped directories
    - Add frame rotation during post-processing
    - Create video encoding with hardware acceleration
    - _Requirements: 2.4, 11.2, 11.3, 12.2_

- [ ] 11. Implement event storage and cleanup
  - [ ] 11.1 Create file system event management
    - Implement timestamped directory creation
    - Add event metadata tracking
    - Create file organization utilities
    - _Requirements: 10.4_

  - [ ] 11.2 Add automatic cleanup system
    - Implement scheduled cleanup tasks
    - Add retention period enforcement
    - Create safe deletion with timestamp validation
    - _Requirements: 10.1, 10.2, 10.3, 10.5_

- [ ] 12. Implement error handling and recovery
  - [ ] 12.1 Create comprehensive error types
    - Define error enums for all components
    - Implement error conversion and propagation
    - Add structured error logging
    - _Requirements: 7.1, 7.5_

  - [ ] 12.2 Add component recovery strategies
    - Implement camera reconnection with backoff
    - Add graceful degradation for component failures
    - Create system health monitoring
    - _Requirements: 7.2, 7.3_

- [ ] 13. Create application orchestration and CLI
  - [ ] 13.1 Implement main application coordinator
    - Create component lifecycle management
    - Add graceful shutdown handling
    - Implement signal handling for systemd
    - _Requirements: 9.1, 9.4_

  - [ ] 13.2 Add command-line interface
    - Implement CLI argument parsing with clap
    - Add configuration file path option
    - Create debug mode and logging controls
    - _Requirements: 9.3_

- [ ] 14. Add deployment and service integration
  - [ ] 14.1 Create systemd service configuration
    - Write systemd service file template
    - Add proper service dependencies and restart policies
    - Configure logging integration
    - _Requirements: 9.2, 9.5_

  - [ ] 14.2 Create build and installation scripts
    - Add cross-compilation support for ARM
    - Create installation and setup scripts
    - Add dependency documentation
    - _Requirements: 12.5_