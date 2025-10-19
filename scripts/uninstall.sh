#!/bin/bash
# Doorcam Uninstallation Script
# Removes doorcam binary, configuration, and systemd service

set -euo pipefail

# Configuration
PROJECT_NAME="doorcam"
INSTALL_PREFIX="/usr/local"
CONFIG_DIR="/etc/doorcam"
DATA_DIR="/var/lib/doorcam"
LOG_DIR="/var/log/doorcam"
SERVICE_USER="doorcam"

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
Doorcam Uninstallation Script

Usage: $0 [OPTIONS]

OPTIONS:
    -h, --help              Show this help message
    -p, --prefix PATH       Installation prefix (default: /usr/local)
    -u, --user USER         Service user (default: doorcam)
    --keep-config          Keep configuration files
    --keep-data            Keep data directory and files
    --keep-user            Keep service user account
    -d, --dry-run          Show what would be done without executing
    -f, --force            Force removal without confirmation

EXAMPLES:
    $0                      # Standard uninstallation
    $0 --keep-config       # Remove but keep configuration
    $0 --keep-data         # Remove but keep data files
    $0 --dry-run           # Preview uninstallation steps

EOF
}

# Default values
KEEP_CONFIG=false
KEEP_DATA=false
KEEP_USER=false
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
        --keep-config)
            KEEP_CONFIG=true
            shift
            ;;
        --keep-data)
            KEEP_DATA=true
            shift
            ;;
        --keep-user)
            KEEP_USER=true
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
fi

log_info "Uninstalling $PROJECT_NAME"

# Function to execute commands with dry-run support
execute() {
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[DRY-RUN] $*"
    else
        "$@"
    fi
}

# Confirmation prompt
confirm_uninstall() {
    if [[ "$FORCE" == "true" ]] || [[ "$DRY_RUN" == "true" ]]; then
        return 0
    fi
    
    echo
    log_warning "This will remove the following:"
    echo "  - Binary: $INSTALL_PREFIX/bin/$PROJECT_NAME"
    echo "  - Systemd service: /etc/systemd/system/doorcam.service"
    
    if [[ "$KEEP_CONFIG" == "false" ]]; then
        echo "  - Configuration: $CONFIG_DIR"
    fi
    
    if [[ "$KEEP_DATA" == "false" ]]; then
        echo "  - Data directory: $DATA_DIR"
        echo "  - Log directory: $LOG_DIR"
    fi
    
    if [[ "$KEEP_USER" == "false" ]]; then
        echo "  - Service user: $SERVICE_USER"
    fi
    
    echo
    read -p "Are you sure you want to continue? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        log_info "Uninstallation cancelled"
        exit 0
    fi
}

# Stop and disable service
stop_service() {
    log_info "Stopping and disabling systemd service..."
    
    if systemctl is-active --quiet doorcam.service 2>/dev/null; then
        execute systemctl stop doorcam.service
        log_info "Service stopped"
    fi
    
    if systemctl is-enabled --quiet doorcam.service 2>/dev/null; then
        execute systemctl disable doorcam.service
        log_info "Service disabled"
    fi
}

# Remove systemd service
remove_service() {
    log_info "Removing systemd service..."
    
    local service_file="/etc/systemd/system/doorcam.service"
    
    if [[ -f "$service_file" ]]; then
        execute rm -f "$service_file"
        execute systemctl daemon-reload
        log_success "Systemd service removed"
    else
        log_info "Systemd service file not found"
    fi
}

# Remove binary
remove_binary() {
    log_info "Removing binary..."
    
    local binary_path="$INSTALL_PREFIX/bin/$PROJECT_NAME"
    
    if [[ -f "$binary_path" ]]; then
        execute rm -f "$binary_path"
        log_success "Binary removed: $binary_path"
    else
        log_info "Binary not found at $binary_path"
    fi
}

# Remove configuration
remove_config() {
    if [[ "$KEEP_CONFIG" == "true" ]]; then
        log_info "Keeping configuration files (--keep-config specified)"
        return
    fi
    
    log_info "Removing configuration..."
    
    if [[ -d "$CONFIG_DIR" ]]; then
        execute rm -rf "$CONFIG_DIR"
        log_success "Configuration removed: $CONFIG_DIR"
    else
        log_info "Configuration directory not found"
    fi
}

# Remove data and logs
remove_data() {
    if [[ "$KEEP_DATA" == "true" ]]; then
        log_info "Keeping data files (--keep-data specified)"
        return
    fi
    
    log_info "Removing data and log directories..."
    
    if [[ -d "$DATA_DIR" ]]; then
        execute rm -rf "$DATA_DIR"
        log_success "Data directory removed: $DATA_DIR"
    else
        log_info "Data directory not found"
    fi
    
    if [[ -d "$LOG_DIR" ]]; then
        execute rm -rf "$LOG_DIR"
        log_success "Log directory removed: $LOG_DIR"
    else
        log_info "Log directory not found"
    fi
}

# Remove user
remove_user() {
    if [[ "$KEEP_USER" == "true" ]]; then
        log_info "Keeping service user (--keep-user specified)"
        return
    fi
    
    log_info "Removing service user..."
    
    if id "$SERVICE_USER" >/dev/null 2>&1; then
        execute userdel "$SERVICE_USER"
        log_success "Service user removed: $SERVICE_USER"
    else
        log_info "Service user not found: $SERVICE_USER"
    fi
}

# Verify removal
verify_removal() {
    log_info "Verifying removal..."
    
    local issues=0
    
    # Check binary
    if [[ -f "$INSTALL_PREFIX/bin/$PROJECT_NAME" ]]; then
        log_warning "Binary still exists: $INSTALL_PREFIX/bin/$PROJECT_NAME"
        ((issues++))
    fi
    
    # Check service
    if [[ -f "/etc/systemd/system/doorcam.service" ]]; then
        log_warning "Service file still exists: /etc/systemd/system/doorcam.service"
        ((issues++))
    fi
    
    # Check config (only if not keeping)
    if [[ "$KEEP_CONFIG" == "false" ]] && [[ -d "$CONFIG_DIR" ]]; then
        log_warning "Configuration directory still exists: $CONFIG_DIR"
        ((issues++))
    fi
    
    # Check data (only if not keeping)
    if [[ "$KEEP_DATA" == "false" ]] && [[ -d "$DATA_DIR" ]]; then
        log_warning "Data directory still exists: $DATA_DIR"
        ((issues++))
    fi
    
    # Check user (only if not keeping)
    if [[ "$KEEP_USER" == "false" ]] && id "$SERVICE_USER" >/dev/null 2>&1; then
        log_warning "Service user still exists: $SERVICE_USER"
        ((issues++))
    fi
    
    if [[ $issues -eq 0 ]]; then
        log_success "Uninstallation completed successfully!"
    else
        log_warning "Uninstallation completed with $issues issues"
    fi
}

# Main uninstallation process
main() {
    log_info "Starting uninstallation process..."
    
    confirm_uninstall
    stop_service
    remove_service
    remove_binary
    remove_config
    remove_data
    remove_user
    verify_removal
    
    if [[ "$DRY_RUN" == "false" ]]; then
        echo
        log_info "Uninstallation completed!"
        
        if [[ "$KEEP_CONFIG" == "true" ]] || [[ "$KEEP_DATA" == "true" ]] || [[ "$KEEP_USER" == "true" ]]; then
            log_info "Some components were preserved as requested"
        fi
    fi
}

# Run main function
main