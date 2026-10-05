# HiPerCalc

[![CI](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml)
[![Release](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#quick-start)
[![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)](https://www.rust-lang.org/)
[![Precision](https://img.shields.io/badge/precision-80%20digits-brightgreen.svg)](docs/DOC.en.md)
[![UI](https://img.shields.io/badge/UI-%E7%AE%80%E4%BD%93%E4%B8%AD%E6%96%87%20%7C%20%E7%B9%81%E9%AB%94%E4%B8%AD%E6%96%87%20%7C%20English-blueviolet.svg)](docs/DOC.en.md)
[![Last commit](https://img.shields.io/github/last-commit/EasonZYQ/high_precision_calc)](https://github.com/EasonZYQ/high_precision_calc/commits/main)

[简体中文](README.md) · **English**

An **ultra-precision command-line calculator** (a two-crate Rust workspace: `hipercalc` plus the reusable numeric library `hipercalc-core`): 80 decimal digits of internal precision,
arbitrary-precision decimals plus exact symbolic arithmetic, equation / system solving, polynomial factorization,
function fitting, triangle solving, prime factorization — and **higher mathematics** (derivative `diff`,
limit `lim`, integral `int`, Taylor series `taylor`, sums and products `sum`/`prod`, all composable) —
with a syntax-highlighted REPL (Tab completes a function with the cursor inside the parentheses, bracket pairing,
in-bracket argument hints, Ctrl+C interrupts (Windows only) or quits, full-width punctuation auto-converted), and a Simplified Chinese / Traditional Chinese / English UI.

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
> diff(x^2*sin(x),x)
= x^2*cos(x) + 2*x*sin(x)
> fac(x^5+x^4+1)
= (x^2 + x + 1) * (x^3 - x + 1)
> sum(k,k,1,1000000)    <- sums with a closed form are exact (no term-by-term loop)
= 500000500000
```

> Real output with the **default** settings (LineIO, 20 digits, radians); switch to MathIO for exact
> fractions and radicals, or `/mode deg` for degrees. The Chinese demo is `docs/demo.gif`.

## Input & shortcuts

| Action | Effect |
|---|---|
| `Tab` | complete functions / commands / constants / variables; a function becomes `name()` with the **cursor placed inside the brackets**. Single-letter tokens get no candidates (so `xy` still means x·y) |
| Cursor inside a function call | the parameters **still to write** are shown in dim text to the right; too many turn red. Hints are display-only and never inserted |
| Cursor on a bracket | that bracket and its **partner are bolded**; a surplus `)` is always red, an unclosed `(` turns red when the cursor is not to its right (Enter is never blocked) |
| `Ctrl+C` | **when idle**: quit (all platforms); **while computing**: interrupt the current computation (high-degree root finding, numeric integration…) — mid-computation interrupt is currently **Windows-only** |
| `Ctrl+L` / `Ctrl+R` | clear screen / reverse-search history |
| Full-width punctuation from an IME | `。` `（` `）` `，` `＝` `＋` `－` `×` `÷` and full-width digits are **converted automatically** |
| Non-interactive use | `hipercalc -e "expr"`, `-f script`, `--stdin`, `--lang zh-CN|zh-TW|en`, `-q` (hide timings) |

## Layout

```text
crates/
├── hipercalc-core/   numeric core (arbitrary-precision floats / exact numbers / complex / exact trig) — publishable on its own
└── hipercalc/        parser, solvers, higher mathematics, trilingual UI and the REPL (produces the hipercalc binary)
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
