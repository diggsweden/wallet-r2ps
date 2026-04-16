# SPDX-FileCopyrightText: 2025 Digg - Agency for Digital Government
#
# SPDX-License-Identifier: CC0-1.0

# Quality checks and automation for wallet-r2ps
# Run 'just' to see available commands

devtools_repo := env("DEVBASE_CHECK_REPO", "https://github.com/diggsweden/devbase-check")
devtools_dir := env("XDG_DATA_HOME", env("HOME") + "/.local/share") + "/devbase-check"
lint := devtools_dir + "/linters"
colors := devtools_dir + "/utils/colors.sh"

# Rust crate directories
crates := "hsm-worker wallet-bff"

# Color variables
CYAN_BOLD := "\\033[1;36m"
GREEN := "\\033[1;32m"
BLUE := "\\033[1;34m"
NC := "\\033[0m"

# ==================================================================================== #
# DEFAULT - Show available recipes
# ==================================================================================== #

# Display available recipes
default:
    @printf "{{CYAN_BOLD}} wallet-r2ps{{NC}}\n\n"
    @printf "Quick start: {{GREEN}}just setup-devtools{{NC}} | {{BLUE}}just verify{{NC}}\n\n"
    @just --list --unsorted

# ==================================================================================== #
# SETUP - Development environment setup
# ==================================================================================== #

# ▪ Install devtools and tools
[group('setup')]
install: setup-devtools tools-install

# ▪ Setup devtools (clone or update)
[group('setup')]
setup-devtools:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -d "{{devtools_dir}}" ]]; then
        # setup.sh handles update checks with 1-hour cache
        if [[ -f "{{devtools_dir}}/scripts/setup.sh" ]]; then
            "{{devtools_dir}}/scripts/setup.sh" "{{devtools_repo}}" "{{devtools_dir}}"
        fi
    else
        printf "Cloning devbase-check to %s...\n" "{{devtools_dir}}"
        mkdir -p "$(dirname "{{devtools_dir}}")"
        git clone --depth 1 "{{devtools_repo}}" "{{devtools_dir}}"
        git -C "{{devtools_dir}}" fetch --tags --depth 1 --quiet
        latest=$(git -C "{{devtools_dir}}" describe --tags --abbrev=0 origin/main 2>/dev/null || echo "")
        if [[ -n "$latest" ]]; then
            git -C "{{devtools_dir}}" fetch --depth 1 origin tag "$latest" --quiet
            git -C "{{devtools_dir}}" checkout "$latest" --quiet
        fi
        printf "Installed devbase-check %s\n" "${latest:-main}"
    fi

# Check required tools are installed
[group('setup')]
check-tools: _ensure-devtools
    @{{devtools_dir}}/scripts/check-tools.sh --check-devtools mise git just cargo cargo-audit rumdl yamlfmt actionlint gitleaks shellcheck shfmt gommitlint reuse hadolint

# Install tools via mise
[group('setup')]
tools-install: _ensure-devtools
    @mise install

# ==================================================================================== #
# VERIFY - Quality assurance
# ==================================================================================== #

# ▪ Run all checks (linters + tests)
[group('verify')]
verify: _ensure-devtools check-tools
    @{{devtools_dir}}/scripts/verify.sh
    @just audit
    @just test

# ==================================================================================== #
# LINT - Code quality checks
# ==================================================================================== #

# ▪ Run all linters with summary
[group('lint')]
lint-all: _ensure-devtools
    @{{devtools_dir}}/scripts/verify.sh

# Validate version control
[group('lint')]
lint-version-control:
    @{{lint}}/version-control.sh

# Validate commit messages
[group('lint')]
lint-commits:
    @{{lint}}/commits.sh

# Scan for secrets
[group('lint')]
lint-secrets:
    @{{lint}}/secrets.sh

# Lint YAML files
[group('lint')]
lint-yaml:
    @{{lint}}/yaml.sh check

# Lint markdown files
[group('lint')]
lint-markdown:
    @{{lint}}/markdown.sh check "MD013,MD041,MD033"

# Lint shell scripts
[group('lint')]
lint-shell:
    @{{lint}}/shell.sh

# Check shell formatting
[group('lint')]
lint-shell-fmt:
    @{{lint}}/shell-fmt.sh check

# Lint GitHub Actions
[group('lint')]
lint-actions:
    @{{lint}}/github-actions.sh

