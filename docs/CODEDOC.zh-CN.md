# CODEDOC（中文）—— 代码结构与扩展指南

> **读者**：要改这个仓库的人（包括几个月后的自己）。
> **不重复**：用户手册看 `docs/DOC.zh-CN.md`；性能流水账看 `change_logs/`；
> 协作约定看 `AGENTS.md`。这里只讲**代码长什么样、为什么这么长、要加东西该动哪里**。
>
> 生成 API 文档：`cargo doc --no-deps --workspace --document-private-items --open`
> （仓库要求**零 rustdoc 警告**：`RUSTDOCFLAGS="-D warnings"` 时同样干净）。

---

## 1. 三层结构

```
┌──────────────────────────────────────────────────────────┐
│ hipercalc-gui   （Iced 图形界面：输入编辑器 / 键区 / 页面） │
│   ↕ 只通过 Session                                       │
├──────────────────────────────────────────────────────────┤
│ hipercalc       （表达式层：解析 / 求值 / 求解器 / 命令 / i18n）│
│   ↓ 依赖                                                 │
├──────────────────────────────────────────────────────────┤
│ hipercalc-core  （数值底座：大数、双表示、格式化、矩阵…）    │
└──────────────────────────────────────────────────────────┘
```

| crate | 一句话 | 能否独立发布 |
| --- | --- | --- |
| `hipercalc-core` | 把数算准：任意精度浮点、精确有理数、复数/矩阵/量纲、格式化 | 能（`0.1.x`） |
| `hipercalc` | 把话说懂：解析文本、分派求解器、产出可读结果、命令行 | 能（CLI 二进制） |
| `hipercalc-gui` | 把界面做好：输入、键区、历史、各功能页、液态玻璃 | 否（依赖前两者） |

⚠️ 根 `Cargo.toml` 的 **`default-members = [core, hipercalc]` 不能删**：
GUI 依赖树有 300+ 包，没有它，裸 `cargo build` / CI 的 6 目标交叉编译都会连带编 Iced。

---

## 2. 关键类型（先认这几个，代码就看得懂了）

| 类型 | 位置 | 它代表什么 | 记住这一条 |
| --- | --- | --- | --- |
| `BigFloat` | core/`bigfloat.rs` | **十进制定点**：`value / 10^precision` | 精度是"十进制位数"；每次舍入 = 一次 `BigInt / 10^k` |
| `W` | core/`wide.rs` | **定宽 512 位栈上大数**：`m × 2^e`，零堆分配 | 只给热循环用；没有除法；溢出置 `over` |
| `Number` | core/`number.rs` | 一个"数"的联合体：精确有理 / 近似 / 复数 / 矩阵 / 量纲 | **能精确就精确**，一旦有一边近似就全程近似 |
| `Expr` | cli/`parser.rs` | 语法树（含 `Equation`/`Inequality`/`Function`） | 解析期产物，求值期的输入 |
| `Evaluator` | cli/`parser.rs` | 求值上下文：变量表、**自定义函数表**、显示/角度模式 | 求值走 `&self`（深度计数器用 `Cell`） |
| `Session` | cli/`session.rs` | GUI 唯一入口：`eval(line)` + 设置 + 持久化 | CLI 与 GUI 共享同一套语义 |
| `Doc` | gui/`expr.rs` | **输入框的结构化文档**（树 + 光标），不是 `String` | 文本模型表达不了"同一处多个空位" |
| `State` | gui/`app/mod.rs` | 界面状态：输入、历史、页面、模式副本 | 求值在**别的线程**，UI 只拿快照 |

---

## 3. 一次输入经过的路径

```
"f(x)=x^2+1" / "x^2-2x+1=0" / "int(exp(-x^2),x,0,1)"
        │
        ├─ parse_and_eval（parser.rs）
        │     ├─ 词法+语法 → Expr
        │     └─ 是 `f(x)=…` 且 f 未定义 ⇒ 登记为自定义函数，返回 Symbolic
        │
        ├─ run_line（lib.rs）按 EvalResult 分派
        │     ├─ Value/Symbolic → Evaluator::evaluate
        │     ├─ Equation      → equation::is_polynomial？→ solver_poly / solver_linear / 初值扫描
        │     ├─ Inequality    → solver_ineq（先求根再分段判号）
        │     ├─ System        → solver_linear（线性）/ solver_nonlinear（多维牛顿）
        │     ├─ Fit           → solver_fit
        │     └─ Triangle      → solver_triangle
        │
        └─ 渲染
              ├─ CLI：calculus::render + display::format_* → 带 ANSI 颜色的文本
              └─ GUI：同样的文本 → mathrender 转成数学排版
```

