<!--
Thanks for contributing to Pico de Gallo! 🌶️

Please make sure your PR:
- Follows Conventional Commits (`feat(scope): …`, `fix(scope): …`, `chore(scope): …`).
  Scopes: internal, lib, hal, ffi, application, mcp, pyco, firmware, repo.
- Has commits that each build and pass CI on their own (no "fixup" commits — squash them locally first).
- Includes the `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>` trailer
  for any AI-assisted commits, plus an `Assisted-by:` trailer per CONTRIBUTING.md.
- Does NOT add a `Signed-off-by:` trailer on AI-assisted commits.

If this is a draft, mark it as such and CI will still run.
-->

## Summary

<!-- One or two sentences describing what this PR does and why. -->

## Affected component(s)

<!-- Tick all that apply. -->

- [ ] firmware (`pico-de-gallo-firmware`, no_std)
- [ ] wire protocol (`pico-de-gallo-internal`)
- [ ] host library (`pico-de-gallo-lib`)
- [ ] embedded-hal adapter (`pico-de-gallo-hal`)
- [ ] C FFI (`pico-de-gallo-ffi`)
- [ ] CLI application (`gallo` / `pico-de-gallo-app`)
- [ ] MCP server (`gallo-mcp` / `pico-de-gallo-mcp`)
- [ ] Python bindings (`pyco-de-gallo`)
- [ ] hardware (KiCad PCB / enclosure)
- [ ] documentation (book, README, rustdoc)
- [ ] CI / release tooling

## Related issues

<!-- e.g. "Closes #123", "Refs #456". Required for non-trivial changes. -->

## Wire-protocol impact

<!--
postcard serializes enums by variant index. Reordering variants or changing
request/response shapes is a BREAKING change and requires coordinated
firmware + host releases plus a `pico-de-gallo-internal` minor bump (pre-1.0).
-->

- [ ] No wire-protocol impact.
- [ ] Adds a new endpoint or topic (append-only, non-breaking).
- [ ] Appends a new variant to an existing wire enum (non-breaking).
- [ ] **BREAKING**: changes existing request/response types or reorders enum variants. I bumped `pico-de-gallo-internal` accordingly and updated firmware + all host crates in lockstep.

## Testing performed

<!-- What did you run and on what hardware? Be specific. -->

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --locked -- -D warnings`
- [ ] `cargo test --locked`
- [ ] Firmware, `hw-rev2` (default): `cd crates/pico-de-gallo-firmware && cargo build --release --locked --target thumbv8m.main-none-eabihf && cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings`
- [ ] Firmware, `hw-rev1` (deprecated, still published): same two commands with `--no-default-features --features hw-rev1`
- [ ] Tested on real hardware (describe below)

<!-- Hardware test notes, logic-analyzer captures, etc. -->

## Book ↔ code parity

<!--
Every code change must land with the matching `book/src/...` update
in the same PR. See AGENTS.md §15.1 for the per-area mapping and
reviewer checklist. PRs that ship code without docs (or docs without
verifying the code) will be flagged as blockers by reviewers,
including the automated Copilot reviewer.
-->

- [ ] No book-visible behavior changed in this PR.
- [ ] Book chapters updated in this PR to match the code change. List them:
  - <!-- e.g. book/src/interfaces/i2c.md, book/src/appendix/endpoints.md -->
- [ ] If only the book changed: I re-verified the code still matches what the new book text claims, and re-derived any tables (endpoints, status codes, capability bits) from the source files in AGENTS.md §15.1.
- [ ] `mdbook build book` is clean locally.

## Checklist

- [ ] Commits follow Conventional Commits with a correct scope.
- [ ] `Cargo.lock` is committed alongside any `Cargo.toml` change (host **and** firmware workspaces, as relevant). I ran with `--locked`.
- [ ] New `=X.Y.Z` exact pins are documented in `.github/copilot-instructions.md` under "Pinned dependency rationale".
- [ ] Public items have rustdoc; PyO3 items have docstrings.
- [ ] `book/` updated for new endpoints, CLI flags, or behavior changes (see "Book ↔ code parity" above).
- [ ] Each changed crate's `crates/<crate>/CHANGELOG.md` (or `zephyr/CHANGELOG.md`) follows Keep a Changelog (hand-written; not auto-generated). There is no root `CHANGELOG.md`.
- [ ] AI-assisted commits include `Co-authored-by: Copilot` and `Assisted-by:` trailers; no `Signed-off-by:` on AI commits.
