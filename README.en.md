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

An **ultra-precision command-line calculator** (a single Rust crate): 80 decimal digits of internal precision,
arbitrary-precision decimals plus exact symbolic arithmetic, equation / system solving, polynomial factorization,
function fitting, triangle solving, prime factorization — and **higher mathematics** (derivative `diff`,
limit `lim`, integral `int`, Taylor series `taylor`, sums and products `sum`/`prod`, all composable) —
with a syntax-highlighted REPL (Tab completes a function with the cursor inside the parentheses, bracket pairing,
in-bracket argument hints, Ctrl+C interrupts or quits, full-width punctuation auto-converted), and a Simplified Chinese / Traditional Chinese / English UI.

## Demo

![HiPerCalc demo](docs/demo.en.gif)

```
> 1/3+1/6
= 1 / 2
Time: <1s
> (x-2)(x+3)+2x=12
x = 3, x = -6
Time: <1s
> cos(x)=0
x = 90 + k·180, k ∈ ℤ
Time: <1s
> fac(x^5+x^4+1)
= (x^2 + x + 1) * (x^3 - x + 1)
Time: <1s
> (0,1) (1,3) (2,7)
y = x^2 + x + 1
y = (x + 1 / 2)^2 + 3 / 4
Time: <1s
> triangle(a=3,b=4,c=5)
a = 3, b = 4, c = 5
A ≈ 36.86989764584402129685561255909341065759157140070955794402796736942386835703051884, B ≈ 53.13010235415597870314438744090658934240842859929044205597203263057613164296948117, C = 90
hA = 4, hB = 3, hC = 12 / 5
area = 6, perimeter = 12
circumradius = 5 / 2, inradius = 1
Time: <1s
```

> The demo above is the output under **MathIO display + 80 significant digits + degree mode**;
> the defaults are LineIO / 20 digits / radians.

## Quick start

Grab the archive for your platform from [Releases](https://github.com/EasonZYQ/high_precision_calc/releases)
and unpack it — or build it yourself (needs a [Rust toolchain](https://rustup.rs)):

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
