TARGET  := wasm32-wasip1
WASM    := target/$(TARGET)/release/zjstatus-hints.wasm
PLUGIN  := $(HOME)/.local/share/zellij/plugins/zjstatus-hints.wasm

.PHONY: build install dev

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
