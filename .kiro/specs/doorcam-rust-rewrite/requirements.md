# Requirements Document

## Introduction

This document specifies the requirements for rewriting the existing Python-based doorcam application in Rust. The doorcam system is a Raspberry Pi peephole camera that provides motion detection, video capture, live streaming, and display functionality for door monitoring applications.

## Glossary

- **Doorcam_System**: The complete Rust-based door camera application
- **Motion_Analyzer**: Component responsible for detecting motion in camera frames
- **Video_Capture**: Component that records video when motion is detected
- **Stream_Server**: HTTP/MJPEG streaming server for remote viewing
- **Display_Controller**: Component managing the physical screen and touch interface
- **Camera_Interface**: Component handling video device capture and configuration
- **Configuration_Manager**: Component managing application settings from multiple sources
- **Event_Storage**: System for storing and managing captured video events
- **Frame_Buffer**: Circular buffer system for maintaining preroll frame history

## Requirements

### Requirement 1

**User Story:** As a homeowner, I want the system to automatically detect motion at my door, so that I can be alerted to visitors or activity.

#### Acceptance Criteria

1. WHEN motion exceeds the configured contour area threshold, THE Motion_Analyzer SHALL trigger motion detection callbacks
2. WHILE analyzing frames, THE Motion_Analyzer SHALL process frames at the configured maximum FPS rate
3. THE Motion_Analyzer SHALL process frames from the Frame_Buffer for motion detection
4. THE Motion_Analyzer SHALL use background subtraction with configurable delta threshold and contour analysis with minimum area filtering for motion detection
5. WHEN motion is detected, THE Motion_Analyzer SHALL log the detection event with contour area information

### Requirement 2

**User Story:** As a homeowner, I want the system to capture video when motion is detected, so that I have a record of door activity.

#### Acceptance Criteria

1. THE Frame_Buffer SHALL continuously maintain a circular buffer of frames for the configured preroll duration
2. WHEN motion is detected, THE Video_Capture SHALL begin recording using frames from the Frame_Buffer for preroll
3. WHILE recording, THE Video_Capture SHALL continue capture for the configured postroll duration after last motion
4. THE Video_Capture SHALL save captured frames as JPEG images in timestamped directories

### Requirement 3

**User Story:** As a homeowner, I want to view the camera feed remotely, so that I can monitor my door from anywhere.

#### Acceptance Criteria

1. THE Stream_Server SHALL serve MJPEG video stream over HTTP at the configured IP and port
2. WHEN a client requests the stream endpoint, THE Stream_Server SHALL provide continuous MJPEG frames
3. THE Stream_Server SHALL handle multiple concurrent client connections
4. THE Stream_Server SHALL serve frames from the Frame_Buffer at the configured frame rate
5. WHEN streaming errors occur, THE Stream_Server SHALL log errors and gracefully handle client disconnections

### Requirement 4

**User Story:** As a homeowner, I want a local display showing the camera view, so that I can see visitors without opening the door.

#### Acceptance criteria

1. WHEN motion is detected or screen is touched, THE Display_Controller SHALL activate the screen for the configured duration
2. THE Display_Controller SHALL render frames from the Frame_Buffer to the DPI-connected HyperPixel display via framebuffer or DRM
3. THE Display_Controller SHALL control backlight power through the configured backlight device
4. THE Display_Controller SHALL process touch input from the configured input device
5. WHILE displaying, THE Display_Controller SHALL apply rotation and color conversion optimized for the HyperPixel 4.0 DPI interface

### Requirement 5

**User Story:** As a system administrator, I want configurable camera settings, so that I can optimize the system for different hardware and environments.

#### Acceptance Criteria

1. THE Configuration_Manager SHALL load settings from TOML configuration files with environment variable overrides
2. THE Configuration_Manager SHALL provide default values for all configuration parameters using Rust derive macros
3. THE Configuration_Manager SHALL validate configuration parameters at startup using type-safe deserialization
4. THE Camera_Interface SHALL apply resolution, FPS, and format settings from configuration
5. THE project SHALL include an example TOML configuration file with documented default values and comments

### Requirement 6

**User Story:** As a developer, I want comprehensive error handling and logging, so that I can diagnose and troubleshoot system issues.

