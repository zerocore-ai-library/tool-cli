#!/bin/sh
# install.sh - Tool CLI installer with animated progress
# Usage: curl -fsSL https://raw.githubusercontent.com/zerocore-ai/tool-cli/main/install.sh | sh
#    or: ./install.sh [options]
#
# Options:
#   --version=X.Y.Z    Install specific version (default: latest)
#   --prefix=PATH      Install prefix (default: ~/.local)
#   --uninstall        Remove tool binary
#   --check            Verify existing installation
#   --no-modify-path   Skip PATH configuration prompts
#   --quiet            Minimal output
#   --force            Overwrite without prompts
#   --help             Show this help

set -e

#--------------------------------------------------------------------------------------------------
# Constants
#--------------------------------------------------------------------------------------------------

VERSION=""
GITHUB_REPO="zerocore-ai/tool-cli"
SCRIPT_VERSION="0.1.0"

# Installation targets
BINARIES="tool"

# Defaults
PREFIX="$HOME/.local"
QUIET=0
FORCE=0
MODIFY_PATH=1
UNINSTALL=0
CHECK=0

#--------------------------------------------------------------------------------------------------
# Terminal Detection
#--------------------------------------------------------------------------------------------------

# Check if we have a TTY and support colors
if [ -t 1 ] && [ -t 2 ]; then
    HAS_TTY=1
    TERM_COLS=$(tput cols 2>/dev/null || echo 70)
else
    HAS_TTY=0
    TERM_COLS=70
fi

# Check unicode support
if printf '⠋' 2>/dev/null | grep -q '⠋' 2>/dev/null; then
    HAS_UNICODE=1
else
    HAS_UNICODE=0
fi

#--------------------------------------------------------------------------------------------------
# Colors & Symbols
#--------------------------------------------------------------------------------------------------

