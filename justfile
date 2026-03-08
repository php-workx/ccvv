set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set dotenv-load

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
build:
  cd mac && ./build.sh

# Install latest built mac app bundle (kills running app first, relaunches after).
install:
  pkill -x ccvv || true
  cd mac && ./build.sh
  rm -rf /Applications/ccvv.app
  ditto build/ccvv.app /Applications/ccvv.app
  open /Applications/ccvv.app

# Static code security scan (Semgrep).
semgrep:
  #!/usr/bin/env bash
  set -euo pipefail
  command -v semgrep >/dev/null 2>&1 || { echo "semgrep not found. Install: brew install semgrep"; exit 1; }
  semgrep scan --config auto --error core mac

# Shell script linting.
shellcheck:
  #!/usr/bin/env bash
  set -euo pipefail
  command -v shellcheck >/dev/null 2>&1 || { echo "shellcheck not found. Install: brew install shellcheck"; exit 1; }
  mapfile -t scripts < <(find . -name '*.sh' -not -path '*/target/*' -not -path '*/.git/*')
  if [ ${#scripts[@]} -eq 0 ]; then
    echo "No shell scripts found."
    exit 0
  fi
  shellcheck -- "${scripts[@]}"

# Dependency vulnerability audit.
audit:
  #!/usr/bin/env bash
  set -euo pipefail
  command -v cargo-audit >/dev/null 2>&1 || { echo "cargo-audit not found. Install: cargo install cargo-audit"; exit 1; }
  cd core && cargo audit

# Generate LCOV coverage report.
coverage:
  #!/usr/bin/env bash
  set -euo pipefail
  command -v cargo-llvm-cov >/dev/null 2>&1 || { echo "cargo-llvm-cov not found. Install: cargo install cargo-llvm-cov"; exit 1; }
  cd core && mkdir -p target/coverage && cargo llvm-cov --workspace --lcov --output-path target/coverage/lcov.info

# Generate HTML coverage report.
coverage-html:
  #!/usr/bin/env bash
  set -euo pipefail
  command -v cargo-llvm-cov >/dev/null 2>&1 || { echo "cargo-llvm-cov not found. Install: cargo install cargo-llvm-cov"; exit 1; }
  cd core && cargo llvm-cov --workspace --html --output-dir target/coverage/html

# Run SonarQube: clippy report → scan → terminal report → quality gate (fails if gate doesn't pass).
sonar:
  #!/usr/bin/env bash
  set -euo pipefail
  SONAR_URL="http://localhost:9000"
  PROJECT_KEY="ccvv"
  TOKEN="${SONAR_TOKEN:-}"
  if [ -z "$TOKEN" ]; then
    echo "Set SONAR_TOKEN (run: just sonar-setup)"
    exit 1
  fi
  AUTH=(-H "Authorization: Bearer $TOKEN")

  # 1. Generate clippy JSON report
  printf '=== Clippy Report ===\n'
  cd core && cargo clippy --workspace --all-targets --message-format=json 2>/dev/null \
    | jq -s '[.[] | select(.reason == "compiler-message")]' > target/clippy-report.json || true
  cd ..

  # 2. Run sonar-scanner
  printf '\n=== SonarQube Scan ===\n'
  if command -v sonar-scanner >/dev/null 2>&1; then
    sonar-scanner -Dsonar.token="$TOKEN"
  elif command -v docker >/dev/null 2>&1; then
    docker run --rm -e SONAR_TOKEN -v "$(pwd):/usr/src" \
      sonarsource/sonar-scanner-cli:latest -Dsonar.token="$TOKEN"
  else
    echo "Need sonar-scanner or docker."
    exit 1
  fi

  # 3. Report
  printf '\n=== Quality Gate ===\n'
  QG=$(curl -sf "${AUTH[@]}" "$SONAR_URL/api/qualitygates/project_status?projectKey=$PROJECT_KEY")
  STATUS=$(echo "$QG" | jq -r '.projectStatus.status')
  if [ "$STATUS" = "OK" ]; then
    printf '  Status: ✅ PASSED\n'
  elif [ "$STATUS" = "ERROR" ]; then
    printf '  Status: ❌ FAILED\n'
    echo "$QG" | jq -r '.projectStatus.conditions[] | select(.status == "ERROR") | "  ⚠ \(.metricKey): \(.actualValue) (threshold: \(.errorThreshold))"'
  else
    printf '  Status: ⚠️  %s\n' "$STATUS"
  fi

  printf '\n=== Metrics ===\n'
  METRICS="bugs,vulnerabilities,code_smells,coverage,duplicated_lines_density,ncloc"
  curl -sf "${AUTH[@]}" "$SONAR_URL/api/measures/component?component=$PROJECT_KEY&metricKeys=$METRICS" \
    | jq -r '.component.measures[] | "  \(.metric): \(.value)"'

  printf '\n=== Open Issues (top 15) ===\n'
  ISSUES=$(curl -sf "${AUTH[@]}" "$SONAR_URL/api/issues/search?componentKeys=$PROJECT_KEY&statuses=OPEN,CONFIRMED&ps=15&s=SEVERITY&asc=false")
  TOTAL=$(echo "$ISSUES" | jq '.total')
  printf '  Total open: %s\n\n' "$TOTAL"
  echo "$ISSUES" | jq -r '.issues[] | "  \(.severity | ascii_downgrade) | \(.component | split(":")[1] // .component):\(.line // "?") | \(.message | .[0:100])"' 2>/dev/null || \
  echo "$ISSUES" | jq -r '.issues[] | "  \(.severity) | \(.component | split(":")[1] // .component):\(.line // "?") | \(.message | .[0:100])"'

  printf '\n=== Security Hotspots ===\n'
  HOTSPOTS=$(curl -sf "${AUTH[@]}" "$SONAR_URL/api/hotspots/search?projectKey=$PROJECT_KEY&ps=10")
  HS_TOTAL=$(echo "$HOTSPOTS" | jq '.paging.total')
  printf '  Total: %s\n' "$HS_TOTAL"
  if [ "$HS_TOTAL" -gt 0 ] 2>/dev/null; then
    echo "$HOTSPOTS" | jq -r '.hotspots[] | "  \(.vulnerabilityProbability) | \(.component | split(":")[1] // .component):\(.line // "?") | \(.message | .[0:100])"'
  fi

  printf '\n  Full report: %s/dashboard?id=%s\n' "$SONAR_URL" "$PROJECT_KEY"

  # 4. Gate — exit non-zero if quality gate failed
  if [ "$STATUS" != "OK" ]; then
    exit 1
  fi

# Provision SonarQube: wait for server, create project, generate token, configure new code period.
sonar-setup:
  #!/usr/bin/env bash
  set -euo pipefail
  SONAR_URL="http://localhost:9000"
  PROJECT_KEY="ccvv"
  printf "Waiting for SonarQube at %s ..." "$SONAR_URL"
  for i in $(seq 1 30); do
    if curl -sf "$SONAR_URL/api/system/status" | grep -q '"status":"UP"'; then
      printf " ready.\n"
      break
    fi
    printf "."
    sleep 2
    if [ "$i" -eq 30 ]; then
      printf "\nSonarQube not reachable at %s after 60s. Start it with: docker run -d -p 9000:9000 sonarqube:community\n" "$SONAR_URL"
      exit 1
    fi
  done
  curl -sf -u admin:admin -X POST "$SONAR_URL/api/projects/create?name=$PROJECT_KEY&project=$PROJECT_KEY" \
    -o /dev/null -w '' || true
  curl -sf -u admin:admin -X POST "$SONAR_URL/api/user_tokens/revoke?name=ccvv-local" -o /dev/null || true
  TOKEN=$(curl -sf -u admin:admin -X POST "$SONAR_URL/api/user_tokens/generate?name=ccvv-local" \
    | grep -o '"token":"[^"]*"' | cut -d'"' -f4)
  if [ -z "$TOKEN" ]; then
    echo "Failed to generate token. Check SonarQube credentials (default: admin/admin)."
    exit 1
  fi
  if [ -f .env ]; then
    grep -v '^SONAR_TOKEN=' .env > .env.tmp || true
    mv .env.tmp .env
  fi
  echo "SONAR_TOKEN=$TOKEN" >> .env
  curl -sf -H "Authorization: Bearer $TOKEN" -X POST \
    "$SONAR_URL/api/new_code_periods/set?project=$PROJECT_KEY&type=REFERENCE_BRANCH&value=main"
  echo "Done. Token written to .env, new code period set to main. Run: just sonar"

# Common local preflight.
dev: fmt lint test

# PR-grade quality gate.
check: dev semgrep shellcheck audit coverage sonar

# Full validation incl. coverage output.
check-all: check coverage-html