#### Acceptance Criteria

1. THE Doorcam_System SHALL log all significant events and errors with appropriate severity levels
2. THE Doorcam_System SHALL continue operation when non-critical components fail
3. WHEN camera access fails, THE Doorcam_System SHALL attempt reconnection with exponential backoff
4. THE Doorcam_System SHALL provide structured logging output compatible with systemd journal
5. WHEN configuration errors occur, THE Doorcam_System SHALL report specific validation failures

### Requirement 7

**User Story:** As a system administrator, I want efficient resource usage, so that the system runs reliably on Raspberry Pi hardware.

#### Acceptance Criteria

1. THE Doorcam_System SHALL use async/await patterns for concurrent operations
2. THE Frame_Buffer SHALL implement a lock-free circular buffer to minimize memory allocations and contention
3. THE Doorcam_System SHALL implement zero-copy operations where possible for frame data
4. THE Frame_Buffer SHALL automatically manage memory by overwriting oldest frames when buffer capacity is reached
5. THE Doorcam_System SHALL prioritize hardware-accelerated operations over software implementations for optimal Pi performance

### Requirement 8

**User Story:** As a homeowner, I want to see what happened before motion was detected, so that I have complete context of door events.

#### Acceptance Criteria

1. THE Frame_Buffer SHALL maintain a circular buffer sized to hold frames for the configured preroll duration (calculated as preroll_seconds × camera_fps)
2. THE Frame_Buffer SHALL provide thread-safe access for concurrent read and write operations
3. WHEN motion is detected, THE Frame_Buffer SHALL provide all frames captured within the preroll time window preceding the motion event
4. THE Frame_Buffer SHALL timestamp each frame for accurate preroll duration calculation
5. THE Frame_Buffer SHALL handle frame rate variations without losing temporal accuracy

### Requirement 9

**User Story:** As a system administrator, I want the system to run as a systemd service, so that it starts automatically and can be managed like other system services.

#### Acceptance Criteria

1. THE Doorcam_System SHALL handle SIGTERM and SIGINT signals for graceful shutdown
2. THE Doorcam_System SHALL use structured logging that integrates well with systemd journal
3. THE Doorcam_System SHALL support command-line arguments for configuration file path and debug mode
4. THE Doorcam_System SHALL exit with appropriate status codes for systemd service management
5. THE project SHALL include a systemd service file template for easy deployment

### Requirement 10

**User Story:** As a homeowner, I want automatic cleanup of old recordings, so that storage space is managed efficiently.

#### Acceptance Criteria

1. WHERE trim_old is enabled, THE Event_Storage SHALL automatically delete events older than the configured limit
2. THE Event_Storage SHALL run cleanup operations on a scheduled interval
3. THE Event_Storage SHALL preserve events within the configured retention period
4. THE Event_Storage SHALL organize captured events in timestamped directory structures
5. WHEN cleanup fails, THE Event_Storage SHALL log errors without affecting ongoing capture

### Requirement 11

**User Story:** As a homeowner, I want frame rotation support, so that the camera can be mounted in different orientations.

#### Acceptance Criteria

1. THE Camera_Interface SHALL support configurable frame rotation (90°, 180°, 270°)
2. THE Display_Controller SHALL apply rotation independently from camera rotation for proper screen orientation
3. THE Video_Capture SHALL apply rotation during post-processing for saved images and videos
4. THE Stream_Server SHALL serve frames with camera rotation applied
5. THE Doorcam_System SHALL validate rotation values and provide clear error messages for invalid configurations

### Requirement 12

**User Story:** As a system administrator, I want hardware acceleration utilized, so that the system performs efficiently on Raspberry Pi hardware.

#### Acceptance Criteria

1. THE Camera_Interface SHALL utilize V4L2 hardware-accelerated video capture when available
2. THE Video_Capture SHALL use hardware-accelerated H.264 encoding (h264_v4l2m2m) for video generation when available
3. THE Motion_Analyzer SHALL leverage GPU acceleration for image processing operations when OpenCV GPU support is available
4. THE Display_Controller SHALL use optimized rendering for DPI displays (framebuffer with hardware acceleration where available)
5. THE Doorcam_System SHALL gracefully fall back to software implementations when hardware acceleration is unavailable