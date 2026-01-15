#!/usr/bin/env bash
#
# Setup script for install.tool.store domain.
# Configures Caddy to serve an install script for tool-cli.
# This script is idempotent - safe to run multiple times.
#
# Prerequisites:
#   - SSH access to server with sudo privileges
#   - Caddy already installed
#   - DNS A/AAAA record for install.tool.store pointing to this server
#
# Quick Start:
#   # 1. Copy both files to your server
#   scp install.sh scripts/setup-install-domain.sh user@server:~/
#
#   # 2. Run the setup (install.sh must be in same directory or specify path)
#   sudo ./setup-install-domain.sh
#
#   # 3. Verify it works
#   curl -sSfL https://install.tool.store | sh
#
# Options:
#   --domain=NAME       Use a different domain (default: install.tool.store)
#   --install-sh=PATH   Path to install.sh (default: ./install.sh)
#   --skip-caddy        Skip Caddy configuration (only deploy install script)
#   --dry-run           Show what would be done without making changes
#   --help              Show this help message
#
# The script will:
#   - Copy install.sh to /var/www/install.tool.store/install.sh
#   - Add a server block to /etc/caddy/Caddyfile
#   - Reload Caddy to apply changes
#

set -euo pipefail

#--------------------------------------------------------------------------------------------------
# Constants
#--------------------------------------------------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Defaults (can be overridden via flags)
DOMAIN="install.tool.store"
INSTALL_SH="${SCRIPT_DIR}/../install.sh"
WEB_ROOT="/var/www"
CADDYFILE="/etc/caddy/Caddyfile"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

#--------------------------------------------------------------------------------------------------
# Functions: Logging
#--------------------------------------------------------------------------------------------------

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}/

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

#--------------------------------------------------------------------------------------------------
# Functions: Helpers
#--------------------------------------------------------------------------------------------------

check_root() {
    if [[ $EUID -ne 0 ]]; then
        log_error "This script must be run as root (use sudo)"
        exit 1
    fi
}

show_help() {
    sed -n '2,33p' "$0" | sed 's/^#//' | sed 's/^ //'
    exit 0
}

command_exists() {
    command -v "$1" &> /dev/null
}

#--------------------------------------------------------------------------------------------------
# Functions: Setup Steps
#--------------------------------------------------------------------------------------------------

deploy_install_script() {
    local install_dir="$WEB_ROOT/$DOMAIN"
    local install_script="$install_dir/install.sh"

    log_step "Deploying install script..."

    # Resolve install.sh path
    if [[ ! -f "$INSTALL_SH" ]]; then
        # Try current directory as fallback
        if [[ -f "./install.sh" ]]; then
            INSTALL_SH="./install.sh"
        else
            log_error "install.sh not found at: $INSTALL_SH"
            log_error "Specify path with --install-sh=PATH or place install.sh in current directory"
            exit 1
        fi
    fi

    log_info "  Source: $INSTALL_SH"

    if [[ "$DRY_RUN" == "true" ]]; then
        log_info "  Would copy to: $install_script"
        return 0
    fi

    mkdir -p "$install_dir"
    cp "$INSTALL_SH" "$install_script"
    chmod 644 "$install_script"

    log_info "  Deployed: $install_script"
}

configure_caddy() {
    log_step "Configuring Caddy..."

    if [[ "$DRY_RUN" == "true" ]]; then
        log_info "  Would add server block for $DOMAIN to $CADDYFILE"
        return 0
    fi

    if ! command_exists caddy; then
        log_error "Caddy is not installed. Install it first or use --skip-caddy"
        exit 1
    fi

    if [[ ! -f "$CADDYFILE" ]]; then
        log_error "Caddyfile not found at $CADDYFILE"
        exit 1
    fi

    # Check if domain already configured
    if grep -q "^${DOMAIN}\s*{" "$CADDYFILE" || grep -q "^${DOMAIN}{" "$CADDYFILE"; then
        log_info "  $DOMAIN already configured in Caddyfile"
        return 0
    fi

    # Add server block
    log_info "  Adding $DOMAIN server block..."

    cat >> "$CADDYFILE" << EOF

${DOMAIN} {
    root * ${WEB_ROOT}/${DOMAIN}
    rewrite / /install.sh
    header Content-Type "text/plain; charset=utf-8"
    file_server
}
EOF

    log_info "  Added server block for $DOMAIN"
}

validate_and_reload_caddy() {
    if [[ "$SKIP_CADDY" == "true" ]] || [[ "$DRY_RUN" == "true" ]]; then
        return 0
    fi

    log_step "Validating and reloading Caddy..."

    if ! caddy validate --config "$CADDYFILE" --adapter caddyfile >/dev/null 2>&1; then
        log_error "Caddy configuration is invalid. Check $CADDYFILE"
        caddy validate --config "$CADDYFILE" --adapter caddyfile
        exit 1
    fi

    log_info "  Configuration valid"

    if systemctl is-active caddy >/dev/null 2>&1; then
        systemctl reload caddy
        log_info "  Reloaded Caddy"
    else
        log_warn "  Caddy is not running. Start it with: sudo systemctl start caddy"
    fi
}

show_summary() {
    echo ""
    log_info "=========================================="
    log_info "  Setup Complete!"
    log_info "=========================================="
    echo ""

    if [[ "$DRY_RUN" == "true" ]]; then
        log_warn "This was a dry run. No changes were made."
        echo ""
    fi

    echo "Install script location:"
    echo "  ${WEB_ROOT}/${DOMAIN}/install.sh"
    echo ""
    echo "Users can install tool-cli with:"
    echo "  curl -sSfL https://${DOMAIN} | sh"
    echo ""
    echo "Or with wget:"
    echo "  wget -qO- https://${DOMAIN} | sh"
    echo ""

    if [[ "$SKIP_CADDY" == "false" ]]; then
        echo "Caddy commands:"
        echo "  View logs:      journalctl -u caddy -f"
        echo "  Reload config:  sudo systemctl reload caddy"
        echo "  Validate:       caddy validate --config $CADDYFILE"
        echo ""
    fi

    echo "To test locally (before DNS propagation):"
    echo "  curl -sSfL http://localhost/install.sh | sh"
    echo ""
}

#--------------------------------------------------------------------------------------------------
# Main
#--------------------------------------------------------------------------------------------------

main() {
    # Flags
    SKIP_CADDY=false
    DRY_RUN=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --domain=*)
                DOMAIN="${1#*=}"
                shift
                ;;
            --install-sh=*)
                INSTALL_SH="${1#*=}"
                shift
                ;;
            --skip-caddy)
                SKIP_CADDY=true
                shift
                ;;
            --dry-run)
                DRY_RUN=true
                shift
                ;;
            --help|-h)
                show_help
                ;;
            *)
                log_error "Unknown option: $1"
                show_help
                ;;
        esac
    done

    echo ""
    echo "========================================"
    echo "  Install Domain Setup Script"
    echo "========================================"
    echo ""
    echo "  Domain:     $DOMAIN"
    echo "  install.sh: $INSTALL_SH"
    echo ""

    if [[ "$DRY_RUN" == "false" ]]; then
        check_root
    fi

    deploy_install_script

    if [[ "$SKIP_CADDY" == "false" ]]; then
        configure_caddy
        validate_and_reload_caddy
    fi

    show_summary
}

main "$@"
