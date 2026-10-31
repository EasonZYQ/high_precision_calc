# CODEDOC (English) — Code Structure & Extension Guide

> **Who this is for**: anyone changing this repository (including future me).
> **What it is not**: the user manual lives in `docs/DOC.en.md`; the performance log lives in
> `change_logs/`; collaboration rules live in `AGENTS.md`. This document only answers
> *what the code looks like, why it looks that way, and where to touch it when adding things*.
>
> Generate API docs: `cargo doc --no-deps --workspace --document-private-items --open`
> (the repo requires **zero rustdoc warnings**: it is equally clean under `RUSTDOCFLAGS="-D warnings"`).

---

## 1. Three layers

```
┌──────────────────────────────────────────────────────────────┐
│ hipercalc-gui   (Iced UI: input editor / keypad / pages)      │
│   ↕ only through Session                                     │
├──────────────────────────────────────────────────────────────┤
│ hipercalc       (expression layer: parse / evaluate / solvers │
│                  / commands / i18n)                          │
│   ↓ depends on                                               │
├──────────────────────────────────────────────────────────────┤
│ hipercalc-core  (numeric base: bignums, dual representation,  │
│                  formatting, matrices …)                     │
└──────────────────────────────────────────────────────────────┘
```

| crate | in one line | publishable |
| --- | --- | --- |
| `hipercalc-core` | get the number right: arbitrary-precision floats, exact rationals, complex/matrix/quantity, formatting | yes (`0.1.x`) |
| `hipercalc` | get the meaning right: parse text, dispatch solvers, produce readable output, CLI | yes (binary) |
| `hipercalc-gui` | get the UI right: input, keypad, history, feature pages, liquid glass | no |

⚠️ The root `Cargo.toml` **`default-members = [core, hipercalc]` must stay**: the GUI dependency
tree has 300+ crates; without it a bare `cargo build` and the 6-target cross-compile in CI would
pull in Iced.

---

## 2. Key types (learn these five and the code reads itself)

| Type | Location | What it is | The one thing to remember |
| --- | --- | --- | --- |
| `BigFloat` | core/`bigfloat.rs` | **decimal fixed point**: `value / 10^precision` | precision is *decimal digits*; every rounding is one `BigInt / 10^k` |
| `W` | core/`wide.rs` | **fixed 512-bit stack bignum**: `m × 2^e`, zero heap allocation | hot loops only; no division; overflow sets `over` |
| `Number` | core/`number.rs` | union of "a number": exact rational / approx / complex / matrix / quantity | **exact when possible**, degrade to approx once either side is approx |
| `Expr` | cli/`parser.rs` | syntax tree (incl. `Equation` / `Inequality` / `Function`) | output of parsing, input of evaluation |
| `Evaluator` | cli/`parser.rs` | evaluation context: variables, **user functions**, display/angle mode | evaluates through `&self` (depth guard is a `Cell`) |
| `Session` | cli/`session.rs` | the GUI's only entry point: `eval(line)` + settings + persistence | CLI and GUI share one semantic |
| `Doc` | gui/`expr.rs` | the **structured input document** (tree + cursor), not a `String` | a text model cannot express "several slots in one place" |
| `State` | gui/`app/mod.rs` | UI state: input, history, current page, cached modes | evaluation happens on **another thread**; the UI only gets snapshots |

---

## 3. The path of one input

```
"f(x)=x^2+1" / "x^2-2x+1=0" / "int(exp(-x^2),x,0,1)"
        │
        ├─ parse_and_eval (parser.rs)
        │     ├─ lex + parse → Expr
        │     └─ `f(x)=…` with f undefined ⇒ register a user function, return Symbolic
        │
        ├─ run_line (lib.rs) dispatches on EvalResult
        │     ├─ Value / Symbolic → Evaluator::evaluate
        │     ├─ Equation  → polynomial? → solver_poly / solver_linear / numeric scan
        │     ├─ Inequality→ solver_ineq (roots first, then sign per interval)
        │     ├─ System    → solver_linear (linear) / solver_nonlinear (multivariate Newton)
        │     ├─ Fit       → solver_fit
        │     └─ Triangle  → solver_triangle
        │
        └─ render
              ├─ CLI: calculus::render + display::format_* → ANSI-coloured text
              └─ GUI: same text → mathrender turns it into math typesetting
```

