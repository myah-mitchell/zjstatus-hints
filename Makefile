TARGET  := wasm32-wasip1
WASM    := target/$(TARGET)/release/zjstatus-hints.wasm
PLUGIN  := $(HOME)/.local/share/zellij/plugins/zjstatus-hints.wasm
REPO    := myah-mitchell/zjstatus-hints

.PHONY: build install dev test check nightly latest

# Build the release wasm.
build:
	cargo build --release --target $(TARGET)

# Copy the built wasm into the local Zellij plugin dir.
# Writes through the symlink, so the dotfiles setup stays intact.
install: build
	install -m 644 $(WASM) $(PLUGIN)
	@echo "Installed -> $(PLUGIN)"

# Build + install in one step. Then start a fresh Zellij session to load it.
dev: install

# Tests build for the host, not wasm, and need OpenSSL headers
# (libssl-dev). See docs/AUTOMATION.md if this fails to link.
test:
	cargo test --all-features

# What CI runs, so a red build can be reproduced before pushing.
check: test
	cargo fmt --all --check
	cargo clippy --all-features --target $(TARGET) -- -D warnings
	cargo build --release --target $(TARGET)

# Install the newest nightly from GitHub.
#
# Zellij caches remote plugins by URL, so pointing the config at a rolling
# release URL will keep serving whatever it downloaded first. Fetching to the
# plugin path instead means the next session always picks up the new build.
nightly:
	@echo "Fetching nightly from $(REPO)…"
	@curl -fsSL -o "$(PLUGIN).tmp" \
		"https://github.com/$(REPO)/releases/download/nightly/zjstatus-hints.wasm"
	@mv "$(PLUGIN).tmp" "$(PLUGIN)"
	@echo "Installed nightly -> $(PLUGIN)"
	@echo "Start a new Zellij session to load it."

# Same, but the current stable release.
latest:
	@echo "Fetching latest release from $(REPO)…"
	@curl -fsSL -o "$(PLUGIN).tmp" \
		"https://github.com/$(REPO)/releases/latest/download/zjstatus-hints.wasm"
	@mv "$(PLUGIN).tmp" "$(PLUGIN)"
	@echo "Installed latest -> $(PLUGIN)"
	@echo "Start a new Zellij session to load it."