# Check license compliance
[group('lint')]
lint-license:
    @{{lint}}/license.sh

# Lint containers
[group('lint')]
lint-container:
    @{{lint}}/container.sh

# Skip XML linting (no XML files in this Rust project)
[group('lint')]
lint-xml:
    @echo "ⓘ No XML files in this project — skipping"

# Run cargo clippy on all crates
[group('lint')]
lint-rust:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    just_header "Clippy" "cargo clippy"
    for crate in {{crates}}; do
        printf "  %s ... " "$crate"
        if cargo clippy --manifest-path "$crate/Cargo.toml" --all-targets -- -D warnings 2>&1 | tail -1; then
            printf "\033[32m✓\033[0m\n"
        else
            printf "\033[31m✗\033[0m\n"
            exit 1
        fi
    done
    just_success "Clippy passed"

# Check Rust formatting
[group('lint')]
lint-rust-fmt:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    just_header "Rust fmt check" "cargo fmt --check"
    for crate in {{crates}}; do
        printf "  %s ... " "$crate"
        if cargo fmt --check --manifest-path "$crate/Cargo.toml" 2>&1; then
            printf "\033[32m✓\033[0m\n"
        else
            printf "\033[31m✗\033[0m\n"
            exit 1
        fi
    done
    just_success "Rust formatting OK"

# ==================================================================================== #
# LINT-FIX - Auto-fix code issues
# ==================================================================================== #

# ▪ Fix all auto-fixable issues
[group('lint-fix')]
lint-fix: _ensure-devtools lint-yaml-fix lint-markdown-fix lint-shell-fmt-fix lint-rust-fmt-fix
    #!/usr/bin/env bash
    source "{{colors}}"
    just_success "All auto-fixes completed"

# Fix YAML formatting
[group('lint-fix')]
lint-yaml-fix:
    @{{lint}}/yaml.sh fix

# Fix markdown formatting
[group('lint-fix')]
lint-markdown-fix:
    @{{lint}}/markdown.sh fix "MD013,MD041,MD033"

# Fix shell formatting
[group('lint-fix')]
lint-shell-fmt-fix:
    @{{lint}}/shell-fmt.sh fix

# Fix Rust formatting
[group('lint-fix')]
lint-rust-fmt-fix:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    just_header "Rust fmt fix" "cargo fmt"
    for crate in {{crates}}; do
        printf "  %s ... " "$crate"
        cargo fmt --manifest-path "$crate/Cargo.toml"
        printf "\033[32m✓\033[0m\n"
    done
    just_success "Rust formatting fixed"

# ==================================================================================== #
# SECURITY - Dependency auditing
# ==================================================================================== #

# Audit crate dependencies for known vulnerabilities
[group('security')]
audit:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    just_header "Audit" "cargo audit"
    cargo audit -f Cargo.lock
    just_success "No known vulnerabilities"

# ==================================================================================== #
# TEST - Run tests
# ==================================================================================== #

# Run unit tests
[group('test')]
test:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    cargo test --workspace --exclude integration-load-tests --lib 2>&1
    just_success "All unit-tests are passing"


# Run unit tests and integration tests (no external dependencies)
[group('test')]
test-all:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    cargo test --workspace --exclude integration-load-tests 2>&1
    just_success "All unit and integration tests are passing"

# Run all tests (requires Docker and SoftHsm)
[group('test')]
test-full:
     #!/usr/bin/env bash
        set -euo pipefail
        source "{{colors}}"
        cargo test --workspace --exclude integration-load-tests -- --include-ignored --test-threads=1  2>&1
        just_success "The full test suite is passing"


# ==================================================================================== #
# BUILD - Build project
# ==================================================================================== #

# ▪ Build all crates (release mode)
[group('build')]
build:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    just_header "Building" "cargo build --release"
    for crate in {{crates}}; do
        printf "  %s ... " "$crate"
        cargo build --release --manifest-path "$crate/Cargo.toml"
        printf "\033[32m✓\033[0m\n"
    done
    just_success "Build completed"

# Clean build artifacts for all crates
[group('build')]
clean:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    just_header "Cleaning" "cargo clean"
    for crate in {{crates}}; do
        cargo clean --manifest-path "$crate/Cargo.toml"
    done
    just_success "Clean completed"

# ==================================================================================== #
# INTERNAL
# ==================================================================================== #

[private]
_ensure-devtools:
    @just setup-devtools
