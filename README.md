# HiPerCalc

[![CI](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/ci.yml)
[![Release](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml/badge.svg)](https://github.com/EasonZYQ/high_precision_calc/actions/workflows/release.yml)

[English](README.en.md) · **简体中文**

**中文超高精度命令行计算器**（Rust 单 crate）：内置 80 位精度，任意精度小数与符号精确运算兼得，
支持方程与方程组求解、多项式因式分解与函数拟合、三角形求解、素因数分解，
以及**高等数学**（求导 `diff` / 极限 `lim` / 积分 `int` / 泰勒展开 `taylor` / 求和求积 `sum`·`prod`，
结果可继续参与运算），自带语法高亮的交互 REPL（Tab 补全函数会把光标放进括号、括号配对高亮、
框内参数提示、Ctrl+C 清行、全角标点自动转半角），界面支持简体中文 / 繁體中文 / English。

## 演示

![HiPerCalc 演示](docs/demo.gif)

```
> 1/3+1/6
= 1 / 2
用时：<1秒
> (x-2)(x+3)+2x=12
x = 3, x = -6
用时：<1秒
> cos(x)=0
x = 90 + k·180，k 为整数
用时：<1秒
> fac(x^5+x^4+1)
= (x^2 + x + 1) * (x^3 - x + 1)
用时：<1秒
> (0,1) (1,3) (2,7)
y = x^2 + x + 1
y = (x + 1 / 2)^2 + 3 / 4
用时：<1秒
> triangle(a=3,b=4,c=5)
a = 3, b = 4, c = 5
A ≈ 36.86989764584402129685561255909341065759157140070955794402796736942386835703051884, B ≈ 53.13010235415597870314438744090658934240842859929044205597203263057613164296948117, C = 90
hA = 4, hB = 3, hC = 12 / 5
面积 = 6, 周长 = 12
外接圆半径 = 5 / 2, 内切圆半径 = 1
用时：<1秒
```

> 上面这段是 **MathIO 显示 + 80 位有效数字 + 角度模式**下的输出；默认是 LineIO / 20 位 / 弧度。

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
