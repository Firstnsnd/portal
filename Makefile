# Portal — development tasks
# `make help` lists all targets.

CARGO ?= cargo

UNAME_S := $(shell uname -s)

.PHONY: help run run-log build build-release check test bench fmt fmt-check clippy lint dmg release clean integrate integrate-remove

help: ## List available targets
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

run: ## Run the app (debug build)
	$(CARGO) run

run-log: ## Run with debug logging (RUST_LOG=debug) — shows port-forward diagnostics
	RUST_LOG=debug $(CARGO) run

build: ## Build (debug)
	$(CARGO) build

build-release: ## Build (release)
	$(CARGO) build --release

check: ## Type-check without producing a binary
	$(CARGO) check --all-targets

test: ## Run the test suite
	$(CARGO) test

bench: ## Run benchmarks (release mode)
	$(CARGO) bench

fmt: ## Format code
	$(CARGO) fmt

fmt-check: ## Check formatting without rewriting files
	$(CARGO) fmt --check

clippy: ## Lint with clippy
	$(CARGO) clippy --all-targets

lint: fmt-check clippy ## Format check + clippy

dmg: ## Build, sign, and package Portal.app into a .dmg
	./scripts/build-dmg.sh

release: ## Full release: bump version, commit, tag, push (make release VERSION=x.y.z)
	@if [ -z "$(VERSION)" ]; then echo "Usage: make release VERSION=x.y.z"; exit 1; fi
	./scripts/release.sh $(VERSION)

clean: ## Remove build artifacts
	$(CARGO) clean

# Register OS right-click actions ("New Portal Tab/Window Here").
# Defaults to the local debug build; PORTAL_BIN= uses the installed app bundle.
ifeq ($(UNAME_S),Darwin)
integrate: ## Install Finder Quick Actions (uses the debug build)
	PORTAL_BIN="$(CURDIR)/target/debug/portal" ./scripts/install_finder_actions.sh

integrate-remove: ## Uninstall Finder Quick Actions
	./scripts/install_finder_actions.sh --uninstall
else
integrate: ## Install file-manager actions (Nautilus/Dolphin)
	./scripts/install_filemanager_actions.sh

integrate-remove: ## Uninstall file-manager actions
	./scripts/install_filemanager_actions.sh --uninstall
endif
