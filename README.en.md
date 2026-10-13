# HiPerCalc

[![CI](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml)
[![Release](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml)
[![Version](https://img.shields.io/github/v/release/EasonZYQ/high_precision_calc?label=release)](https://github.com/EasonZYQ/high_precision_calc/releases/latest)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#quick-start)
[![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)](https://www.rust-lang.org/)
[![Precision](https://img.shields.io/badge/precision-80%20digits-brightgreen.svg)](docs/DOC.en.md)
[![UI](https://img.shields.io/badge/UI-%E7%AE%80%E4%BD%93%E4%B8%AD%E6%96%87%20%7C%20%E7%B9%81%E9%AB%94%E4%B8%AD%E6%96%87%20%7C%20English-blueviolet.svg)](docs/DOC.en.md)
[![Last commit](https://img.shields.io/github/last-commit/EasonZYQ/high_precision_calc)](https://github.com/EasonZYQ/high_precision_calc/commits/main)

[简体中文](README.md) · **English**

An **ultra-precision command-line calculator** (a Rust workspace: the `hipercalc` command line plus the reusable numeric library `hipercalc-core`; the graphical front-end `hipercalc-gui` is **work in progress**): 80 decimal digits of internal precision,
arbitrary-precision decimals plus exact symbolic arithmetic, equation / system / inequality solving, polynomial factorization,
function fitting, triangle solving, prime factorization, matrices and linear algebra, statistics, number bases and bitwise
ops, dimensional quantities with units, special functions (`gamma`/`erf`/`zeta`) and physical constants — and **higher mathematics** (derivative `diff`,
limit `lim`, integral `int`, Taylor series `taylor`, sums and products `sum`/`prod`, all composable) —
with a syntax-highlighted REPL (Tab completes a function with the cursor inside the parentheses, bracket pairing,
in-bracket argument hints, Ctrl+C interrupts or quits, full-width punctuation auto-converted), and a Simplified Chinese / Traditional Chinese / English UI.

## Demo

![HiPerCalc demo](docs/demo.en.gif)

```
> 1/3+1/6
= 0.5
> sin(pi/6)             <- type sin then Tab: it becomes sin() with the cursor inside,
= 0.5                      and the parameters still to write appear in dim text; each one
                           disappears once you have typed it
> (x-2)(x+3)+2x=12
x = 3, x = -6
> x^6-1=0               <- equations give every root, real and complex
x = 1, x ≈ 0.5 + 0.86602540378443864676i, x ≈ -0.5 + 0.86602540378443864676i, x = -1, x ≈ -0.5 - 0.86602540378443864676i, x ≈ 0.5 - 0.86602540378443864676i
> tan(x)=1              <- trigonometric equations give the general solution
x = π/4 + k·π, k ∈ ℤ
> 3 km / 2 second       <- unit arithmetic with dimensions
= 1500 m/s
> x^4-5x^2+4>0          <- inequalities come back as intervals
x ∈ (-∞, -2) ∪ (-1, 1) ∪ (2, ∞)
> sum(k,k,1,1000000)    <- sums with a closed form are exact (no term-by-term loop)
= 500000500000
> triangle(x=3 y=4 z=5) <- triangle notation accepts any letters: lower = side, upper = angle, hX = altitude
x = 3, y = 4, z = 5
X ≈ 0.6435011087932843868, Y ≈ 0.92729521800161223243, Z ≈ 1.5707963267948966192
hX = 4, hY = 3, hZ = 2.4
area = 6, perimeter = 12
circumradius = 2.5, inradius = 1
```

> Real output with the **default** settings (LineIO, 20 digits, radians); switch to MathIO for exact
> fractions and radicals, or `/mode deg` for degrees. The Chinese demo is `docs/demo.gif`. Both GIFs are
> recorded with [`vhs`](https://github.com/charmbracelet/vhs) from `docs/demo.tape` / `docs/demo.en.tape`
> and compressed with `gifsicle -O3 --colors 128 --lossy=80`; to re-record, move `~/.hipercalc_state`
> away first (otherwise the demo inherits this machine's persisted settings and is not reproducible).

## Input & shortcuts

| Action | Effect |
|---|---|
| `Tab` | complete functions / commands / constants / variables; a function becomes `name()` with the **cursor placed inside the brackets**. Single-letter tokens get no candidates (so `xy` still means x·y) |
| Cursor inside a function call | the parameters **still to write** are shown in dim text to the right; too many turn red. Hints are display-only and never inserted |
| Cursor on a bracket | that bracket and its **partner are bolded**; a surplus `)` is always red, an unclosed `(` turns red when the cursor is not to its right (Enter is never blocked) |
| Units & dimensions | `3 km`, `2 mile` are **dimensioned values**: `3 km / 2 second = 1500 m/s`, `(2 km)^2 = 4000000 m^2`, `1 km / 1 mile ~= 0.621371`; adding mismatched dimensions is **rejected** (`3 km + 2 second`), while `*` and `/` follow dimensional algebra |
| `Ctrl+C` | **when idle**: quit; **while computing**: interrupt the current computation (high-degree root finding, numeric integration…) — **supported on all platforms** (Windows via `SetConsoleCtrlHandler`, Linux / macOS / BSD via `SIGINT`; both hand-written FFI, no extra dependency) |
| `Ctrl+L` / `Ctrl+R` | clear screen / reverse-search history |
| Full-width punctuation from an IME | `。` `（` `）` `，` `＝` `＋` `－` `×` `÷` and full-width digits are **converted automatically** |
| Non-interactive use | `hipercalc -e "expr"`, `-f script`, `--stdin`, `--lang zh-CN|zh-TW|en`, `-q` (hide timings) |

## Layout

```text
crates/
├── hipercalc-core/   numeric core (arbitrary-precision floats / exact numbers / complex / exact trig) — publishable on its own
├── hipercalc/        parser, solvers, higher mathematics, trilingual UI and the REPL (produces the hipercalc binary)
└── hipercalc-gui/    graphical front-end (Iced) — **work in progress**, currently just a toolchain smoke test; see its README
```

## Quick start

Grab the raw executable for your platform from [Releases](https://github.com/EasonZYQ/high_precision_calc/releases)
(no archives — download and run; **on Linux / macOS restore the executable bit with `chmod +x` first**,
and renaming it to `hipercalc` is recommended) — or build it yourself (needs a [Rust toolchain](https://rustup.rs)):

```bash
cargo build --release   # the binary lands at target/release/hipercalc
cargo test              # unit tests
```

## Documentation

- [Full documentation (English)](docs/DOC.en.md)
- [完整文档（中文）](docs/DOC.zh-CN.md)
- [Contributing](CONTRIBUTING.en.md) · [贡献指南](CONTRIBUTING.md)

## License

[MIT](LICENSE) © 2026 EasonZYQ
