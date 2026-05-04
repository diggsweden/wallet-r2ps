# SPDX-FileCopyrightText: 2025 Digg - Agency for Digital Government
#
# SPDX-License-Identifier: CC0-1.0

# Quality checks and automation for wallet-r2ps
# Run 'just' to see available commands

devtools_repo := env("DEVBASE_CHECK_REPO", "https://github.com/diggsweden/devbase-check")
devtools_dir := env("XDG_DATA_HOME", env("HOME") + "/.local/share") + "/devbase-check"
lint := devtools_dir + "/linters"
rust_lint := devtools_dir + "/linters/rust"
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

# ▪ Setup devtools (clone on first run, then delegate to devbase-check)
[group('setup')]
setup-devtools:
    @[ -d "{{devtools_dir}}" ] || { mkdir -p "$(dirname "{{devtools_dir}}")" && git clone --depth 1 "{{devtools_repo}}" "{{devtools_dir}}"; }
    @"{{devtools_dir}}/scripts/setup.sh" "{{devtools_repo}}" "{{devtools_dir}}"

# ▪ Force-update devtools to latest release tag (or --ref <branch/tag/sha>)
[group('setup')]
update-devtools *ARGS:
    @"{{devtools_dir}}/scripts/update.sh" "{{devtools_dir}}" {{ ARGS }}

# Check required tools are installed
[group('setup')]
check-tools: _ensure-devtools
    @{{devtools_dir}}/scripts/check-tools.sh --check-devtools mise git just cargo rumdl yamlfmt actionlint gitleaks shellcheck shfmt gommitlint reuse hadolint

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
    @just cargo-audit
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

# Run all Rust linters (clippy; rustfmt check is a separate recipe)
[group('lint')]
lint-rust: _rust-toolchain-ready
    @{{rust_lint}}/lint.sh

# Run cargo clippy only (alternative entry point parallel to lint-rust)
[group('lint')]
lint-rust-clippy: _rust-toolchain-ready
    @{{rust_lint}}/clippy.sh

# Check Rust formatting (delegates to devbase-check; uses --all for workspace)
[group('lint')]
lint-rust-fmt: _rust-toolchain-ready
    @{{rust_lint}}/format.sh check

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
lint-rust-fmt-fix: _rust-toolchain-ready
    @{{rust_lint}}/format.sh fix

# ==================================================================================== #
# SECURITY - Dependency auditing
# ==================================================================================== #

# Audit crate dependencies. Named `cargo-audit` so verify.sh skips it —
# audit is slow and shouldn't gate `just lint-all`.
[group('security')]
cargo-audit: _rust-toolchain-ready
    @{{rust_lint}}/audit.sh

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

# Run testcontainer integration tests (requires Docker)
[group('test')]
test-containers:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    cargo test --workspace --exclude integration-load-tests --features hsm-worker/testcontainers,wallet-bff/testcontainers -- --test-threads=1 2>&1
    just_success "All testcontainer tests are passing"

# Run all tests (requires Docker and SoftHSM)
[group('test')]
test-full:
    #!/usr/bin/env bash
    set -euo pipefail
    source "{{colors}}"
    cargo test --workspace --exclude integration-load-tests --features hsm-worker/testcontainers,wallet-bff/testcontainers -- --include-ignored --test-threads=1 2>&1
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

# mise's core:rust skips rust-toolchain.toml's `components` — add them.
[private]
_rust-toolchain-ready:
    @rustup component add clippy rustfmt 2>/dev/null || true
