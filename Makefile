# =============================================================================
# tool-cli Makefile - Build and install tool CLI
# =============================================================================

# -----------------------------------------------------------------------------
# Build Configuration
# -----------------------------------------------------------------------------
DEBUG ?= 0
LTO ?= 0
CARGO_BUILD_MODE := $(if $(filter 1,$(DEBUG)),,--release)
CARGO_TARGET_DIR := target/$(if $(filter 1,$(DEBUG)),debug,release)

# Set CARGO_PROFILE_RELEASE_LTO based on LTO setting
export CARGO_PROFILE_RELEASE_LTO := $(if $(filter 1,$(LTO)),true,off)

# -----------------------------------------------------------------------------
# Installation Paths
# -----------------------------------------------------------------------------
HOME_BIN := $(HOME)/.local/bin

# -----------------------------------------------------------------------------
# Build Paths and Directories
# -----------------------------------------------------------------------------
TOOL_BIN := $(CARGO_TARGET_DIR)/tool
BUILD_DIR := build

# -----------------------------------------------------------------------------
# Phony Targets Declaration
# -----------------------------------------------------------------------------
.PHONY: all build install clean help uninstall force-build run

# -----------------------------------------------------------------------------
# Main Targets
# -----------------------------------------------------------------------------
all: build

# Always rebuild to ensure we catch compilation errors
build: force-build
	@mkdir -p $(BUILD_DIR)
	@cp $(TOOL_BIN) $(BUILD_DIR)/
	@echo "tool-cli build artifacts ($(if $(filter 1,$(DEBUG)),debug,release) mode) copied to $(BUILD_DIR)/"

force-build: $(TOOL_BIN)

# -----------------------------------------------------------------------------
# Binary Building
# -----------------------------------------------------------------------------
# Force rebuild every time to catch compilation errors
$(TOOL_BIN): FORCE
	cargo build $(CARGO_BUILD_MODE) --bin tool

# FORCE target ensures the binaries are always rebuilt
FORCE:

# -----------------------------------------------------------------------------
# Installation
# -----------------------------------------------------------------------------
install: build
	@echo "Installing $(if $(filter 1,$(DEBUG)),debug,release) build..."
	install -d $(HOME_BIN)
	install -m 755 $(BUILD_DIR)/tool $(HOME_BIN)/tool
	@echo "Installation complete."

# -----------------------------------------------------------------------------
# Run Helpers
# -----------------------------------------------------------------------------
run: build
	cargo run $(CARGO_BUILD_MODE) --bin tool -- $(ARGS)

# -----------------------------------------------------------------------------
# Maintenance
# -----------------------------------------------------------------------------
clean:
	rm -rf $(BUILD_DIR)
	cargo clean

uninstall:
	rm -f $(HOME_BIN)/tool

# -----------------------------------------------------------------------------
# Help Documentation
# -----------------------------------------------------------------------------
help:
	@echo "tool-cli Makefile Help"
	@echo "======================"
	@echo
	@echo "Targets:"
	@echo "  build      - Build tool CLI binary (always rebuilds)"
	@echo "  install    - Build and install binary to ~/.local/bin"
	@echo "  run        - Build and run tool with ARGS"
	@echo "  uninstall  - Remove installed binary"
	@echo "  clean      - Remove build artifacts and cargo build cache"
	@echo "  help       - Show this help message"
	@echo
	@echo "Options:"
	@echo "  DEBUG=1    - Build in debug mode (default: release mode)"
	@echo "  LTO=1      - Enable link-time optimization for smaller binaries"
	@echo "  ARGS=...   - Extra arguments forwarded to tool when using run"
	@echo
	@echo "Examples:"
	@echo "  make                 # Build in release mode"
	@echo "  make build           # Same as above"
	@echo "  make install         # Build and install to ~/.local/bin"
	@echo "  make DEBUG=1 build   # Build in debug mode"
	@echo "  make LTO=1 install   # Build with optimization and install"
	@echo "  make run ARGS='--tree'  # Build and run tool --tree"
	@echo "  make clean           # Clean all build artifacts"
	@echo
	@echo "Note: The build target always rebuilds to ensure compilation errors are caught."
