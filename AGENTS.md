# Repository Guidelines

## Project Structure & Module Organization
Source lives in `src/` with modules: `gui.rs` for egui UI, `scanner.rs` for parallel walkdir, `database.rs` + `reference_loader.rs` for SQLite/CSV, `searcher.rs` + `matcher.rs` for fuzzy scoring, `opener.rs` for OS launchers, and `main.rs` wiring everything. Fixtures are in `test_data/` (via `create_test_files.ps1`) and `sample_ids.csv`; helper scripts (`build.sh`, `build.bat`, `test_runner.ps1`) stay at the root beside generated `target/` builds.

## Build, Test, and Development Commands
- `cargo run` – starts the GUI in dev mode with stdout logging.
- `cargo build --release` – optimized binary; same logic behind `./build.sh` (Linux/macOS) or `build.bat`.
- `cargo fmt --all` – enforces rustfmt before reviews.
- `cargo clippy --all-targets -- -D warnings` – static lint gate for CI parity.
- `cargo test --all` – runs unit/integration suites; add `-- --nocapture` when debugging.
- `pwsh ./test_runner.ps1` – Windows pre-flight that checks fixtures, binaries, and cache scaffolding.

## Coding Style & Naming Conventions
Follow Rust 2021 defaults: 4-space indents, `snake_case` functions/modules, `CamelCase` types, `SCREAMING_SNAKE_CASE` consts. Keep GUI state small and pass shared services (`Scanner`, `Database`) via structs instead of globals. Prefer Result-returning helpers over `unwrap`, and keep blocking I/O inside `database.rs` or `scanner.rs` so `gui.rs` stays responsive.

## Testing Guidelines
Place fast unit tests beside each module in `mod tests`; integration-style checks can exercise real CSVs and TIFFs under `test_data/` (populate via `pwsh ./create_test_files.ps1`). Run `cargo test --all` before opening a PR, then `pwsh ./test_runner.ps1` on Windows to verify assets, binaries, and cache expectations. Update `TEST_EXECUTION_REPORT.md` whenever manual GUI walkthroughs are performed.

## Commit & Pull Request Guidelines
The repository has no published history yet, so adopt Conventional Commits (e.g., `feat: add fuzzy threshold slider`) for clarity. Keep commits focused and include rationale in the body when touching performance-sensitive code. Pull requests should link issues or tasks, list the commands run (`cargo test`, `cargo clippy`), and attach before/after screenshots or screen recordings for any GUI change.

## Operational Tips
Never commit `cache.db`, `target/`, or user-specific paths; rely on `.gitignore` and refresh temp assets locally. Reference CSVs must expose an `hh_id` column, otherwise `reference_loader.rs` will fail fast. Keep large TIFF sets outside the repo and point the GUI folder picker at them; document unusual OS-specific opener tweaks inside `opener.rs` comments.