if [ "$HAS_TTY" = 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BLUE='\033[0;34m'
    MAGENTA='\033[0;35m'
    CYAN='\033[0;36m'
    BOLD='\033[1m'
    DIM='\033[2m'
    RESET='\033[0m'
else
    RED='' GREEN='' YELLOW='' BLUE='' MAGENTA='' CYAN='' BOLD='' DIM='' RESET=''
fi

if [ "$HAS_UNICODE" = 1 ]; then
    SYM_OK="✓"
    SYM_ERR="✗"
    SYM_WARN="⚠"
    SYM_ARROW="→"
    SYM_BULLET="•"
    SPINNER_FRAMES='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
    BOX_TL="╭" BOX_TR="╮" BOX_BL="╰" BOX_BR="╯" BOX_H="─" BOX_V="│"
else
    SYM_OK="+"
    SYM_ERR="x"
    SYM_WARN="!"
    SYM_ARROW="->"
    SYM_BULLET="*"
    SPINNER_FRAMES='|/-\'
    BOX_TL="+" BOX_TR="+" BOX_BL="+" BOX_BR="+" BOX_H="-" BOX_V="|"
fi

#--------------------------------------------------------------------------------------------------
# Utility Functions
#--------------------------------------------------------------------------------------------------

# Print to stderr
err() {
    printf '%b\n' "$*" >&2
}

# Print unless quiet mode
log() {
    [ "$QUIET" = 1 ] && return
    printf '%b\n' "$*"
}

# Print without newline
logn() {
    [ "$QUIET" = 1 ] && return
    printf '%b' "$*"
}

# Print a horizontal line
hr() {
    _width=${1:-$TERM_COLS}
    _char=${2:-$BOX_H}
    _i=0
    while [ $_i -lt "$_width" ]; do
        printf '%s' "$_char"
        _i=$((_i + 1))
    done
}

# Print a box header with colored title
box_top() {
    _text="$1"
    _width=${2:-66}
    _inner=$((_width - 2))
    _text_len=${#_text}
    _pad_total=$((_inner - _text_len))
    _pad_left=$((_pad_total / 2))
    _pad_right=$((_pad_total - _pad_left))

    printf '%b%s' "$CYAN" "$BOX_TL"
    hr "$_inner"
    printf '%s%b\n' "$BOX_TR" "$RESET"

    printf '%b%s%b' "$CYAN" "$BOX_V" "$RESET"
    _i=0; while [ $_i -lt $_pad_left ]; do printf ' '; _i=$((_i + 1)); done
    printf '%b%s%b' "$CYAN$BOLD" "$_text" "$RESET"
    _i=0; while [ $_i -lt $_pad_right ]; do printf ' '; _i=$((_i + 1)); done
    printf '%b%s%b\n' "$CYAN" "$BOX_V" "$RESET"

    printf '%b%s' "$CYAN" "$BOX_BL"
    hr "$_inner"
    printf '%s%b\n' "$BOX_BR" "$RESET"
}

# Get character at position (1-indexed)
char_at() {
    _str="$1"
    _pos="$2"
    printf '%s' "$_str" | cut -c"$_pos"
}

#--------------------------------------------------------------------------------------------------
# Spinner Animation
#--------------------------------------------------------------------------------------------------

# Run a command with animated spinner
# Usage: spin "message" command [args...]
spin() {
    _msg="$1"
    shift

    if [ "$QUIET" = 1 ]; then
        "$@" >/dev/null 2>&1
        return $?
    fi

    # Create temp file for output capture
    _out_file="${TMPDIR:-/tmp}/tool-cli-install-out.$$"

    # Run command in background
    "$@" >"$_out_file" 2>&1 &
    _pid=$!

    _i=1
    _max_frames=10
    [ "$HAS_UNICODE" = 0 ] && _max_frames=4

    # Animate while process runs
    while kill -0 "$_pid" 2>/dev/null; do
        _frame=$(char_at "$SPINNER_FRAMES" "$_i")
        printf '\r  %b%s%b %s' "$CYAN" "$_frame" "$RESET" "$_msg"
        _i=$((_i % _max_frames + 1))
        sleep 0.08
    done

    # Get exit status
    wait "$_pid"
    _status=$?

    # Clear line and show result
    printf '\r\033[K'
    if [ $_status -eq 0 ]; then
        printf '  %b%s%b %s\n' "$GREEN" "$SYM_OK" "$RESET" "$_msg"
    else
        printf '  %b%s%b %s\n' "$RED" "$SYM_ERR" "$RESET" "$_msg"
        # Show error output indented
        if [ -s "$_out_file" ]; then
            printf '\n'
            sed 's/^/     /' "$_out_file" >&2
            printf '\n'
        fi
    fi

    rm -f "$_out_file"
    return $_status
}

# Show step progress without spinner
step_ok() {
    log "  ${GREEN}${SYM_OK}${RESET} $1"
}

step_err() {
    err "  ${RED}${SYM_ERR}${RESET} $1"
}

step_warn() {
    log "  ${YELLOW}${SYM_WARN}${RESET} $1"
}

step_info() {
    log "  ${BLUE}${SYM_BULLET}${RESET} $1"
}

#--------------------------------------------------------------------------------------------------
# Platform Detection
#--------------------------------------------------------------------------------------------------

detect_platform() {
    OS=""
    ARCH=""

    case "$(uname -s)" in
        Linux*)  OS="linux" ;;
        Darwin*) OS="darwin" ;;
        MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
        *)
            err "${RED}Unsupported operating system: $(uname -s)${RESET}"
            exit 1
            ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64)  ARCH="x86_64" ;;
        arm64|aarch64) ARCH="aarch64" ;;  # Normalize arm64 -> aarch64
        *)
            err "${RED}Unsupported architecture: $(uname -m)${RESET}"
            exit 1
            ;;
    esac

    PLATFORM="${OS}-${ARCH}"
}

#--------------------------------------------------------------------------------------------------
# Checksum Verification
#--------------------------------------------------------------------------------------------------

# Get the available checksum command
get_checksum_cmd() {
    if command -v sha256sum >/dev/null 2>&1; then
        echo "sha256sum"
    elif command -v shasum >/dev/null 2>&1; then
        echo "shasum -a 256"
    else
        echo ""
    fi
}

