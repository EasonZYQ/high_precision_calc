# HiPerCalc

[![CI](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/release/EasonZYQ/high_precision_calc?label=release)](https://github.com/EasonZYQ/high_precision_calc/releases/latest)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#install)
[![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)](https://www.rust-lang.org/)
[![Precision](https://img.shields.io/badge/precision-80%20digits-brightgreen.svg)](docs/DOC.en.md)

**English** · [简体中文](README.md)

**A high-precision calculator** with two front ends — a CLI and a GUI. **80-digit precision**,
exact results whenever exactness is possible, single static binary for Windows / Linux / macOS,
UI in Simplified Chinese / Traditional Chinese / English.

<p align="center">
  <img src="docs/gui-light.png" width="440" alt="GUI (light)">
  <img src="docs/gui-dark.png" width="440" alt="GUI (dark, liquid glass)">
</p>

## Highlights

- **Accurate** — 80-digit precision; `1/3+1/6` is exactly `1/2`, MathIO mode shows fractions and radicals
- **Does calculus** — `diff` / `lim` / `int` / `taylor` / `sum` / `prod`, results stay usable in further math
- **Solves equations** — equations, systems, inequalities; **all** roots (complex included), trig equations return the general form
- **More than arithmetic** — triangle solver, factorization, matrices & linear algebra, statistics, base/bitwise ops, dimensional units, special functions, physical constants
- **Two front ends** — a syntax-highlighted REPL (Tab completion / bracket matching / argument hints) and an Iced GUI (Desmos-style keypad, real math typesetting, liquid glass, light & dark themes)
- **Three languages** — Simplified Chinese / Traditional Chinese / English, switchable at runtime

```
> 1/3+1/6
= 0.5
> (x-2)(x+3)+2x=12
x = 3, x = -6
> 3 km / 2 second
= 1500 m/s
> sum(k,k,1,1000000)
= 500000500000
```

![CLI demo](docs/demo.en.gif)

## Install

Grab the binary for your platform from [Releases](https://github.com/EasonZYQ/high_precision_calc/releases)
(on Linux / macOS run `chmod +x` first — GitHub drops the executable bit).
Or build it yourself (requires Rust 1.85+):

```bash
cargo build --release                     # CLI → target/release/hipercalc
cargo build -p hipercalc-gui --release    # GUI → target/release/hipercalc-gui
```

The GUI ships for all three platforms (Windows / macOS / Linux). On Linux it needs `libxkbcommon-x11-0`
and `libwayland-client0`; the glass texture uses Windows desktop duplication and falls back to "off" elsewhere.

## Docs

- 📘 [Full manual (English)](docs/DOC.en.md) · [完整文档（中文）](docs/DOC.zh-CN.md) — every function, command and switch
- 🎨 [Liquid glass notes](docs/liquid-glass-report.md) — algorithm, frame budget, measurements
- 🧩 [Windows 7 compatibility survey](docs/windows7.md)
- 🤝 [Contributing](CONTRIBUTING.en.md) · [贡献指南](CONTRIBUTING.md)

## License

[MIT](LICENSE) © 2026 EasonZYQ
