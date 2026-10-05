# Contributing

[简体中文](CONTRIBUTING.md) · **English**

Thanks for wanting to help with HiPerCalc! Bug reports, documentation improvements and code are all welcome.
(Issues and PRs in Chinese or English are both fine.)

## What you can contribute

- **Bug reports**: include the input, the expected result and the actual result, plus your environment
  (OS, how you built it). If the output is long, just paste the relevant part.
- **Documentation**: both `docs/DOC.en.md` (English) and `docs/DOC.zh-CN.md` (Chinese) need to be kept in sync.
- **Code**: bug fixes, new functions and performance work are all welcome. Opening an issue first to discuss
  the approach is appreciated.

## Development setup

All you need is a [Rust toolchain](https://rustup.rs) (stable) — the project is a single crate with no extra
system dependencies:

```bash
cargo build            # debug build
cargo build --release  # release build
cargo run              # start the interactive REPL
cargo test --workspace # run all unit tests (144: core 11 + hipercalc 133)
cargo test -p hipercalc-core   # numeric core only (much faster when iterating on bignum/precision work)
```

> On Windows, `failed to remove ... hipercalc.exe (os error 5)` means the binary is still running —
> `taskkill /f /im hipercalc.exe` and build again.

## Checklist before you submit

1. **`cargo build` and `cargo test --workspace` both pass, with no new compiler warnings** — and **note the elapsed
   time** (the `finished in …` line at the end of `cargo test`, or `time cargo test` to include compilation);
   without it there is no way to tell whether a change made the solvers much slower;
2. **New behaviour needs unit tests** (tests live in `#[cfg(test)] mod tests` at the bottom of the source file);
3. **Keep the UI languages in sync**: the entry table now lives in `src/language/<code>.json`
   (embedded at compile time — **adding a language means adding one JSON file** and registering it in
   `language::FILES`; see the module docs in that directory). Every user-visible string must be added to
   both `zh-TW.json` and `en.json` — the **Simplified source text is the key**, and the two key sets must
   match exactly (a missing entry leaves Chinese text in that language's UI).
   Watch out for long entries with placeholders: a literal part must **not straddle two coloured tokens**
   (e.g. the ` = ` in `{0} = {1}，k 为整数` sits between a variable and a number), otherwise the replacement
   drops the colour codes there. Rewrite such entries as a contiguous plain-text fragment
   (e.g. keep only the tail `，k 为整数`);
4. **Keep `/help` in sync in all three languages** (`HELP_TEXT` / `HELP_TEXT_TW` / `HELP_TEXT_EN` in `src/main.rs`);
5. **Keep the docs in sync**: update `docs/DOC.en.md` and `docs/DOC.zh-CN.md` when behaviour changes;
   add pitfalls and conventions to `AGENTS.md`;
6. **Don't commit generated or local-only files**: `target/`, `.workbuddy/`, `.trae/`, `change_logs/`, `*.log`
   are all in `.gitignore` — check `git status` after `git add`.

## Code conventions

- **Comments and commit messages may be in Chinese or English** (just stay consistent within one place —
  there is no requirement to pick one language globally);
- User-visible output must go through the `lprint!` / `leprint!` macros, **never `println!`** — that would
  bypass i18n;
- **Do not remove the size guards** in the numeric core (the `exp` argument cap, the power-result digit cap,
  trial-division budgets, Newton divergence protection, …): they exist to avoid silently wrong results or a
  frozen UI. `/mode deep` is the switch that lifts them;
- **Always check the divisor with `is_zero()` before calling `BigFloat::div`** — it uses `assert!` internally,
  so a zero divisor panics the process;
- When writing two platform implementations with `#[cfg(windows)]` / `#[cfg(not(windows))]`, **their types and
  function signatures must match**: shared callers compile against both, and a missing variant only shows up
  on non-Windows platforms;
- Before touching a module, read the "easy to get wrong" notes for it in `AGENTS.md` — most pitfalls are recorded there.

## Submitting

1. Fork the repo and branch off `main` (`fix/xxx`, `feat/xxx`);
2. Walk through the checklist above;
3. **Keep commit messages short** — one or two sentences on what changed; details belong in the PR description;
4. Open a pull request explaining the motivation and how you verified it.

## CI

Every push / PR triggers [CI](.github/workflows/ci.yml):

- **test**: `cargo build` + `cargo test` on Ubuntu / Windows / macOS;
- **cross**: builds every target that Release ships (Linux gnu/musl/aarch64, macOS x86_64/aarch64,
  Windows msvc).

> **Cross-platform compilability matters**: a missing enum variant in the non-Windows stub (`rawkey::Key` had
> only `Other`) once broke **every non-Windows target**, and it only surfaced when a tag was pushed.
> The `cross` job now catches that class of problem at PR time.

Pushing a `v*` tag triggers [Release](.github/workflows/release.yml), which builds the per-platform binaries
and publishes them to Releases.
