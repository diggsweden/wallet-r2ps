# Project Guidelines

## Language & Edition
- Use **Rust 2024 edition** (`edition = "2024"` in Cargo.toml)
- Prefer `use<'a>` precise capturing, `async fn` in traits, and other 2024 idioms

## Architecture: Ports & Adapters (Hexagonal)
- **Domain** (`src/domain/`): Pure business logic, no I/O, no framework deps
- **Application** (`src/application/`): Use cases / services, orchestrates domain
- **Ports** (`src/ports/`): Traits (interfaces) for inbound and outbound dependencies
- **Adapters** (`src/adapters/`): Concrete implementations (DB, HTTP, CLI, etc.)
- Dependencies always point **inward** — adapters depend on ports, never the reverse

## Domain-Driven Design
- Model the **Ubiquitous Language** from the domain — use domain terms in code
- Organize by **Bounded Contexts**, not technical layers
- Prefer **Value Objects** (newtype pattern) over primitives for domain concepts
- **Aggregates** own their invariants; enforce them in constructors / methods
- **Domain Events** for cross-aggregate communication
- No `pub` fields on domain entities — use constructors and methods

## General Rules
- Errors: use `thiserror` for domain errors, `anyhow` for application glue
- No `unwrap()` or `expect()` in domain or application layers
- Tests live next to the code (`#[cfg(test)]`) for unit tests; `tests/` for integration
```

---

### `.claude/` directory (optional but useful)

Claude Code also supports a `.claude/` folder for additional context:
```
.claude/
  commands/         # Custom slash commands
  settings.json     # Project-level settings