**GUI 侧多一层**：`Message::Evaluate` → `evaluator::Evaluator`（worker 线程）→
`Session::eval` → `run_line` → 回包 `Evaluated{ text, user_fns, user_vars }`
（UI 读不到 worker 的 `Session`，所以**快照随回包带回**）。

---

## 4. 模块地图

### `hipercalc-core/src/`

| 文件 | 职责 |
| --- | --- |
| `bigfloat.rs` | 任意精度浮点（四则、开方、幂、exp/ln、三角级数、常数缓存、规模保护） |
| `bigint_ext.rs` | 大整数补充运算（自带长除法 + 整数平方根） |
| `wide.rs` | 定宽 512 位栈上大数 `W`（热循环零分配；**没有除法**） |
| `number.rs` | `Number` 双表示与逐级降级规则 |
| `complex.rs` | 复数运算 |
| `matrix.rs` | 矩阵：`det`（Bareiss）/ `inv` / `solve` / `rank` / 2×2·3×3 特征多项式 |
| `trig.rs` | 特殊角精确值表、角度模式、反三角 |
| `display.rs` | 数 → 文本（MathIO / LineIO / 进制 / 分组 / LaTeX） |
| `quantity.rs` | 单位与量纲 |
| `settings.rs` | 进程级开关（显示模式、结果进制、LaTeX…） |
| `calc_mode.rs` | Fast / Deep 两种计算模式与规模护栏 |
| `cancel.rs` | 长计算的中断检查点（由上层装控制台处理器） |

### `hipercalc/src/`

| 文件 | 职责 |
| --- | --- |
| `parser.rs` | 词法 / 语法 / `Evaluator` / 内置函数表 / 自定义函数（**最大的一个文件**） |
| `lib.rs` | REPL 主体：命令分发、结果渲染、状态持久化、耗时统计 |
| `session.rs` | GUI 用的会话外壳（内部就是 `AppState` + `run_line`） |
| `state.rs` | 状态文件读写（键值行；**自定义函数存定义原文**） |
| `equation.rs` | 方程结构判定（多项式？线性？含三角？） |
| `solve_aux.rs` | 解的组织与渲染（通式、去重、区间合并、`=`/`≈` 判定） |
| `solver_poly.rs` | 多项式求根（二次精确解 / Durand-Kerner / 牛顿；优先用 `W`） |
| `solver_factor.rs` | 多项式因式分解（单/多元、有理数域/实数域） |
| `solver_ineq.rs` | 多项式不等式（求根 → 分段判号 → 合并区间） |
| `solver_linear.rs` | 线性方程组（高斯-约当） |
| `solver_nonlinear.rs` | 非线性方程组（多维牛顿 + 初值网格） |
| `solver_fit.rs` | 多项式拟合 / 模板参数求解 |
| `solver_triangle.rs` | 三角形求解（解析解 + Gauss-Newton 兜底） |
| `calculus/` | 微积分：`diff` / `integrate`（自适应 Gauss-Legendre）/ `limit` / `series`（taylor）/ `sumprod` / `render`（表达式 → 文本） |
| `stats.rs` | 统计函数 |
| `i18n.rs` + `language/` | 三语（zh-CN / zh-TW / en）词条与最长匹配替换 |

### `hipercalc-gui/src/`

| 文件 | 职责 |
| --- | --- |
| `app/mod.rs` | `State` + `Message` + `update` 分发（**界面的中枢**） |
| `app/view.rs` | 布局：工具条、输入栏、结果区、键盘区的拼装 |
| `app/results.rs` | 结果区：历史列表 / 三角页 / 自定义函数页（`ResultsPage` 三选一） |
| `app/triangle.rs` · `app/userdefs.rs` | 两个独立功能页 |
| `app/settings.rs` | 设置面板（字体、精度、主题、颜色…） |
| `expr.rs` | **输入框模型**：`Doc` 树 + 光标 + 按键语义（`insert_key` 是唯一接缝） |
| `mathrender.rs` | `Expr` → Iced 元素（数学排版、上下标、Σ、可点热区） |
| `evaluator.rs` | 与 worker 线程的通道（`Request` / `Evaluated` / `Command`） |
| `keypad.rs` | 虚拟键盘布局与键位表 |
| `theme.rs` | 主题、颜色、字体（深/浅两套） |
| `backdrop.rs` · `liquid.rs` · `refraction.rs` · `glass_wgsl.rs` | 液态玻璃：抓屏、SDF 折射、WGSL |
| `chrome.rs` | 自绘标题栏与窗口按钮 |
| `dupl.rs` | 桌面复制（DXGI）抓屏后端 |

