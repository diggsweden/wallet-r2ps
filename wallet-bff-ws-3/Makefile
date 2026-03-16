# ============================================================================
# wallet-bff-ws Makefile
# ============================================================================
#
# Build, run, test, containerize, and lint the wallet-bff-ws service.
#
# Quick reference:
#   make build              Build debug binary
#   make build-release      Build release binary
#   make run                Run locally (debug)
#   make test               Run tests
#   make docker-build       Build release container image
#   make docker-scan        Scan image for vulnerabilities (Trivy)
#   make lint-api           Full Swedish REST API Profile lint workflow
#   make help               Show all available targets
#
# ============================================================================

# -- Configuration -----------------------------------------------------------

# Cargo
CARGO               ?= cargo
RUST_LOG            ?= info

# Server
SERVER_HOST         ?= 0.0.0.0
SERVER_PORT         ?= 8088
CONTEXT_PATH        ?= /r2ps-api/v1
BASE_URL            := http://$(SERVER_HOST):$(SERVER_PORT)$(CONTEXT_PATH)
OPENAPI_URL         := $(BASE_URL)/openapi.json
HEALTH_URL          := $(BASE_URL)/health

# Container image
DOCKER              ?= docker
IMAGE_NAME          ?= wallet-bff-ws
VERSION             := $(shell grep '^version' Cargo.toml | head -n1 | sed 's/.*"\(.*\)".*/\1/')
GIT_COMMIT          := $(shell git rev-parse --short HEAD 2>/dev/null || echo "dev")
IMAGE_TAG           := $(GIT_COMMIT)
FULL_IMAGE          := $(IMAGE_NAME):$(IMAGE_TAG)
CONTAINER_REGISTRY  ?=
PLATFORMS           ?= linux/amd64,linux/arm64

# Trivy
TRIVY_IMAGE         ?= aquasec/trivy:latest
TRIVY_SEVERITY      ?= CRITICAL,HIGH

# RAP-LP (Swedish REST API Profile linting)
RAPLP_VERSION       ?= v1.1.1
RAPLP_IMAGE         := ghcr.io/diggsweden/rest-api-profil-lint-processor:$(RAPLP_VERSION)

# Generated / temporary files
OPENAPI_FILE        := openapi.json
LOG_FILE            := raplp.log
DIAGNOSTIC_FILE     := raplp-diagnostic.json
EXCEL_REPORT        := Avstamning_REST_API_profil.xlsx
SERVER_PID_FILE     := .server.pid
SERVER_LOG          := server.log

# Lint options
CATEGORIES          ?=
WAIT_TIMEOUT        ?= 30

# ============================================================================
# Phony targets
# ============================================================================

.PHONY: build build-release run run-release test clippy fmt fmt-check clean \
        docker-build docker-build-debug docker-build-all \
        docker-buildx docker-buildx-debug \
        docker-scan docker-tag-version docker-push \
        docker-run docker-stop show-image-info \
        lint-api lint-quick lint-categories export-openapi \
        start-server stop-server wait-for-server \
        check-rust check-docker check-lint-deps check-lint-image \
        docker-login clean-lint help

# ============================================================================
# Local Build & Development
# ============================================================================

## Build the application (debug profile)
build:
	@echo "Building debug binary..."
	$(CARGO) build
	@echo "Built: target/debug/wallet-bff-ws"

## Build the application (release profile — optimized)
build-release:
	@echo "Building release binary..."
	$(CARGO) build --release
	@echo "Built: target/release/wallet-bff-ws"

## Run the application locally (debug)
run:
	RUST_LOG=$(RUST_LOG) $(CARGO) run

## Run the application locally (release)
run-release:
	RUST_LOG=$(RUST_LOG) $(CARGO) run --release

## Run all tests
test:
	$(CARGO) test

## Run Clippy linter
clippy:
	$(CARGO) clippy -- -D warnings

## Format code with rustfmt
fmt:
	$(CARGO) fmt

## Check formatting without modifying files
fmt-check:
	$(CARGO) fmt -- --check

## Clean build artifacts and temporary files
clean:
	$(CARGO) clean
	rm -f $(SERVER_PID_FILE) $(SERVER_LOG)

# ============================================================================
# Container Image — Single-platform (native)
# ============================================================================

## Show image build metadata
show-image-info:
	@echo "Image:   $(FULL_IMAGE)"
	@echo "Version: $(VERSION)"
	@echo "Commit:  $(GIT_COMMIT)"

