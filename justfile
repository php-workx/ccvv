set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
  @just --list

# Format Rust code.
fmt:
  cd core && cargo fmt --all

# Check Rust formatting without writing changes.
fmt-check:
  cd core && cargo fmt --all -- --check

# Run clippy with warnings denied.
lint:
  cd core && cargo clippy --workspace --all-targets -- -D warnings

# Run full test suite.
test:
  cd core && cargo test --workspace

# Build all Rust workspace crates.
build-core:
  cd core && cargo build --workspace

# Fast macOS build (reuse existing Rust lib).
build-mac:
  cd mac && ./build.sh --skip-rust

# Full macOS build (rebuild Rust lib first).
build-mac-full:
  cd mac && ./build.sh

# Install latest built mac app bundle.
install-mac:
  cd mac && ditto build/ccvv.app /Applications/ccvv.app

# Static code security scan (Semgrep).
semgrep:
  if ! command -v semgrep >/dev/null 2>&1; then
    echo "semgrep not found. Install: brew install semgrep"
    exit 1
  fi
  semgrep scan --config auto --error core mac

# Shell script linting.
shellcheck:
  if ! command -v shellcheck >/dev/null 2>&1; then
    echo "shellcheck not found. Install: brew install shellcheck"
    exit 1
  fi
  scripts="$(rg --files -g '*.sh' || true)"
  if [ -z "$scripts" ]; then
    echo "No shell scripts found."
    exit 0
  fi
  shellcheck $scripts

# Dependency vulnerability audit.
audit:
  if ! cargo audit --version >/dev/null 2>&1; then
    echo "cargo-audit not found. Install: cargo install cargo-audit"
    exit 1
  fi
  cd core && cargo audit

# Generate LCOV coverage report.
coverage:
  if ! cargo llvm-cov --version >/dev/null 2>&1; then
    echo "cargo-llvm-cov not found. Install: cargo install cargo-llvm-cov"
    exit 1
  fi
  cd core && cargo llvm-cov --workspace --lcov --output-path target/coverage/lcov.info

# Generate HTML coverage report.
coverage-html:
  if ! cargo llvm-cov --version >/dev/null 2>&1; then
    echo "cargo-llvm-cov not found. Install: cargo install cargo-llvm-cov"
    exit 1
  fi
  cd core && cargo llvm-cov --workspace --html --output-dir target/coverage/html

# Run SonarQube/SonarCloud scan.
sonar:
  if [ -z "${SONAR_HOST_URL:-}" ] || [ -z "${SONAR_TOKEN:-}" ] || [ -z "${SONAR_PROJECT_KEY:-}" ]; then
    echo "Set SONAR_HOST_URL, SONAR_TOKEN, SONAR_PROJECT_KEY"
    exit 1
  fi
  if command -v sonar-scanner >/dev/null 2>&1; then
    sonar-scanner \
      -Dsonar.projectKey="${SONAR_PROJECT_KEY}" \
      -Dsonar.projectBaseDir="$(pwd)" \
      -Dsonar.sources=core,mac \
      -Dsonar.exclusions="**/target/**,**/build/**,**/.git/**" \
      -Dsonar.host.url="${SONAR_HOST_URL}" \
      -Dsonar.token="${SONAR_TOKEN}" \
      -Dsonar.rust.lcov.reportPaths=core/target/coverage/lcov.info
  elif command -v docker >/dev/null 2>&1; then
    docker run --rm \
      -e SONAR_HOST_URL \
      -e SONAR_TOKEN \
      -v "$(pwd):/usr/src" \
      sonarsource/sonar-scanner-cli:latest \
      -Dsonar.projectKey="${SONAR_PROJECT_KEY}" \
      -Dsonar.projectBaseDir=/usr/src \
      -Dsonar.sources=core,mac \
      -Dsonar.exclusions="**/target/**,**/build/**,**/.git/**" \
      -Dsonar.rust.lcov.reportPaths=core/target/coverage/lcov.info
  else
    echo "Need sonar-scanner or docker for sonar target."
    exit 1
  fi

# Common local preflight.
dev: fmt lint test

# PR-grade quality gate.
check: dev semgrep shellcheck audit coverage sonar

# Full validation incl. coverage output.
check-all: check coverage-html
