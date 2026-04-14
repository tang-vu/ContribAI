.PHONY: help dev build test lint clippy fmt bench release clean install dev-deps stats docker audit completions

# Default target
help: ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-15s\033[0m %s\n", $$1, $$2}'

# ── Development ─────────────────────────────────────────────────────────────

dev: dev-deps test clippy ## One-command setup + verify (dev dependencies, tests, lint)
	@echo ""
	@echo "✅ Development environment ready!"

dev-deps: ## Install development dependencies
	@echo "📦 Checking Rust toolchain..."
	@rustc --version || (echo "❌ Rust not installed. Install from: https://rustup.rs/" && exit 1)
	@cargo --version || true
	@echo "📦 Installing dev tools..."
	cargo install --quiet cargo-audit || true
	cargo install --quiet cargo-outdated || true
	cargo install --quiet typos-cli || true
	@echo "📦 Setting up pre-commit hooks..."
	@if command -v pre-commit >/dev/null 2>&1; then \
		pre-commit install 2>/dev/null || true; \
	fi
	@echo "✅ Dev dependencies installed"

# ── Build ────────────────────────────────────────────────────────────────────

build: ## Build release binary
	@echo "🔨 Building release..."
	cargo build --release
	@echo "✅ Binary: target/release/contribai"

install: ## Install to PATH
	@echo "📦 Installing to ~/.cargo/bin..."
	cargo install --path crates/contribai-rs
	@echo "✅ Installed: contribai"

# ── Testing ──────────────────────────────────────────────────────────────────

test: ## Run all tests
	@echo "🧪 Running 600+ tests..."
	cargo test

test-quick: ## Run tests without output
	cargo test --quiet

test-watch: ## Watch and re-run tests on change (requires cargo-watch)
	cargo watch -x test

# ── Linting ──────────────────────────────────────────────────────────────────

lint: fmt clippy ## Run all lint checks (fmt + clippy)

fmt: ## Format code with rustfmt
	cargo fmt --all

fmt-check: ## Check formatting
	cargo fmt --all -- --check

clippy: ## Run clippy linter (zero warnings allowed)
	cargo clippy -- -D warnings

typos: ## Check for typos (requires typos-cli)
	typos || true

# ── Benchmarks ───────────────────────────────────────────────────────────────

bench: ## Run benchmarks
	@echo "⚡ Running benchmarks..."
	cargo bench
	@echo "📊 Results: target/criterion/report/index.html"

bench-save: ## Save baseline benchmarks
	cargo bench -- --save-baseline latest

bench-diff: ## Compare against saved baseline
	cargo bench -- --baseline latest

# ── Security ─────────────────────────────────────────────────────────────────

audit: ## Check for security vulnerabilities
	cargo audit

outdated: ## Check for outdated dependencies
	cargo outdated

# ── Release ──────────────────────────────────────────────────────────────────

release: test clippy ## Build production binary
	@echo "🔨 Building optimized release..."
	cargo build --release
	@echo ""
	@echo "📦 Binary: target/release/contribai"
	@ls -lh target/release/contribai 2>/dev/null || ls -lh target/release/contribai.exe 2>/dev/null || true

release-strip: ## Build + strip binary
	cargo build --release
	@strip target/release/contribai 2>/dev/null || strip target/release/contribai.exe 2>/dev/null || true
	@echo "📦 Stripped binary:"
	@ls -lh target/release/contribai 2>/dev/null || ls -lh target/release/contribai.exe 2>/dev/null || true

tag: ## Create and push a new release tag (usage: make tag VERSION=v6.3.0)
	@if [ -z "$(VERSION)" ]; then \
		echo "❌ Usage: make tag VERSION=v6.3.0"; \
		exit 1; \
	fi
	@echo "🏷 Creating tag $(VERSION)..."
	git tag -a $(VERSION) -m "Release $(VERSION)"
	git push origin $(VERSION)
	@echo "✅ Tag $(VERSION) pushed — CD will build binaries"

# ── Shell Completions ───────────────────────────────────────────────────────

completions: build ## Generate shell completions
	@echo "🔧 Generating shell completions..."
	@mkdir -p completions
	@./target/release/contribai completions bash > completions/contribai.bash 2>/dev/null || true
	@./target/release/contribai completions zsh > completions/contribai.zsh 2>/dev/null || true
	@./target/release/contribai completions fish > completions/contribai.fish 2>/dev/null || true
	@./target/release/contribai completions powershell > completions/contribai.ps1 2>/dev/null || true
	@echo "✅ Completions generated in completions/"

# ── Docker ───────────────────────────────────────────────────────────────────

docker: ## Build Docker image
	docker build -t contribai:latest .

docker-run: ## Run with Docker (dry run)
	docker run --rm -v $(PWD)/config.yaml:/home/contribai/config.yaml:ro contribai:latest run --dry-run

# ── Cleanup ──────────────────────────────────────────────────────────────────

clean: ## Clean build artifacts
	cargo clean
	rm -rf dist/ build/ *.egg-info .pytest_cache htmlcov .coverage completions/
	find . -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true

clean-targets: ## Only clean build targets (faster)
	cargo clean

# ── Stats ────────────────────────────────────────────────────────────────────

stats: ## Show project statistics
	@echo "📁 Rust files:"
	@find crates/contribai-rs/src -name "*.rs" | wc -l
	@echo "📝 Lines of code:"
	@find crates/contribai-rs/src -name "*.rs" -exec cat {} + | wc -l
	@echo "🧪 Test files:"
	@find crates/contribai-rs/tests -name "*.rs" 2>/dev/null | wc -l || echo "0"
	@echo "📊 Binary size:"
	@ls -lh target/release/contribai 2>/dev/null | awk '{print $$5}' || ls -lh target/release/contribai.exe 2>/dev/null | awk '{print $$5}' || echo "not built"

# ── Legacy Python ────────────────────────────────────────────────────────────

py-test: ## Run legacy Python tests
	cd python && pytest tests/ -v --tb=short

py-lint: ## Lint legacy Python code
	cd python && ruff check contribai/