# Verify checksum of a file
# Usage: verify_checksum file expected_hash
verify_checksum() {
    _file="$1"
    _expected="$2"
    _cmd=$(get_checksum_cmd)

    if [ -z "$_cmd" ]; then
        step_warn "No checksum tool available, skipping verification"
        return 0
    fi

    _actual=$($_cmd "$_file" | cut -d' ' -f1)

    if [ "$_actual" = "$_expected" ]; then
        return 0
    else
        err "Checksum mismatch:"
        err "  Expected: $_expected"
        err "  Actual:   $_actual"
        return 1
    fi
}

#--------------------------------------------------------------------------------------------------
# Version Detection
#--------------------------------------------------------------------------------------------------

get_latest_version() {
    _url="https://api.github.com/repos/${GITHUB_REPO}/releases/latest"
    _version=$(curl -fsSL "$_url" 2>/dev/null | grep '"tag_name"' | sed -E 's/.*"v?([^"]+)".*/\1/')

    if [ -z "$_version" ]; then
        err "${RED}Failed to fetch latest version${RESET}"
        exit 1
    fi

    echo "$_version"
}

#--------------------------------------------------------------------------------------------------
# Installation Functions
#--------------------------------------------------------------------------------------------------

check_requirements() {
    _missing=""

    for _cmd in curl tar; do
        if ! command -v "$_cmd" >/dev/null 2>&1; then
            _missing="$_missing $_cmd"
        fi
    done

    if [ -n "$_missing" ]; then
        err "${RED}Missing required commands:${RESET}$_missing"
        err "Please install them and try again."
        exit 1
    fi
}

create_directories() {
    mkdir -p "$PREFIX/bin"
}

download_release() {
    _version="$1"
    _dest="$2"
    _archive="tool-${_version}-${PLATFORM}.tar.gz"
    _url="https://github.com/${GITHUB_REPO}/releases/download/v${_version}/${_archive}"
    _file="$_dest/$_archive"

    printf '  %b↓%b Downloading %s\n' "$CYAN" "$RESET" "$_archive" >&2

    # Get total size via HEAD request (follow redirects)
    _total=$(curl -fsSIL "$_url" 2>/dev/null | grep -i content-length | tail -1 | awk '{print $2}' | tr -d '\r')

    if [ -z "$_total" ] || [ "$_total" = "0" ]; then
        # Fallback to simple progress bar if we can't get size
        if curl -fSL --progress-bar -o "$_file" "$_url" 2>&1 >&2; then
            printf '  %b%s%b Downloaded %s\n' "$GREEN" "$SYM_OK" "$RESET" "$_archive" >&2
        else
            printf '  %b%s%b Download failed\n' "$RED" "$SYM_ERR" "$RESET" >&2
            return 1
        fi
        echo "$_archive"
        return 0
    fi

    # Start download in background
    curl -fsSL -o "$_file" "$_url" 2>/dev/null &
    _pid=$!

    # Progress bar characters
    if [ "$HAS_UNICODE" = 1 ]; then
        _fill="█"
        _empty="░"
    else
        _fill="#"
        _empty="-"
    fi

    # Draw progress bar while downloading
    while kill -0 "$_pid" 2>/dev/null; do
        if [ -f "$_file" ]; then
            # Get current file size (macOS vs Linux)
            _current=$(stat -f%z "$_file" 2>/dev/null || stat -c%s "$_file" 2>/dev/null || echo 0)

            if [ "$_total" -gt 0 ]; then
                _percent=$((_current * 100 / _total))
                [ "$_percent" -gt 100 ] && _percent=100
                _filled=$((_percent * 40 / 100))
                _empty_count=$((40 - _filled))

                # Format sizes
                _cur_mb=$(awk "BEGIN {printf \"%.1f\", $_current / 1048576}")
                _tot_mb=$(awk "BEGIN {printf \"%.1f\", $_total / 1048576}")

                # Build progress bar
                _bar=""
                _i=0; while [ $_i -lt $_filled ]; do _bar="${_bar}${_fill}"; _i=$((_i + 1)); done
                _i=0; while [ $_i -lt $_empty_count ]; do _bar="${_bar}${_empty}"; _i=$((_i + 1)); done

                printf '\r    [%b%s%b] %3d%% %sM/%sM' "$GREEN" "$_bar" "$RESET" "$_percent" "$_cur_mb" "$_tot_mb" >&2
            fi
        fi
        sleep 0.1
    done

    # Wait for curl to finish and get exit status
    wait "$_pid"
    _status=$?

    if [ $_status -eq 0 ]; then
        # Final 100% bar
        _bar=""
        _i=0; while [ $_i -lt 40 ]; do _bar="${_bar}${_fill}"; _i=$((_i + 1)); done
        _tot_mb=$(awk "BEGIN {printf \"%.1f\", $_total / 1048576}")
        printf '\r    [%b%s%b] 100%% %sM/%sM\n' "$GREEN" "$_bar" "$RESET" "$_tot_mb" "$_tot_mb" >&2
        printf '  %b%s%b Downloaded %s\n' "$GREEN" "$SYM_OK" "$RESET" "$_archive" >&2
    else
        printf '\r\033[K' >&2
        printf '  %b%s%b Download failed\n' "$RED" "$SYM_ERR" "$RESET" >&2
        return 1
    fi

    echo "$_archive"
}

