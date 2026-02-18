# Repository Guidelines

## Project Structure & Module Organization
`bop` is a Rust CLI project. `src/main.rs` is the command entrypoint, and `src/lib.rs` exposes core modules. Keep new code in the matching domain folder: `src/detect` (hardware probing), `src/audit` (findings/scoring), `src/apply` (writes and persistence), `src/revert` (rollback), `src/monitor` (power telemetry), `src/profile` (hardware profiles), `src/wake` (ACPI wake controls), and `src/output` (table/JSON rendering).

Tests live under `tests/` (current integration-style suite: `tests/sysfs_mock.rs`). Put reusable sample trees/data in `tests/fixtures/`. Do not commit `target/` artifacts.

## Build, Test, and Development Commands
- `cargo check`: fast compile validation during development.
- `cargo build --release`: build optimized binary at `target/release/bop`.
- `cargo run -- audit`: run the CLI locally without installing.
- `cargo test`: run all tests.
- `cargo fmt --all`: apply standard Rust formatting.
- `cargo clippy --all-targets --all-features -- -D warnings`: run lints as errors.
- `cargo install --path .`: install the local binary for manual end-to-end checks.

## Coding Style & Naming Conventions
Use Rust 2024 + `rustfmt` defaults (4-space indentation, trailing commas where formatter applies). Follow standard Rust naming: `snake_case` for functions/files/modules, `PascalCase` for types/traits, `SCREAMING_SNAKE_CASE` for constants.

Prefer small, cohesive modules and explicit boundaries between detection, audit, and apply/revert logic. Use clear error messages for operations that require root or mutate system state.

## Testing Guidelines
Add tests for every behavior change, especially policy decisions and parser/sysfs handling. Name tests by scenario + expected behavior (example: `test_kernel_param_detection`).

Avoid tests that depend on the host machine’s live `/sys` or `/proc`; use temporary mock trees like `tests/sysfs_mock.rs`. There is no strict coverage gate today, but bug fixes should include a regression test.

## Commit & Pull Request Guidelines
Current history is small but uses descriptive, imperative commit subjects (example: `Initial implementation of bop - Battery Optimization Project`). Keep subjects concise and scoped when possible, e.g., `audit: flag disabled wifi powersave`.

PRs should include:
- What changed and why.
- Risk notes for root-required or boot-persistent changes.
- Evidence: `cargo test` and `cargo clippy` results.
- Linked issue(s) and hardware assumptions (for example, Framework 16 AMD specifics).