## Build container image (release)
docker-build: check-docker
	$(DOCKER) build \
		--target runtime-release \
		-t $(FULL_IMAGE) \
		-t $(IMAGE_NAME):latest \
		-f Containerfile .
	@echo ""
	@echo "Tagged: $(FULL_IMAGE), $(IMAGE_NAME):latest"

## Build container image (debug)
docker-build-debug: check-docker
	$(DOCKER) build \
		--target runtime-debug \
		-t $(IMAGE_NAME):$(IMAGE_TAG)-debug \
		-t $(IMAGE_NAME):debug \
		-f Containerfile .
	@echo ""
	@echo "Tagged: $(IMAGE_NAME):$(IMAGE_TAG)-debug, $(IMAGE_NAME):debug"

## Build both debug and release images
docker-build-all: docker-build docker-build-debug

# ============================================================================
# Container Image — Multi-architecture (buildx)
# ============================================================================

## Build and load multi-arch release image (local only, current platform)
docker-buildx: check-docker
	$(DOCKER) buildx build \
		--platform $(PLATFORMS) \
		--target runtime-release \
		-t $(FULL_IMAGE) \
		-t $(IMAGE_NAME):latest \
		-f Containerfile .
	@echo ""
	@echo "Multi-arch build: $(PLATFORMS)"
	@echo "Tagged: $(FULL_IMAGE), $(IMAGE_NAME):latest"

## Build and load multi-arch debug image (local only, current platform)
docker-buildx-debug: check-docker
	$(DOCKER) buildx build \
		--platform $(PLATFORMS) \
		--target runtime-debug \
		-t $(IMAGE_NAME):$(IMAGE_TAG)-debug \
		-t $(IMAGE_NAME):debug \
		-f Containerfile .

# ============================================================================
# Container — Tag, Push, Scan
# ============================================================================

## Tag image with Cargo.toml version
docker-tag-version: check-docker
	$(DOCKER) tag $(FULL_IMAGE) $(IMAGE_NAME):$(VERSION)
	@echo "Tagged: $(IMAGE_NAME):$(VERSION)"

## Push image to registry (set CONTAINER_REGISTRY)
docker-push: check-docker
	@if [ -z "$(CONTAINER_REGISTRY)" ]; then \
		echo "ERROR: CONTAINER_REGISTRY not set."; \
		echo "Usage: make docker-push CONTAINER_REGISTRY=ghcr.io/diggsweden"; \
		exit 1; \
	fi
	$(DOCKER) tag $(FULL_IMAGE) $(CONTAINER_REGISTRY)/$(FULL_IMAGE)
	$(DOCKER) push $(CONTAINER_REGISTRY)/$(FULL_IMAGE)
	$(DOCKER) tag $(FULL_IMAGE) $(CONTAINER_REGISTRY)/$(IMAGE_NAME):$(VERSION)
	$(DOCKER) push $(CONTAINER_REGISTRY)/$(IMAGE_NAME):$(VERSION)
	@echo ""
	@echo "Pushed:"
	@echo "  $(CONTAINER_REGISTRY)/$(FULL_IMAGE)"
	@echo "  $(CONTAINER_REGISTRY)/$(IMAGE_NAME):$(VERSION)"

## Scan container image for vulnerabilities with Trivy
docker-scan: check-docker
	@echo "Scanning $(FULL_IMAGE) for vulnerabilities..."
	$(DOCKER) run --rm \
		-v /var/run/docker.sock:/var/run/docker.sock \
		$(TRIVY_IMAGE) \
		image --severity $(TRIVY_SEVERITY) $(FULL_IMAGE)

## Run container (requires Redis & Kafka accessible on host network)
docker-run: check-docker
	@echo "Starting $(FULL_IMAGE) on host network..."
	@echo "Configure via environment variables (see .env for reference)."
	$(DOCKER) run --rm \
		--name $(IMAGE_NAME) \
		-p $(SERVER_PORT):$(SERVER_PORT) \
		--env-file .env \
		--network host \
		$(FULL_IMAGE)

## Stop running container
docker-stop: check-docker
	$(DOCKER) stop $(IMAGE_NAME) 2>/dev/null || true

# ============================================================================
# Swedish REST API Profile Linting (RAP-LP)
# ============================================================================

## Full lint workflow: start server, export OpenAPI spec, lint, stop
lint-api: check-lint-deps check-lint-image
	@$(MAKE) --no-print-directory start-server
	@trap '$(MAKE) --no-print-directory stop-server' EXIT; \
	$(MAKE) --no-print-directory wait-for-server && \
	$(MAKE) --no-print-directory export-openapi && \
	$(MAKE) --no-print-directory _run-lint-all

