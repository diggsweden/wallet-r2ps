SHELL := /bin/bash

.PHONY: generate_openapi docs clean help check

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
