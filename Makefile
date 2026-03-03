JUST := $(shell command -v just 2>/dev/null)

.PHONY: help fmt fmt-check lint test build-core build-mac build-mac-full install-mac semgrep shellcheck audit coverage coverage-html sonar dev check check-all

help:
	@echo "Common tasks:";
	@echo "  make fmt             # cargo fmt --all";
	@echo "  make fmt-check       # cargo fmt --check";
	@echo "  make lint            # cargo clippy --workspace --all-targets -- -D warnings";
	@echo "  make test            # cargo test --workspace";
	@echo "  make build-core      # cargo build --workspace";
	@echo "  make build-mac       # mac/build.sh --skip-rust";
	@echo "  make build-mac-full  # mac/build.sh";
	@echo "  make install-mac     # copy mac/build/ccvv.app to /Applications";
	@echo "  make semgrep         # semgrep scan --config auto";
	@echo "  make shellcheck      # lint .sh scripts";
	@echo "  make audit           # cargo audit";
	@echo "  make coverage        # cargo llvm-cov (lcov)";
	@echo "  make coverage-html   # cargo llvm-cov (html)";
	@echo "  make sonar           # Sonar scan via sonar-scanner or docker (requires env vars)";
	@echo "  make dev             # essential local checks (fmt + lint + test)";
	@echo "  make check           # PR checks (dev + semgrep + shellcheck + audit + coverage + sonar)";
	@echo "  make check-all       # check + coverage-html";

fmt:
ifdef JUST
	@$(JUST) fmt
else
	@cd core && cargo fmt --all
endif

fmt-check:
ifdef JUST
	@$(JUST) fmt-check
else
	@cd core && cargo fmt --all -- --check
endif

lint:
ifdef JUST
	@$(JUST) lint
else
	@cd core && cargo clippy --workspace --all-targets -- -D warnings
endif

test:
ifdef JUST
	@$(JUST) test
else
	@cd core && cargo test --workspace
endif

build-core:
ifdef JUST
	@$(JUST) build-core
else
	@cd core && cargo build --workspace
endif

build-mac:
ifdef JUST
	@$(JUST) build-mac
else
	@cd mac && ./build.sh --skip-rust
endif

build-mac-full:
ifdef JUST
	@$(JUST) build-mac-full
else
	@cd mac && ./build.sh
endif

install-mac:
ifdef JUST
	@$(JUST) install-mac
else
	@cd mac && ditto build/ccvv.app /Applications/ccvv.app
endif

semgrep:
ifdef JUST
	@$(JUST) semgrep
else
	@if ! command -v semgrep >/dev/null 2>&1; then \
		echo "semgrep not found. Install: brew install semgrep"; \
		exit 1; \
	fi
	@semgrep scan --config auto --error core mac
endif

shellcheck:
ifdef JUST
	@$(JUST) shellcheck
else
	@if ! command -v shellcheck >/dev/null 2>&1; then \
		echo "shellcheck not found. Install: brew install shellcheck"; \
		exit 1; \
	fi
	@scripts="$$(rg --files -g '*.sh' || true)"; \
	if [ -z "$$scripts" ]; then \
		echo "No shell scripts found."; \
	else \
		shellcheck $$scripts; \
	fi
endif

audit:
ifdef JUST
	@$(JUST) audit
else
	@if ! cargo audit --version >/dev/null 2>&1; then \
		echo "cargo-audit not found. Install: cargo install cargo-audit"; \
		exit 1; \
	fi
	@cd core && cargo audit
endif

coverage:
ifdef JUST
	@$(JUST) coverage
else
	@if ! cargo llvm-cov --version >/dev/null 2>&1; then \
		echo "cargo-llvm-cov not found. Install: cargo install cargo-llvm-cov"; \
		exit 1; \
	fi
	@cd core && cargo llvm-cov --workspace --lcov --output-path target/coverage/lcov.info
endif

coverage-html:
ifdef JUST
	@$(JUST) coverage-html
else
	@if ! cargo llvm-cov --version >/dev/null 2>&1; then \
		echo "cargo-llvm-cov not found. Install: cargo install cargo-llvm-cov"; \
		exit 1; \
	fi
	@cd core && cargo llvm-cov --workspace --html --output-dir target/coverage/html
endif

sonar:
ifdef JUST
	@$(JUST) sonar
else
	@if [ -z "$$SONAR_HOST_URL" ] || [ -z "$$SONAR_TOKEN" ] || [ -z "$$SONAR_PROJECT_KEY" ]; then \
		echo "Set SONAR_HOST_URL, SONAR_TOKEN, SONAR_PROJECT_KEY"; \
		exit 1; \
	fi
	@if command -v sonar-scanner >/dev/null 2>&1; then \
		sonar-scanner \
			-Dsonar.projectKey="$$SONAR_PROJECT_KEY" \
			-Dsonar.projectBaseDir="$$(pwd)" \
			-Dsonar.sources=core,mac \
			-Dsonar.exclusions="**/target/**,**/build/**,**/.git/**" \
			-Dsonar.host.url="$$SONAR_HOST_URL" \
			-Dsonar.token="$$SONAR_TOKEN" \
			-Dsonar.rust.lcov.reportPaths=core/target/coverage/lcov.info; \
	elif command -v docker >/dev/null 2>&1; then \
		docker run --rm \
			-e SONAR_HOST_URL \
			-e SONAR_TOKEN \
			-v "$$(pwd):/usr/src" \
			sonarsource/sonar-scanner-cli:latest \
			-Dsonar.projectKey="$$SONAR_PROJECT_KEY" \
			-Dsonar.projectBaseDir=/usr/src \
			-Dsonar.sources=core,mac \
			-Dsonar.exclusions="**/target/**,**/build/**,**/.git/**" \
			-Dsonar.rust.lcov.reportPaths=core/target/coverage/lcov.info; \
	else \
		echo "Need sonar-scanner or docker for sonar target."; \
		exit 1; \
	fi
endif

dev:
ifdef JUST
	@$(JUST) dev
else
	@$(MAKE) fmt
	@$(MAKE) lint
	@$(MAKE) test
endif

check:
ifdef JUST
	@$(JUST) check
else
	@$(MAKE) dev
	@$(MAKE) semgrep
	@$(MAKE) shellcheck
	@$(MAKE) audit
	@$(MAKE) coverage
	@$(MAKE) sonar
endif

check-all:
ifdef JUST
	@$(JUST) check-all
else
	@$(MAKE) check
	@$(MAKE) coverage-html
endif