## Lint only (assumes server is already running)
lint-quick: check-lint-deps check-lint-image export-openapi _run-lint-all

## Lint specific categories (CATEGORIES=DokRules,SakRules)
lint-categories: check-lint-deps check-lint-image export-openapi
ifndef CATEGORIES
	$(error CATEGORIES is required. Example: make lint-categories CATEGORIES=DokRules,SakRules)
endif
	@echo "Running RAP-LP with categories: $(CATEGORIES)"
	@$(DOCKER) run --rm \
		-v "$$(pwd)":/data \
		$(RAPLP_IMAGE) \
		-f /data/$(OPENAPI_FILE) \
		-c $(CATEGORIES)

## Export OpenAPI specification from the running server
export-openapi:
	@echo "Exporting OpenAPI spec from $(OPENAPI_URL)..."
	@curl -sf $(OPENAPI_URL) | jq '.' > $(OPENAPI_FILE)
	@echo "Exported to $(OPENAPI_FILE) ($$(wc -c < $(OPENAPI_FILE) | tr -d ' ') bytes)"

## Remove generated lint output files
clean-lint:
	rm -f $(OPENAPI_FILE) $(LOG_FILE) $(DIAGNOSTIC_FILE) $(EXCEL_REPORT) \
	      $(SERVER_PID_FILE) $(SERVER_LOG)
	@echo "Cleaned lint output files"

# ============================================================================
# Server management (used by lint targets)
# ============================================================================

start-server:
	@if [ -f $(SERVER_PID_FILE) ] && kill -0 $$(cat $(SERVER_PID_FILE)) 2>/dev/null; then \
		echo "Server already running (PID: $$(cat $(SERVER_PID_FILE)))"; \
	else \
		echo "Starting server..."; \
		$(CARGO) run --bin wallet-bff-ws > $(SERVER_LOG) 2>&1 & echo $$! > $(SERVER_PID_FILE); \
		echo "Server started (PID: $$(cat $(SERVER_PID_FILE)))"; \
	fi

stop-server:
	@if [ -f $(SERVER_PID_FILE) ]; then \
		PID=$$(cat $(SERVER_PID_FILE)); \
		if kill -0 $$PID 2>/dev/null; then \
			echo "Stopping server (PID: $$PID)..."; \
			kill $$PID 2>/dev/null || true; \
			sleep 1; \
			kill -0 $$PID 2>/dev/null && kill -9 $$PID 2>/dev/null || true; \
		fi; \
		rm -f $(SERVER_PID_FILE); \
	fi

wait-for-server:
	@echo "Waiting for server at $(HEALTH_URL)..."
	@i=0; while [ $$i -lt $(WAIT_TIMEOUT) ]; do \
		if curl -sf $(HEALTH_URL) > /dev/null 2>&1; then \
			echo "Server is ready"; \
			exit 0; \
		fi; \
		i=$$((i + 1)); \
		sleep 1; \
	done; \
	echo "ERROR: Server did not start within $(WAIT_TIMEOUT)s. Check $(SERVER_LOG)"; \
	exit 1

# Internal: run all RAP-LP rule categories
_run-lint-all:
	@echo "Running Swedish REST API Profile linting ($(RAPLP_VERSION))..."
	@echo "------------------------------------------------------------"
	@$(DOCKER) run --rm \
		-v "$$(pwd)":/data \
		$(RAPLP_IMAGE) \
		-f /data/$(OPENAPI_FILE) \
		-l /data/$(LOG_FILE) \
		-d /data/$(DIAGNOSTIC_FILE) \
		--dex /data/$(EXCEL_REPORT); \
	EXIT_CODE=$$?; \
	echo "------------------------------------------------------------"; \
	echo "Output files:"; \
	echo "  Log:        $(LOG_FILE)"; \
	echo "  Diagnostic: $(DIAGNOSTIC_FILE)"; \
	echo "  Excel:      $(EXCEL_REPORT)"; \
	exit $$EXIT_CODE

# ============================================================================
# Prerequisite checks
# ============================================================================

check-rust:
	@command -v $(CARGO) > /dev/null 2>&1 || \
		{ echo "ERROR: cargo not found. Install Rust: https://rustup.rs"; exit 1; }