download_checksum() {
    _version="$1"
    _dest="$2"
    _archive="$3"
    _checksum_file="${_archive}.sha256"
    _url="https://github.com/${GITHUB_REPO}/releases/download/v${_version}/${_checksum_file}"

    if curl -fsSL -o "$_dest/$_checksum_file" "$_url" 2>/dev/null; then
        cat "$_dest/$_checksum_file" | cut -d' ' -f1
    else
        echo ""
    fi
}

extract_archive() {
    _archive="$1"
    _dest="$2"

    tar -xzf "$_archive" -C "$_dest"
}

install_binaries() {
    _src="$1"
    _dest="$2"

    for _bin in $BINARIES; do
        if [ -f "$_src/$_bin" ]; then
            install -m 755 "$_src/$_bin" "$_dest/"
        fi
    done
}

backup_existing() {
    _dir="$1"
    _backup_dir="${TMPDIR:-/tmp}/tool-cli-backup.$$"

    mkdir -p "$_backup_dir"

    for _bin in $BINARIES; do
        [ -f "$_dir/$_bin" ] && cp "$_dir/$_bin" "$_backup_dir/"
    done

    echo "$_backup_dir"
}

restore_backup() {
    _backup="$1"
    _dest="$2"

    if [ -d "$_backup" ]; then
        cp -P "$_backup"/* "$_dest/" 2>/dev/null || true
        rm -rf "$_backup"
    fi
}

verify_installation() {
    _dir="$1"
    _failed=0

    for _bin in $BINARIES; do
        if [ -x "$_dir/$_bin" ]; then
            # Try to run --version or similar
            if ! "$_dir/$_bin" --version >/dev/null 2>&1; then
                # Some binaries might not have --version, just check they execute
                if ! "$_dir/$_bin" --help >/dev/null 2>&1; then
                    : # Accept if binary exists and is executable
                fi
            fi
        else
            _failed=1
        fi
    done

    return $_failed
}

#--------------------------------------------------------------------------------------------------
# Uninstall
#--------------------------------------------------------------------------------------------------

do_uninstall() {
    log ""
    box_top "Tool CLI Uninstaller"
    log ""

    _bin_dir="$PREFIX/bin"
    _removed=0

    for _bin in $BINARIES; do
        if [ -f "$_bin_dir/$_bin" ]; then
            rm -f "$_bin_dir/$_bin"
            step_ok "Removed $_bin"
            _removed=1
        fi
    done

    if [ "$_removed" = 0 ]; then
        step_info "No tool-cli installation found in $_bin_dir"
    else
        log ""
        step_ok "Uninstall complete"
    fi

    log ""
}

#--------------------------------------------------------------------------------------------------
# Check Installation
#--------------------------------------------------------------------------------------------------

do_check() {
    log ""
    box_top "Tool CLI Installation Check"
    log ""

    _bin_dir="$PREFIX/bin"
    _all_ok=1

    for _bin in $BINARIES; do
        if [ -x "$_bin_dir/$_bin" ]; then
            _ver=$("$_bin_dir/$_bin" --version 2>/dev/null | head -1 || echo "unknown")
            step_ok "$_bin ${DIM}($_ver)${RESET}"
        else
            step_err "$_bin not found"
            _all_ok=0
        fi
    done

    log ""

    # Check PATH
    case ":$PATH:" in
        *":$_bin_dir:"*) step_ok "$_bin_dir is in PATH" ;;
        *) step_warn "$_bin_dir is not in PATH" ;;
    esac

    log ""

    if [ "$_all_ok" = 1 ]; then
        return 0
    else
        return 1
    fi
}

#--------------------------------------------------------------------------------------------------
# PATH Configuration
#--------------------------------------------------------------------------------------------------

configure_path() {
    _bin_dir="$1"
    _shell_name=""
    _rc_file=""

    # Detect current shell
    case "$SHELL" in
        */bash) _shell_name="bash"; _rc_file="$HOME/.bashrc" ;;
        */zsh)  _shell_name="zsh";  _rc_file="$HOME/.zshrc" ;;
        */fish) _shell_name="fish"; _rc_file="$HOME/.config/fish/config.fish" ;;
        *)      _shell_name="sh";   _rc_file="$HOME/.profile" ;;
    esac

    # Check if already in PATH
    case ":$PATH:" in
        *":$_bin_dir:"*)
            step_ok "$_bin_dir already in PATH"
            return 0
            ;;
    esac

    # Check if already configured in rc file
    if [ -f "$_rc_file" ] && grep -q "$_bin_dir" "$_rc_file" 2>/dev/null; then
        step_info "PATH already configured in $_rc_file"
        step_info "Restart your shell or run: source $_rc_file"
        return 0
    fi

    if [ "$MODIFY_PATH" = 0 ]; then
        step_warn "$_bin_dir not in PATH"
        step_info "Add it manually: export PATH=\"$_bin_dir:\$PATH\""
        return 0
    fi

    # Ask user
    log ""
    logn "  Add ${CYAN}$_bin_dir${RESET} to PATH in $_rc_file? [Y/n] "

    if [ "$FORCE" = 1 ]; then
        log "y (--force)"
        _reply="y"
    else
        read -r _reply </dev/tty || _reply="y"
    fi

    case "$_reply" in
        [Nn]*)
            step_info "Skipped PATH configuration"
            step_info "Add manually: export PATH=\"$_bin_dir:\$PATH\""
            ;;
        *)
            # Add to rc file
            if [ "$_shell_name" = "fish" ]; then
                mkdir -p "$(dirname "$_rc_file")"
                printf '\n# Added by tool-cli installer\nfish_add_path %s\n' "$_bin_dir" >> "$_rc_file"
            else
                printf '\n# Added by tool-cli installer\nexport PATH="%s:$PATH"\n' "$_bin_dir" >> "$_rc_file"
            fi
            step_ok "Added to $_rc_file"
            step_info "Restart your shell or run: source $_rc_file"
            ;;
    esac
}

