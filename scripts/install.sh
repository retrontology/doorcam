#!/bin/bash
# Doorcam Installation Script
# Installs doorcam binary, configuration, and systemd service

set -euo pipefail

# Configuration
PROJECT_NAME="doorcam"
INSTALL_PREFIX="/usr/local"
CONFIG_DIR="/etc/doorcam"
DATA_DIR="/var/lib/doorcam"
LOG_DIR="/var/log/doorcam"
SERVICE_USER="doorcam"
SERVICE_GROUP="video"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Help function
show_help() {
    cat << EOF
Doorcam Installation Script

Usage: $0 [OPTIONS]

OPTIONS:
    -h, --help          Show this help message
    -p, --prefix PATH   Installation prefix (default: /usr/local)
    -u, --user USER     Service user (default: doorcam)
    -g, --group GROUP   Service group (default: video)
    -s, --skip-service  Skip systemd service installation
    -d, --dry-run       Show what would be done without executing
    -f, --force         Force installation (overwrite existing files)

EXAMPLES:
    $0                          # Standard installation
    $0 --prefix /opt/doorcam   # Install to /opt/doorcam
    $0 --skip-service          # Install without systemd service
    $0 --dry-run               # Preview installation steps

EOF
}

# Default values
SKIP_SERVICE=false
DRY_RUN=false
FORCE=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -p|--prefix)
            INSTALL_PREFIX="$2"
            shift 2
            ;;
        -u|--user)
            SERVICE_USER="$2"
            shift 2
            ;;
        -g|--group)
            SERVICE_GROUP="$2"
            shift 2
            ;;
        -s|--skip-service)
            SKIP_SERVICE=true
            shift
            ;;
        -d|--dry-run)
            DRY_RUN=true
            shift
            ;;
        -f|--force)
            FORCE=true
            shift
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Check if running as root
if [[ $EUID -ne 0 ]] && [[ "$DRY_RUN" == "false" ]]; then
    log_error "This script must be run as root (use sudo)"
    exit 1
fi

# Dry run prefix
if [[ "$DRY_RUN" == "true" ]]; then
    log_warning "DRY RUN MODE - No changes will be made"
    EXEC_PREFIX="echo [DRY-RUN]"
else
    EXEC_PREFIX=""
fi

log_info "Installing $PROJECT_NAME"
log_info "Installation prefix: $INSTALL_PREFIX"
log_info "Configuration directory: $CONFIG_DIR"
log_info "Data directory: $DATA_DIR"
log_info "Service user: $SERVICE_USER"

# Check if binary exists
if [[ ! -f "$PROJECT_NAME" ]]; then
    log_error "Binary '$PROJECT_NAME' not found in current directory"
    log_info "Make sure you're running this script from the extracted package directory"
    exit 1
fi

# Function to execute commands with dry-run support
execute() {
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[DRY-RUN] $*"
    else
        "$@"
    fi
}

# Create system user and group
create_user() {
    log_info "Creating system user and group..."
    
    # Check if group exists
    if ! getent group "$SERVICE_GROUP" >/dev/null 2>&1; then
        execute groupadd --system "$SERVICE_GROUP"
        log_info "Created group: $SERVICE_GROUP"
    else
        log_info "Group $SERVICE_GROUP already exists"
    fi
    
    # Check if user exists
    if ! id "$SERVICE_USER" >/dev/null 2>&1; then
        execute useradd --system \
            --home-dir "$DATA_DIR" \
            --create-home \
            --shell /bin/false \
            --gid "$SERVICE_GROUP" \
            "$SERVICE_USER"
        log_info "Created user: $SERVICE_USER"
    else
        log_info "User $SERVICE_USER already exists"
    fi
    
    # Add user to required groups
    execute usermod -a -G video,input "$SERVICE_USER"
    log_info "Added $SERVICE_USER to video and input groups"
}

# Create directories
create_directories() {
    log_info "Creating directories..."
    
    execute mkdir -p "$INSTALL_PREFIX/bin"
    execute mkdir -p "$CONFIG_DIR"
    execute mkdir -p "$DATA_DIR"
    execute mkdir -p "$LOG_DIR"
    
    # Set ownership and permissions
    execute chown root:root "$INSTALL_PREFIX/bin"
    execute chown root:root "$CONFIG_DIR"
    execute chown "$SERVICE_USER:$SERVICE_GROUP" "$DATA_DIR"
    execute chown "$SERVICE_USER:$SERVICE_GROUP" "$LOG_DIR"
    
    execute chmod 755 "$INSTALL_PREFIX/bin"
    execute chmod 755 "$CONFIG_DIR"
    execute chmod 755 "$DATA_DIR"
    execute chmod 755 "$LOG_DIR"
    
    log_success "Directories created and configured"
}