check-docker:
	@command -v $(DOCKER) > /dev/null 2>&1 || \
		{ echo "ERROR: $(DOCKER) not found. Install Docker or set DOCKER=podman"; exit 1; }

check-lint-deps:
	@command -v $(DOCKER) > /dev/null 2>&1 || \
		{ echo "ERROR: $(DOCKER) not found."; exit 1; }
	@command -v curl > /dev/null 2>&1 || \
		{ echo "ERROR: curl not found"; exit 1; }
	@command -v jq > /dev/null 2>&1 || \
		{ echo "ERROR: jq not found. Install with: brew install jq"; exit 1; }

check-lint-image:
	@$(DOCKER) image inspect $(RAPLP_IMAGE) > /dev/null 2>&1 || \
	{ echo "Pulling $(RAPLP_IMAGE)..."; \
	  $(DOCKER) pull $(RAPLP_IMAGE) 2>/dev/null || \
	  { echo ""; \
	    echo "ERROR: Failed to pull $(RAPLP_IMAGE)"; \
	    echo "The GHCR registry requires authentication."; \
	    echo "Run: make docker-login"; \
	    exit 1; }; }

## Authenticate to GitHub Container Registry
docker-login:
	@echo "Authenticating to ghcr.io..."
	@echo "You need a GitHub PAT with read:packages scope."
	@echo "Create one at: https://github.com/settings/tokens"
	@echo ""
	@$(DOCKER) login ghcr.io

# ============================================================================
# Help
# ============================================================================

help:
	@echo "wallet-bff-ws"
	@echo "============="
	@echo ""
	@echo "Build & Development:"
	@echo "  build              Build debug binary"
	@echo "  build-release      Build release binary (optimized)"
	@echo "  run                Run application (debug)"
	@echo "  run-release        Run application (release)"
	@echo "  test               Run tests"
	@echo "  clippy             Run Clippy linter"
	@echo "  fmt                Format code with rustfmt"
	@echo "  fmt-check          Check code formatting"
	@echo "  clean              Clean build artifacts"
	@echo ""
	@echo "Container Image:"
	@echo "  docker-build       Build release image       $(FULL_IMAGE)"
	@echo "  docker-build-debug Build debug image         $(IMAGE_NAME):$(IMAGE_TAG)-debug"
	@echo "  docker-build-all   Build both images"
	@echo "  docker-buildx      Multi-arch release build  ($(PLATFORMS))"
	@echo "  docker-buildx-debug Multi-arch debug build"
	@echo "  docker-tag-version Tag with Cargo version    $(IMAGE_NAME):$(VERSION)"
	@echo "  docker-scan        Scan image with Trivy"
	@echo "  docker-push        Push to registry          (set CONTAINER_REGISTRY)"
	@echo "  docker-run         Run container (host network)"
	@echo "  docker-stop        Stop running container"
	@echo "  show-image-info    Display image metadata"
	@echo ""
	@echo "Swedish REST API Profile Linting:"
	@echo "  lint-api           Full lint workflow (start, export, lint, stop)"
	@echo "  lint-quick         Lint only (server must be running)"
	@echo "  lint-categories    Lint specific categories  (set CATEGORIES)"
	@echo "  export-openapi     Export OpenAPI spec from running server"
	@echo "  docker-login       Authenticate to ghcr.io"
	@echo "  clean-lint         Remove generated lint files"
	@echo ""
	@echo "Variables (override with make VAR=value):"
	@echo "  DOCKER             Container runtime         ($(DOCKER))"
	@echo "  RUST_LOG           Log level                 ($(RUST_LOG))"
	@echo "  IMAGE_NAME         Image name                ($(IMAGE_NAME))"
	@echo "  CONTAINER_REGISTRY Push target registry      ($(or $(CONTAINER_REGISTRY),<not set>))"
	@echo "  PLATFORMS          Buildx platforms           ($(PLATFORMS))"
	@echo "  TRIVY_SEVERITY     Trivy severity filter     ($(TRIVY_SEVERITY))"
	@echo "  RAPLP_VERSION      RAP-LP version            ($(RAPLP_VERSION))"
	@echo "  CATEGORIES         Lint rule categories      (all)"
	@echo ""
	@echo "Examples:"
	@echo "  make build && make run"
	@echo "  make docker-build && make docker-scan"
	@echo "  make docker-push CONTAINER_REGISTRY=ghcr.io/diggsweden"
	@echo "  make docker-buildx PLATFORMS=linux/amd64"
	@echo "  make lint-api"