**One extra layer on the GUI side**: `Message::Evaluate` → `evaluator::Evaluator` (worker thread)
→ `Session::eval` → `run_line` → reply `Evaluated{ text, user_fns, user_vars }`
(the UI cannot read the worker's `Session`, so **snapshots travel back with the reply**).

---

## 4. Module map

### `hipercalc-core/src/`

| File | Responsibility |
| --- | --- |
| `bigfloat.rs` | arbitrary-precision float (4 ops, sqrt, pow, exp/ln, trig series, constant cache, scale guards) |
| `bigint_ext.rs` | big-integer helpers (long division + integer sqrt) |
| `wide.rs` | fixed 512-bit stack bignum `W` (zero allocation in hot loops; **no division**) |
| `number.rs` | `Number` dual representation and the degradation rules |
| `complex.rs` | complex arithmetic |
| `matrix.rs` | matrices: `det` (Bareiss) / `inv` / `solve` / `rank` / 2×2·3×3 characteristic poly |
| `trig.rs` | exact special angles, angle modes, inverse trig |
| `display.rs` | number → text (MathIO / LineIO / base / grouping / LaTeX) |
| `quantity.rs` | units and dimensions |
| `settings.rs` | process-wide switches (display mode, result base, LaTeX …) |
| `calc_mode.rs` | Fast / Deep modes and their scale guards |
| `cancel.rs` | cancellation checkpoints for long computations |

### `hipercalc/src/`

| File | Responsibility |
| --- | --- |
| `parser.rs` | lexer / parser / `Evaluator` / builtin table / user functions (**the biggest file**) |
| `lib.rs` | REPL body: command dispatch, result rendering, persistence, timing |
| `session.rs` | session shell used by the GUI (an `AppState` + `run_line`) |
| `state.rs` | state-file I/O (key=value lines; **user functions stored as source text**) |
| `equation.rs` | structural predicates on equations (polynomial? linear? contains trig?) |
| `solve_aux.rs` | solution organisation and rendering (general form, dedup, interval merge, `=`/`≈`) |
| `solver_poly.rs` | polynomial roots (exact quadratic / Durand-Kerner / Newton; prefers `W`) |
| `solver_factor.rs` | polynomial factorisation (uni/multivariate, rationals or reals) |
| `solver_ineq.rs` | polynomial inequalities (roots → sign per interval → merged intervals) |
| `solver_linear.rs` | linear systems (Gauss-Jordan) |
| `solver_nonlinear.rs` | nonlinear systems (multivariate Newton + initial grid) |
| `solver_fit.rs` | polynomial fitting / template parameter solving |
| `solver_triangle.rs` | triangle solving (closed form + Gauss-Newton fallback) |
| `calculus/` | `diff` / `integrate` (adaptive Gauss-Legendre) / `limit` / `series` (taylor) / `sumprod` / `render` |
| `stats.rs` | statistics |
| `i18n.rs` + `language/` | three languages (zh-CN / zh-TW / en) with longest-match replacement |

### `hipercalc-gui/src/`

| File | Responsibility |
| --- | --- |
| `app/mod.rs` | `State` + `Message` + `update` dispatch (**the UI hub**) |
| `app/view.rs` | layout: toolbar, input bar, results area, keypad |
| `app/results.rs` | results area: history / triangle page / user-functions page (`ResultsPage`) |
| `app/triangle.rs` · `app/userdefs.rs` | the two dedicated pages |
| `app/settings.rs` | settings panel (fonts, precision, theme, colours …) |
| `expr.rs` | **input model**: `Doc` tree + cursor + key semantics (`insert_key` is the only seam) |
| `mathrender.rs` | `Expr` → Iced elements (math typesetting, scripts, Σ, clickable hit areas) |
| `evaluator.rs` | channel to the worker thread (`Request` / `Evaluated` / `Command`) |
| `keypad.rs` | on-screen keypad layout and key table |
| `theme.rs` | theme, colours, fonts (dark/light) |
| `backdrop.rs` · `liquid.rs` · `refraction.rs` · `glass_wgsl.rs` | liquid glass: capture, SDF refraction, WGSL |
| `chrome.rs` | self-drawn title bar and window buttons |
| `dupl.rs` | Desktop Duplication (DXGI) capture backend |

---

## 5. Conventions and traps (read before touching code)

1. **Precision is decimal digits** (default 80): `BigFloat::precision` is *not* bits.
2. **Exact when possible**: `1/3` must never become `0.333…`; when writing a new operation,
   ask first "does this step demote an exact value?".
3. **Two rules for `W`**: coefficients must be **monic**, DK initial values **must not be collinear**.
   Violating them causes self-excited divergence that *looks* like a wrong result (bitten three times).
   See `solver_poly::wide_iterate_converges_on_cubic`.
4. **Tests run serially**: `RUST_TEST_THREADS=1` in `.cargo/config.toml`. Number base / precision are
   **process-wide** `Atomic*`; parallel tests pollute each other (observed: green single-threaded,
   flaky multi-threaded).
5. **`cargo build` does not compile `#[cfg(test)]`**: verify locally with `cargo build --all-targets`
   or `cargo test`.
6. **cargo's "fake Fresh"**: if a change seems to have no effect (same test count, old behaviour),
   `cargo clean -p <crate>` or `touch` the source file.
7. **Snapshot baselines are not in the repo** (`.gitignore` ignores `tests/snapshots/*.png`):
   every machine generates its own. After UI changes, delete and re-run, then **open the PNG** —
   a green test is not a proof of a correct picture.
8. **Adding a button or changing copy invalidates many snapshots**: that is expected, not a regression.

---

## 6. Extension guide

### Adding a builtin function (e.g. `cbrt`)

1. Add the name to `FUNCTIONS` in `parser.rs` (checked at **parse time**);
2. Add a branch in `eval_function` (arity, complex/real dispatch);
3. Implement the numerics in `core` (exact if possible, otherwise `BigFloat`);
4. Add one row to the function table in `docs/DOC.zh-CN.md` **and** `DOC.en.md`;
5. Add tests: normal / boundary (0, negative, out of range) / exactness (`cbrt(27)=3`).
   ⚠️ A single-letter builtin would collide with the **user function** rule — builtins are ≥2 chars.

### Adding a solver

1. Make `parser.rs` produce a new `EvalResult` variant (or reuse `Symbolic`);
2. New file `solver_xxx.rs` with a **module header stating responsibility and boundary**;
3. Wire it into `run_line`;
4. Tests: typical case, degenerate case, and a clear error when it must refuse.

### Changing the UI

1. To replace the whole results area: add a `ResultsPage` variant (**never another boolean**);
2. Input behaviour: change `insert_key` in `expr.rs` (the only seam) — do not build strings in `app/`;
3. Then run `cargo test -p hipercalc-gui` (includes snapshots) and look at every PNG.

---

## 7. How this repo proves correctness

| Tool | Command | Covers |
| --- | --- | --- |
| unit / integration | `cargo test` | 211 (CLI) + 28 (core) |
| GUI | `cargo test -p hipercalc-gui` | 157 unit + 20 snapshots |
| lints | `cargo clippy --all-targets -- -D warnings` | zero warnings is mandatory |
| docs | `cargo doc --no-deps --workspace --document-private-items` | zero rustdoc warnings required |
| perf probes | `cargo test --release -p hipercalc --lib perf_scan -- --ignored --nocapture` | per-feature timing (**same-process only**) |
| visual regression | delete `tests/snapshots/*.png`, run snapshot tests | layout / colours / typesetting |

⚠️ **Wall-clock noise on a dev machine is huge** (the same case measured 8 ms and 36 ms).
Performance claims must use **same-process min-of-N** or a **countable metric**
(e.g. "function evaluations"), never absolute seconds compared across moments.

---

## 8. Glossary

| Term | Meaning |
| --- | --- |
| **MathIO / LineIO** | the two result styles: exact form first / decimals first |
| **Fast / Deep** | calculation modes: guarded / brute force |
| **monic** | leading coefficient of a polynomial is 1; the DK step formula requires it |
| **Bareiss** | fraction-free elimination: divide by the *previous* pivot; exact on integer matrices |
| **DK** | Durand-Kerner: iterate all complex roots simultaneously |
| **`over`** | overflow flag of `W`; when set the value is untrustworthy and callers fall back to `BigFloat` |
