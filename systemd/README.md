# Systemd Service Configuration

This directory contains systemd service configuration files for the Doorcam Rust application.

## Files

- `doorcam.service.template` - Main systemd service template
- `doorcam-dev.service` - Development service configuration (optional)
- `README.md` - This documentation file

## Installation

### 1. Create System User

Create a dedicated user for running the doorcam service:

```bash
sudo useradd --system --home-dir /var/lib/doorcam --create-home --shell /bin/false doorcam
sudo usermod -a -G video,input doorcam
```

### 2. Create Directories

```bash
sudo mkdir -p /etc/doorcam
sudo mkdir -p /var/lib/doorcam
sudo mkdir -p /var/log/doorcam
sudo chown doorcam:doorcam /var/lib/doorcam /var/log/doorcam
sudo chmod 755 /var/lib/doorcam /var/log/doorcam
```

### 3. Install Configuration

Copy your configuration file:

```bash
sudo cp doorcam.toml /etc/doorcam/
sudo chown root:doorcam /etc/doorcam/doorcam.toml
sudo chmod 640 /etc/doorcam/doorcam.toml
```

### 4. Install Service

```bash
sudo cp doorcam.service.template /etc/systemd/system/doorcam.service
sudo systemctl daemon-reload
sudo systemctl enable doorcam.service
```

### 5. Start Service

```bash
sudo systemctl start doorcam.service
```

## Service Management

### Check Status
```bash
sudo systemctl status doorcam.service
```

### View Logs
```bash
# Recent logs
sudo journalctl -u doorcam.service

# Follow logs in real-time
sudo journalctl -u doorcam.service -f

# Logs since boot
sudo journalctl -u doorcam.service -b
```

### Restart Service
```bash
sudo systemctl restart doorcam.service
```

### Stop Service
```bash
sudo systemctl stop doorcam.service
```

### Disable Service
```bash
sudo systemctl disable doorcam.service
```

## Configuration Notes

### Security Features

The service template includes several security hardening features:

- **User Isolation**: Runs as dedicated `doorcam` user
- **File System Protection**: Limited read/write access to specific directories
- **Device Access**: Only allows access to required video, framebuffer, and input devices
- **Network Restrictions**: Limited to necessary address families
- **Resource Limits**: Configurable CPU and memory limits

### Device Permissions

The service requires access to:
- `/dev/video*` - Camera devices (read/write)
- `/dev/fb*` - Framebuffer devices for display (read/write)
- `/dev/input*` - Touch input devices (read)
- `/dev/dri*` - DRM devices for hardware acceleration (read/write)

### Environment Variables

Key environment variables:
- `RUST_LOG` - Controls logging level (debug, info, warn, error)
- `DOORCAM_CONFIG_PATH` - Path to configuration file
- `DOORCAM_DATA_PATH` - Path to data storage directory
- `RUST_BACKTRACE` - Enable backtraces for debugging

### Logging Integration

The service is configured for optimal systemd journal integration:
- Structured logging output
- Proper syslog identifiers
- Configurable log levels
- Automatic log rotation via systemd

## Troubleshooting

### Service Won't Start

1. Check service status: `sudo systemctl status doorcam.service`
2. Check logs: `sudo journalctl -u doorcam.service`
3. Verify binary exists and is executable
4. Check configuration file syntax
5. Verify user permissions

### Permission Errors

1. Verify user is in `video` and `input` groups
2. Check device file permissions
3. Verify directory ownership and permissions
4. Check SELinux/AppArmor policies if applicable

### Camera Access Issues

1. Verify camera device exists: `ls -la /dev/video*`
2. Check if camera is in use by another process
3. Verify user has access to video devices
4. Test camera manually: `v4l2-ctl --list-devices`

### Display Issues

1. Check framebuffer device: `ls -la /dev/fb*`
2. Verify display configuration in doorcam.toml
3. Check if display is already in use
4. Test framebuffer access: `cat /dev/urandom > /dev/fb0` (as test user)

## Development Service

For development, you can create a separate service file with different settings:

```bash
sudo cp doorcam.service.template /etc/systemd/system/doorcam-dev.service
```

Modify the development service to:
- Use different user (your development user)
- Point to development binary location
- Use development configuration
- Enable debug logging (`RUST_LOG=debug`)
- Disable some security restrictions for easier debugging