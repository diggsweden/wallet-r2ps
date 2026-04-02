SHELL := /bin/bash

# HSM keytool defaults (variable names match .env / app config)
_HSM_LIB_HOMEBREW       := /opt/homebrew/lib/softhsm/libsofthsm2.so
_HSM_LIB_LINUX          := /usr/lib/softhsm/libsofthsm2.so
PKCS11_LIB              ?= $(or $(wildcard $(_HSM_LIB_HOMEBREW)),$(wildcard $(_HSM_LIB_LINUX)))
PKCS11_SLOT_TOKEN_LABEL ?= wallet-keys
PKCS11_USER_PIN         ?= 123456
PKCS11_WRAP_KEY_ALIAS   ?= hsm-wrap-key
HSM_ROOT_KEY_LABEL      ?= rk-$(shell date +%Y%m)
JWS_DOMAIN_SEPARATOR    ?= $(HSM_ROOT_KEY_LABEL)_jws-$(shell date +%Y%m)
OPAQUE_DOMAIN_SEPARATOR ?= $(HSM_ROOT_KEY_LABEL)_opaque-$(shell date +%Y%m)

KEYTOOL_LOCAL   := ./target/debug/digg-hsm-keytool \
	--hsm-lib $(PKCS11_LIB) \
	--slot-token-label $(PKCS11_SLOT_TOKEN_LABEL) \
	--pin $(PKCS11_USER_PIN)
KEYTOOL_DOCKER  := docker compose -f ../docker-compose.yaml exec rust-r2ps-worker /usr/local/bin/digg-hsm-keytool \
	--hsm-lib $(PKCS11_LIB) \
	--slot-token-label $(PKCS11_SLOT_TOKEN_LABEL) \
	--pin $(PKCS11_USER_PIN)
KEYTOOL         := $(if $(DOCKER),$(KEYTOOL_DOCKER),$(KEYTOOL_LOCAL))

.PHONY: generate_openapi docs clean help check \
	hsm-create-wrapping-key hsm-create-root-key hsm-derive-public-keys hsm-setup hsm-status hsm-remove

## Generate openapi.json from domain model
generate_openapi: openapi.json

openapi.json: src/lib.rs src/domain/**/*.rs src/generate_openapi.rs Cargo.toml
	cargo run --bin generate_openapi --features="openapi"> openapi.json

## Build mdbook documentation from OpenAPI schema
docs: docs/book/book/index.html

docs/book/book/index.html: openapi.json docs/openapi_to_markdown.py docs/book/src/introduction.md docs/book/src/api-reference/README.md
	@mkdir -p docs/book/src/api-reference
	python3 docs/openapi_to_markdown.py openapi.json docs/book/src/api-reference
	cd docs/book && mdbook build

## Remove generated artifacts
clean:
	rm -f openapi.json
	rm -rf docs/book/book
	@find docs/book/src/api-reference -type f -name '*.md' ! -name 'README.md' -delete

## Display available targets
help:
	@echo "Available targets:"
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/^## //' | while IFS= read -r desc; do \
		read -r target < <(grep -A1 "^## $${desc}$$" $(MAKEFILE_LIST) | tail -1); \
		printf "  \033[36m%-20s\033[0m %s\n" "$${target%%:*}" "$$desc"; \
	done

## format, build, test and run clippy
check:
	@set -e; \
	run() { \
		name="$$1"; shift; \
		tmp="$$(mktemp)"; \
		if "$$@" >"$$tmp" 2>&1; then \
			printf "\033[32m✓ %s\033[0m\n" "$$name"; \
		else \
			cat "$$tmp"; \
			rm -f "$$tmp"; \
			exit 1; \
		fi; \
		rm -f "$$tmp"; \
	}; \
	run fmt cargo fmt; \
	run test cargo test -q; \
	run clippy cargo clippy -q --all-targets -- -D warnings;

## Create the AES wrapping key (run once when setting up a new HSM slot)
hsm-create-wrapping-key:
	$(KEYTOOL) create-wrapping-key --label $(PKCS11_WRAP_KEY_ALIAS)

## Create a new P-256 root derivation key and verify it works. Override HSM_ROOT_KEY_LABEL to pin it, e.g. HSM_ROOT_KEY_LABEL=rk-202601
hsm-create-root-key:
	$(KEYTOOL) create-root-key \
		--label $(HSM_ROOT_KEY_LABEL) \
		--test-domain-sep $(JWS_DOMAIN_SEPARATOR)

## Print the JWS and OPAQUE public keys to wire into clients. Use the same HSM_ROOT_KEY_LABEL as hsm-create-root-key
hsm-derive-public-keys:
	$(KEYTOOL) derive-public-keys \
		--root-key-label $(HSM_ROOT_KEY_LABEL) \
		--jws-domain-sep $(JWS_DOMAIN_SEPARATOR) \
		--opaque-domain-sep $(OPAQUE_DOMAIN_SEPARATOR)

## Remove wrapping key and root key (dry-run by default; add FORCE=1 to actually delete)
hsm-remove:
	$(KEYTOOL) remove \
		--wrap-key-label $(PKCS11_WRAP_KEY_ALIAS) \
		--root-key-label $(HSM_ROOT_KEY_LABEL) \
		$(if $(FORCE),--force,)

## Check key presence and run functional round-trips (wrap/sign + derivation/sign). Add VERBOSE=1 to print KIDs.
hsm-status:
	$(KEYTOOL) status \
		--wrap-key-label $(PKCS11_WRAP_KEY_ALIAS) \
		--root-key-label $(HSM_ROOT_KEY_LABEL) \
		--domain-sep $(JWS_DOMAIN_SEPARATOR) \
		$(if $(VERBOSE),--verbose,)

## Full first-time HSM setup: create wrapping key + root key + print public keys
hsm-setup: hsm-create-wrapping-key hsm-create-root-key hsm-derive-public-keys