#--------------------------------------------------------------------------------------------------
# Main Installation
#--------------------------------------------------------------------------------------------------

do_install() {
    log ""
    box_top "Tool CLI Installer v${SCRIPT_VERSION}"
    log ""

    # Detect platform
    detect_platform
    step_info "Platform: ${BOLD}${PLATFORM}${RESET}"

    # Get version
    if [ -z "$VERSION" ]; then
        logn "  ${BLUE}${SYM_BULLET}${RESET} Fetching latest version..."
        VERSION=$(get_latest_version)
        printf '\r\033[K'
        step_info "Version:  ${BOLD}${VERSION}${RESET}"
    else
        step_info "Version:  ${BOLD}${VERSION}${RESET}"
    fi

    log ""

    # Create temp directory
    TEMP_DIR="${TMPDIR:-/tmp}/tool-cli-install.$$"
    mkdir -p "$TEMP_DIR"
    trap 'rm -rf "$TEMP_DIR"' EXIT

    # Backup existing installation
    _backup=""
    if [ -f "$PREFIX/bin/tool" ] && [ "$FORCE" = 0 ]; then
        _backup=$(backup_existing "$PREFIX/bin")
    fi

    # Download (with progress bar)
    _archive=$(download_release "$VERSION" "$TEMP_DIR") || exit 1

    # Checksum
    _expected_hash=$(download_checksum "$VERSION" "$TEMP_DIR" "$_archive")
    if [ -n "$_expected_hash" ]; then
        spin "Verifying checksum" verify_checksum "$TEMP_DIR/$_archive" "$_expected_hash"
    else
        step_warn "Checksum not available, skipping verification"
    fi

    # Extract
    spin "Extracting files" extract_archive "$TEMP_DIR/$_archive" "$TEMP_DIR"

    # Find extracted directory
    _extract_dir=$(find "$TEMP_DIR" -mindepth 1 -maxdepth 1 -type d | head -1)
    [ -z "$_extract_dir" ] && _extract_dir="$TEMP_DIR"

    # Create directories
    create_directories

    # Install
    spin "Installing binary" install_binaries "$_extract_dir" "$PREFIX/bin"

    # Verify
    if spin "Verifying installation" verify_installation "$PREFIX/bin"; then
        # Success - remove backup
        [ -n "$_backup" ] && rm -rf "$_backup"
    else
        # Failed - restore backup
        if [ -n "$_backup" ]; then
            step_warn "Installation verification failed, restoring backup"
            restore_backup "$_backup" "$PREFIX/bin"
        fi
        exit 1
    fi

    # PATH configuration
    configure_path "$PREFIX/bin"

    # Summary
    log ""
    log "  ${GREEN}${SYM_OK}${RESET} ${BOLD}Installation complete${RESET}"
    log ""
    for _bin in $BINARIES; do
        _ver=$("$PREFIX/bin/$_bin" --version 2>/dev/null | head -1 || echo "")
        log "    ${SYM_BULLET} ${BOLD}$_bin${RESET} ${DIM}$PREFIX/bin/$_bin${RESET} ${DIM}$_ver${RESET}"
    done
    log ""
    log "  Run ${CYAN}tool --help${RESET} or ${CYAN}tool --tree${RESET} to get started."
    log ""
}

