# HiPerCalc

[![CI](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml)
[![Release](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml)

[English](docs/README.en.md) · **简体中文**

**中文超高精度命令行计算器**（Rust 单 crate）：内置 80 位精度，任意精度小数与符号精确运算兼得，
支持方程与方程组求解、多项式因式分解与函数拟合、三角形求解、素因数分解，
自带语法高亮的交互 REPL，界面支持简体中文 / 繁體中文 / English。

![HiPerCalc 演示](docs/demo.gif)

## 快速开始

从 [Releases](https://github.com/EasonZYQ/high_precision_calc/releases) 下载对应平台的压缩包，解压即用；
也可以自行构建（需要 [Rust 工具链](https://rustup.rs)）：

```bash
cargo build --release   # 产物：target/release/hipercalc
cargo test              # 单元测试
```

```
> primefac(360)
360 = 2^3 * 3^2 * 5
> x^2-4=0
x = 2, x = -2
> triangle(a=3 b=4 c=5)
a = 3, b = 4, c = 5
A ≈ 36.869897645844021297, B ≈ 53.130102354155978703, C = 90
```

## 文档

- [完整文档（中文）](docs/README.zh-CN.md)
- [Full documentation (English)](docs/README.en.md)
- [贡献指南](CONTRIBUTING.md)