---

## 5. 约定与坑（改代码前先看这 8 条）

1. **精度是十进制位数**（默认 80）：`BigFloat` 的 `precision` 不是比特。
2. **能精确就精确**：`1/3` 不能变成 `0.333…`；写新运算时先想"这一步会不会把精确值降级"。
3. **`W` 的两条纪律**：系数必须**首一**、DK 初值**不能共线**。违反会自激发散，
   看起来像"算错了"（踩过三次）。见 `solver_poly::wide_iterate_converges_on_cubic`。
4. **测试串行**：`.cargo/config.toml` 里 `RUST_TEST_THREADS=1`。数制/精度是**进程级** `Atomic*`，
   并行会互相污染（实测单线程全绿、多线程偶发红）。
5. **`cargo build` 不编译 `#[cfg(test)]`**：本地复核要用 `cargo build --all-targets` 或 `cargo test`。
6. **cargo 的"假 Fresh"**：改了文件却还用旧二进制时，`cargo clean -p <crate>` 或 `touch` 源文件。
   症状是"测试数量没变/改动没生效"。
7. **快照基线不入库**（`.gitignore` 忽略 `tests/snapshots/*.png`）：每台机器自己生成；
   改完界面要删掉重跑，并**用 Read 打开 PNG 看一眼**，不能只看测试变绿。
8. **加按钮/改文案会让大量快照失效**：这是预期，不是回归；重生后逐张确认。

---

## 6. 扩展指南

### 加一个内置函数（例如 `cbrt`）

1. `parser.rs` 的 `FUNCTIONS` 白名单加名字（**解析期**才认）；
2. `eval_function` 里加分支（参数个数、复数/实数分派）；
3. `core` 里加数值实现（能精确就给精确，否则走 `BigFloat`）；
4. `docs/DOC.zh-CN.md` + `DOC.en.md` 的函数表各加一行；
5. 加单测：`normal` / 边界（0、负数、超范围）/ 精确性（`cbrt(27)=3`）。
   ⚠️ 函数名若是单字母，会与**自定义函数**规则冲突 —— 内置一律用 ≥2 字符。

### 加一个求解器

1. 在 `parser.rs` 里让它产生新的 `EvalResult` 变体（或复用 `Symbolic`）；
2. 新文件 `solver_xxx.rs`，**文件头写清职责与边界**（和相邻 solver 的分工）；
3. `run_line` 里接上分派；
4. 补测试：典型用例 + 退化用例 + 不该接的时候要报明确错误（不是"未知错误"）。

### 改界面

1. 结果区整页替换：加 `ResultsPage` 的一个变体（**不要加布尔**）；
2. 输入相关：改 `expr.rs` 的 `insert_key`（唯一接缝），别在 `app/` 里拼字符串；
3. 改完跑 `cargo test -p hipercalc-gui`（含快照），再逐张看 PNG。

---

## 7. 验证手段（这个仓库怎么"证明它是对的"）

| 手段 | 命令 | 覆盖什么 |
| --- | --- | --- |
| 单元/集成测试 | `cargo test` | 211 条（CLI）+ 28 条（core） |
| GUI 测试 | `cargo test -p hipercalc-gui` | 157 单测 + 20 快照 |
| 静态检查 | `cargo clippy --all-targets -- -D warnings` | 零警告是硬要求 |
| 文档 | `cargo doc --no-deps --workspace --document-private-items` | 要求**零 rustdoc 警告** |
| 性能探针 | `cargo test --release -p hipercalc --lib perf_scan -- --ignored --nocapture` | 各功能耗时（**只能同进程比较**） |
| 视觉回归 | 删 `tests/snapshots/*.png` 后跑快照测试 | 布局/配色/排版 |

⚠️ **本机计时噪声极大**（同一题 8ms 与 36ms 都测到过）。性能结论一律用
**同进程 min-of-N** 或**可数量化指标**（如"函数求值次数"），跨时刻的绝对秒数不可比。

---

## 8. 术语表

| 词 | 含义 |
| --- | --- |
| **MathIO / LineIO** | 结果的两套显示风格：精确式优先 / 小数优先 |
| **Fast / Deep** | 计算模式：带规模护栏 / 死算到底 |
| **首一（monic）** | 多项式最高次项系数为 1；DK 迭代的步长公式要求如此 |
| **Bareiss** | 分数自由消元：每层除以"上一个主元"，整数矩阵下整除 |
| **DK** | Durand-Kerner：同时迭代出全部复根 |
| **`over`** | `W` 的溢出标志；置位表示该值不可信，调用方应退回 `BigFloat` |
