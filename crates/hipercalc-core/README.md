# hipercalc-core

[hipercalc](https://github.com/EasonZYQ/high_precision_calc) 的**数值底座**：任意精度浮点、符号精确数、复数与特殊角精确值。

这一层**不依赖**解析器、求解器、i18n 或 REPL —— 只做"把数算准"，因此可以独立集成。

## 模块

| 模块 | 职责 |
|---|---|
| `bigfloat` | 任意精度浮点（四则 / 开方 / 幂 / exp / ln / 三角级数 + 规模保护） |
| `bigint_ext` | 大整数补充运算（自带长除法 + 整数平方根，绕开 num-bigint 的边界缺陷） |
| `number` | 符号精确 + 数值近似的双表示（`Number`） |
| `complex` | 复数运算 |
| `trig` | 特殊角精确值表（度 / 弧度 / 反三角）与角度模式 |
| `display` | 输出格式化（MathIO 精确式 / LineIO 小数） |
| `calc_mode` | 进程级计算模式开关（Fast 有规模保护 / Deep 死算） |
| `cancel` | 长计算的**中断检查点**（由上层安装控制台处理器） |

## 精度

内部计算默认 **80 位**十进制精度、显示 **20 位**有效数字，都可在运行期调整：

```rust
use hipercalc_core::bigfloat::BigFloat;
use hipercalc_core::number::Number;

// 精确有理数：1/3 不会立刻变成小数
let third = Number::div(&Number::from_int(1), &Number::from_int(3));

// 需要小数时再取近似值
let sqrt2 = BigFloat::from_u64(2).sqrt(80);
assert!(sqrt2.to_significant_string(10).starts_with("1.41421356"));
```

## 依赖

只依赖 `num-bigint` / `num-rational` / `num-traits` / `num-integer`。
**刻意不依赖 `colored`、`rustyline`** —— 终端着色与行编辑属于 UI 层，不属于数值层。

## 许可

[MIT](../LICENSE) © 2026 EasonZYQ
