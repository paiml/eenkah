# eenkah — run `make` (or `make help`) to see the targets. Nothing runs without a target.

SUYO_UI := crates/suyo/ui
UI      := ui
BIN     := target/release/eenkah
DATA    ?=
LISTEN  ?= 127.0.0.1:8090
EENKAH  := $(BIN) $(if $(DATA),--data $(DATA))

.DEFAULT_GOAL := help
.PHONY: help ui ui-deps build run dev import test typecheck verify trace fmt clean

help: ## Show this help
	@echo "eenkah make targets:"
	@grep -hE '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "} {printf "  \033[1m%-10s\033[0m %s\n", $$1, $$2}'
	@echo
	@echo "Variables: DATA=<data dir> (default ~/.local/share/eenkah), LISTEN=$(LISTEN)"

ui-deps: ## Install the web apps' npm dependencies (eenkah and suyo)
	cd $(SUYO_UI) && npm ci --no-audit --no-fund
	cd $(UI) && npm ci --no-audit --no-fund

ui: ## Build the web assets (suyo charts + eenkah app) into ui/dist
	@[ -d $(SUYO_UI)/node_modules ] || (cd $(SUYO_UI) && npm ci --no-audit --no-fund)
	@[ -d $(UI)/node_modules ] || (cd $(UI) && npm ci --no-audit --no-fund)
	cd $(SUYO_UI) && npm run build
	cd $(UI) && npm run build

build: ui ## Build the web assets and the release binary (assets are embedded)
	cargo build --release -p eenkah-cli

run: build ## Build, then serve the web UI on LISTEN
	$(EENKAH) serve --listen $(LISTEN)

dev: ## Live UI development: eenkah API on :8090 + Vite with hot reload on :5173
	cargo build -p eenkah-cli
	(target/debug/eenkah $(if $(DATA),--data $(DATA)) serve --listen 127.0.0.1:8090 &) ; cd $(UI) && npm run dev

import: build ## Download new data from Garmin Connect and import it (sign in once with `eenkah login`)
	$(EENKAH) import

test: ## Rust tests and TypeScript type checks
	cargo test --workspace --release
	cd $(UI) && npm run typecheck

typecheck: ## TypeScript type checks only
	cd $(UI) && npm run typecheck

trace: ## Every spec rule has a test or a stated reason (docs/DEFERRED.md)
	cargo xtask trace

verify: ## Everything CI checks: format, clippy, tests, type checks, rule trace
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --release -- -D warnings
	$(MAKE) --no-print-directory test
	$(MAKE) --no-print-directory trace

fmt: ## Format the Rust code
	cargo fmt --all

clean: ## Remove build outputs (keeps your data)
	cargo clean
	rm -rf $(UI)/dist $(SUYO_UI)/dist
