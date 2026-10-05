# HiPerCalc

[![CI](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml)
[![Release](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#快速开始)
[![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)](https://www.rust-lang.org/)
[![Precision](https://img.shields.io/badge/precision-80%20digits-brightgreen.svg)](docs/DOC.zh-CN.md)
[![UI](https://img.shields.io/badge/UI-%E7%AE%80%E4%BD%93%E4%B8%AD%E6%96%87%20%7C%20%E7%B9%81%E9%AB%94%E4%B8%AD%E6%96%87%20%7C%20English-blueviolet.svg)](docs/DOC.zh-CN.md)
[![Last commit](https://img.shields.io/github/last-commit/EasonZYQ/high_precision_calc)](https://github.com/EasonZYQ/high_precision_calc/commits/main)

[English](README.en.md) · **简体中文**

**中文超高精度命令行计算器**（Rust 双 crate workspace：`hipercalc` + 可独立使用的数值库 `hipercalc-core`）：内置 80 位精度，任意精度小数与符号精确运算兼得，
支持方程与方程组求解、多项式因式分解与函数拟合、三角形求解、素因数分解，
以及**高等数学**（求导 `diff` / 极限 `lim` / 积分 `int` / 泰勒展开 `taylor` / 求和求积 `sum`·`prod`，
结果可继续参与运算），自带语法高亮的交互 REPL（Tab 补全函数会把光标放进括号、括号配对高亮、
框内参数提示、Ctrl+C 中断运算或退出、全角标点自动转半角），界面支持简体中文 / 繁體中文 / English。

## 演示

![HiPerCalc 演示](docs/demo.gif)

```
> 1/3+1/6
= 0.5
> sin(pi/6)             ← 输入 sin 后按 Tab：自动补成 sin() 并把光标放进括号，
= 0.5                      右侧暗色显示还没写的参数，写完一个该参数就消失
> (x-2)(x+3)+2x=12
x = 3, x = -6
> diff(x^2*sin(x),x)
= x^2*cos(x) + 2*x*sin(x)
> fac(x^5+x^4+1)
= (x^2 + x + 1) * (x^3 - x + 1)
> sum(k,k,1,1000000)    ← 有闭式的求和直接给精确值（不逐项累加）
= 500000500000
> triangle(a=3,b=4,c=5)
a = 3, b = 4, c = 5
A ≈ 0.6435011087932843868, B ≈ 0.92729521800161223243, C ≈ 1.5707963267948966192
hA = 4, hB = 3, hC = 2.4
面积 = 6, 周长 = 12
外接圆半径 = 2.5, 内切圆半径 = 1
```

> 上面是**默认设置**（LineIO 20 位 / 弧度）下的真实输出；切到 MathIO 会显示精确分数与根式，
> `/mode deg` 换成角度制。
> 英文界面演示见 `docs/demo.en.gif`（录制脚本 `--lang en`）。

## 输入与快捷键

| 操作 | 效果 |
|---|---|
| `Tab` | 补全函数 / 指令 / 常数 / 变量；函数补成 `name()` 并**把光标放进括号**，单字母 token 不补（保护 `xy` 这类隐式乘法） |
| 光标停在函数括号内 | 右侧以暗色显示**还没写的参数**，参数过多转红提示；提示只显示、不补全 |
| 光标停在括号上 | 该括号与其**配对括号加粗**；多余的 `)` 一律标红，未闭合的 `(` 在光标不在其右侧时标红（不拦回车） |
| `Ctrl+C` | **计算中**中断当前运算（如高次求根、数值积分）；**空闲时**退出程序 |
| `Ctrl+L` / `Ctrl+R` | 清屏 / 反向搜索历史 |
| 中文输入法的全角标点 | `。` `（` `）` `，` `＝` `＋` `－` `×` `÷` 与全角数字**自动转半角**，不会报"多余的字符" |
| 非交互用法 | `hipercalc -e "表达式"`、`-f 脚本文件`、`--stdin`、`--lang zh-CN|zh-TW|en`、`-q`（不显示耗时） |

## 项目结构

```text
crates/
├── hipercalc-core/   数值底座（任意精度浮点 / 符号精确数 / 复数 / 特殊角）—— 可独立发布
└── hipercalc/        解析器、求解器、高等数学、三语界面与 REPL（产出 hipercalc 二进制）
```

## 快速开始

从 [Releases](https://github.com/EasonZYQ/high_precision_calc/releases) 下载对应平台的压缩包，解压即用；
也可以自行构建（需要 [Rust 工具链](https://rustup.rs)）：

```bash
cargo build --release   # 产物：target/release/hipercalc
cargo test              # 单元测试
```

## 文档

- [完整文档（中文）](docs/DOC.zh-CN.md)
- [Full documentation (English)](docs/DOC.en.md)
- [贡献指南](CONTRIBUTING.md) · [Contributing (English)](CONTRIBUTING.en.md)

## 许可

[MIT](LICENSE) © 2026 EasonZYQ