# Install binary
install_binary() {
    log_info "Installing binary..."
    
    local target_path="$INSTALL_PREFIX/bin/$PROJECT_NAME"
    
    if [[ -f "$target_path" ]] && [[ "$FORCE" == "false" ]]; then
        log_warning "Binary already exists at $target_path"
        read -p "Overwrite? (y/N): " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "Skipping binary installation"
            return
        fi
    fi
    
    execute cp "$PROJECT_NAME" "$target_path"
    execute chmod 755 "$target_path"
    execute chown root:root "$target_path"
    
    log_success "Binary installed to $target_path"
}

# Install configuration
install_config() {
    log_info "Installing configuration..."
    
    local config_file="$CONFIG_DIR/doorcam.toml"
    
    if [[ -f "doorcam.toml.example" ]]; then
        if [[ -f "$config_file" ]] && [[ "$FORCE" == "false" ]]; then
            log_warning "Configuration file already exists at $config_file"
            log_info "Backing up existing configuration to $config_file.backup"
            execute cp "$config_file" "$config_file.backup"
        fi
        
        execute cp "doorcam.toml.example" "$config_file"
        execute chown root:"$SERVICE_GROUP" "$config_file"
        execute chmod 640 "$config_file"
        
        log_success "Configuration installed to $config_file"
        log_warning "Please review and customize the configuration file"
    else
        log_warning "No configuration template found (doorcam.toml.example)"
    fi
}

# Install systemd service
install_service() {
    if [[ "$SKIP_SERVICE" == "true" ]]; then
        log_info "Skipping systemd service installation"
        return
    fi
    
    log_info "Installing systemd service..."
    
    if [[ ! -d "systemd" ]]; then
        log_warning "No systemd directory found, skipping service installation"
        return
    fi
    
    local service_file="/etc/systemd/system/doorcam.service"
    
    if [[ -f "systemd/doorcam.service.template" ]]; then
        # Customize service file with installation paths
        execute sed \
            -e "s|/usr/local/bin/doorcam|$INSTALL_PREFIX/bin/doorcam|g" \
            -e "s|User=doorcam|User=$SERVICE_USER|g" \
            -e "s|Group=video|Group=$SERVICE_GROUP|g" \
            -e "s|/var/lib/doorcam|$DATA_DIR|g" \
            -e "s|/var/log/doorcam|$LOG_DIR|g" \
            -e "s|/etc/doorcam|$CONFIG_DIR|g" \
            "systemd/doorcam.service.template" > "/tmp/doorcam.service"
        
        execute cp "/tmp/doorcam.service" "$service_file"
        execute rm -f "/tmp/doorcam.service"
        execute chmod 644 "$service_file"
        execute chown root:root "$service_file"
        
        # Reload systemd
        execute systemctl daemon-reload
        
        log_success "Systemd service installed"
        log_info "To enable and start the service:"
        log_info "  sudo systemctl enable doorcam.service"
        log_info "  sudo systemctl start doorcam.service"
    else
        log_warning "No systemd service template found"
    fi
}

# Verify installation
verify_installation() {
    log_info "Verifying installation..."
    
    local binary_path="$INSTALL_PREFIX/bin/$PROJECT_NAME"
    
    if [[ -f "$binary_path" ]]; then
        log_success "Binary installed: $binary_path"
        if [[ "$DRY_RUN" == "false" ]]; then
            local version=$("$binary_path" --version 2>/dev/null || echo "unknown")
            log_info "Version: $version"
        fi
    else
        log_error "Binary not found at $binary_path"
    fi
    
    if [[ -f "$CONFIG_DIR/doorcam.toml" ]]; then
        log_success "Configuration installed: $CONFIG_DIR/doorcam.toml"
    else
        log_warning "Configuration not found"
    fi
    
    if [[ -f "/etc/systemd/system/doorcam.service" ]] && [[ "$SKIP_SERVICE" == "false" ]]; then
        log_success "Systemd service installed"
    fi
    
    if id "$SERVICE_USER" >/dev/null 2>&1; then
        log_success "Service user created: $SERVICE_USER"
    fi
}

# Main installation process
main() {
    log_info "Starting installation process..."
    
    create_user
    create_directories
    install_binary
    install_config
    install_service
    verify_installation
    
    log_success "Installation completed successfully!"
    
    if [[ "$DRY_RUN" == "false" ]]; then
        echo
        log_info "Next steps:"
        log_info "1. Review configuration: $CONFIG_DIR/doorcam.toml"
        log_info "2. Enable service: sudo systemctl enable doorcam.service"
        log_info "3. Start service: sudo systemctl start doorcam.service"
        log_info "4. Check status: sudo systemctl status doorcam.service"
        log_info "5. View logs: sudo journalctl -u doorcam.service -f"
    fi
}

# Run main function
main