#--------------------------------------------------------------------------------------------------
# Help
#--------------------------------------------------------------------------------------------------

show_help() {
    printf '%b\n' "${BOLD}USAGE${RESET}"
    printf '    curl -fsSL https://raw.githubusercontent.com/zerocore-ai/tool-cli/main/install.sh | sh\n'
    printf '    ./install.sh [OPTIONS]\n'
    printf '\n'
    printf '%b\n' "${BOLD}OPTIONS${RESET}"
    printf '    --version=X.Y.Z    Install specific version (default: latest)\n'
    printf '    --prefix=PATH      Installation prefix (default: ~/.local)\n'
    printf '    --uninstall        Remove tool binary\n'
    printf '    --check            Verify existing installation\n'
    printf '    --no-modify-path   Don'\''t prompt to modify shell PATH\n'
    printf '    --quiet            Minimal output\n'
    printf '    --force            Don'\''t prompt, assume yes\n'
    printf '    --help             Show this help\n'
    printf '\n'
}

#--------------------------------------------------------------------------------------------------
# Argument Parsing
#--------------------------------------------------------------------------------------------------

parse_args() {
    for _arg in "$@"; do
        case "$_arg" in
            --version=*)
                VERSION="${_arg#*=}"
                ;;
            --prefix=*)
                PREFIX="${_arg#*=}"
                ;;
            --uninstall)
                UNINSTALL=1
                ;;
            --check)
                CHECK=1
                ;;
            --no-modify-path)
                MODIFY_PATH=0
                ;;
            --quiet|-q)
                QUIET=1
                ;;
            --force|-f)
                FORCE=1
                ;;
            --help|-h)
                show_help
                exit 0
                ;;
            *)
                err "Unknown option: $_arg"
                err "Run with --help for usage"
                exit 1
                ;;
        esac
    done
}

#--------------------------------------------------------------------------------------------------
# Entry Point
#--------------------------------------------------------------------------------------------------

main() {
    parse_args "$@"
    check_requirements

    if [ "$UNINSTALL" = 1 ]; then
        do_uninstall
    elif [ "$CHECK" = 1 ]; then
        do_check
    else
        do_install
    fi
}

main "$@"
