# 路线图（ROADMAP）

> 记录"接下来可以加什么"：每项都标注**现有基础 / 要改的接口 / 成本 / 风险**，便于单独立项。  
> 现有家底：59 个数学函数 + 15 个指令（`/help` `/clear` `/mode` `/lang` `/set` `/let` `/var` `/del` `/reset` `/save` `/load` `/exit` `/q`），  
> 双 crate workspace（`hipercalc-core` 数值底座 + `hipercalc` 应用），156 项测试，CI 三平台 + lint。

按"价值 ÷ 成本"排序，**建议每次只做一项并单独提交**（这是本仓库一贯的节奏）。

---

## 1. 自定义函数 `f(x) = x^2 + 1`

**价值最高**：现在只有 `/let NAME = 值`（存常量），无法定义带参数的函数，重复推导时只能复制粘贴。

- **现有基础**：`Evaluator.vars: BTreeMap<String, Number>`（`parser.rs:744`）；`evaluate_with_vars` 已支持多变量替换。
- **要改的接口**（4 处，注意第 1 条是**硬约束**）：
  1. **解析期白名单**：函数名在**解析阶段**就对着 `parser::FUNCTIONS` 校验（`parser.rs:656` 报 `未知函数`）⇒  
     `f(3)` 会在**求值之前**被拒。必须改成"未知名字先接受为函数调用节点、由求值期决定是否报错"，  
     否则自定义函数根本走不到求值层。**这是本项目最需要注意的回归点**（涉及多字母拆分 `xy` = x·y 的判定）。
  2. 求值器新增字段：`funcs: BTreeMap<String, (Vec<String>, Expr)>`（名字 → 形参 + 函数体）。
  3. `eval_function` 里查表：把实参绑定到形参后用 `evaluate_with_vars` 求值函数体。
  4. 定义语法识别：在 `run_line` 入口检测 `IDENT(params) = 表达式`（**不要**改语法分析器，行级识别成本最低）。
- **顺带要做**：`/fn` 或复用 `/let` 的展示与删除；`state.rs` 持久化（否则重启即丢）；  
  `/help` 三语；`FUNCTIONS_META` 无关（用户函数不在表里，注意提示逻辑要跳过）。
- **成本**：中（parser 校验策略 + 求值器 + 持久化 + i18n + 测试）。
- **风险**：中——动的是解析期的名字判定，必须有"`xy` 仍是 x·y"这类既有测试兜底（现已存在）。

## 2. 数制与位运算　✅ 已完成

**位运算已实现**（6 个函数：`and` / `or` / `xor` / `not` / `shl` / `shr`，只接受非负整数，负数的补码语义明确拒绝；  
`not(a)` 与 Python 一致取无限宽补码 `~a = -a - 1`；移位位数上限 1e6）。测试见 `cli_tests::bitwise_functions`。

**已完成**：位运算 6 函数 + 进制字面量 `0x`/`0o`/`0b`（大小写均可，纯解析期）+ `/base dec|hex|oct|bin` 结果数制  
（显示层开关，只作用于整数：小数与根式仍按十进制，避免"0.5 的十六进制"这种伪答案；带 `0x`/`0o`/`0b` 前缀以便粘回输入）。

**原设计的其余说明**（保留备查）——

- **位运算函数**：`and` / `or` / `xor` / `not` / `shl` / `shr`，照 `mod` / `gcd` 的形状加即可  
  （`TWO_ARG_FUNCTIONS`、`FUNCTIONS`、`FUNCTIONS_META`、`eval_function` 四处同步；整数参数用现成的 `as_int` / `as_nonneg_int`）。
- **数制输出**：`hex` / `bin` / `oct` **不适合做成函数**——`Number` 里没有字符串类型；  
  应做成**显示模式**（`/mode hex` 之类），与既有 MathIO / LineIO 同一层（`calc_mode` + `display`），这才是贴合架构的做法。
- **顺带**：十六进制字面量输入（`0xFF`）、`fib` / `Catalan` 之类的整数序列函数（见第 4 项）。
- **成本**：低（函数）/ 中（显示模式）。**风险**：低。

## 3. LaTeX 输出模式　✅ 已完成

**已完成**（`/mode latex`）：见下；原评估——**性价比第二高**：MathIO 的符号输出已经很接近 LaTeX，加一个 `/mode latex` 即可让结果直接粘进论文。

- **现有基础**：`display.rs` 的 MathIO/LineIO 双通路 + `DisplayMode` 枚举（已是"模式"形态，扩展点现成）。
- **要改**：`DisplayMode` 加 `Latex` 分支；`format_mathio` 的对应实现（分数 `\frac{}{}`、根式 `\sqrt{}`、  
  上下标 `x^{2}`、`\pi` / `\cdot` / `\times` 等）；`/mode` 的取值与帮助；`state.rs` 持久化。
- **实现方式与计划不同（更稳）**：计划里写的是"给 `DisplayMode` 加 `Latex` 变体"，实际发现  
  `DisplayMode::` 在全仓有 **87 处引用**，加变体会牵动一大片 match ⇒ 改用**显示层开关**  
  （`display::set_latex`，与数制开关同层）。LaTeX 只影响"结果怎么排"，本质就是显示层的事。
- **渲染器是平行实现**：`format_exact_expr_latex` / `format_term_latex` 与 MathIO 版并列  
  （`ExactTerm` 只有 4 个变体，重复这几行比让原路径承担新分支便宜）。已覆盖分数 → `\frac{}{}`、  
  根式 → `\sqrt{}`（系数 1 省略、-1 只留负号）、`\pi`、整体分母、负号提到分式外。
- **已知边界（留待后续）**：只覆盖**数值/符号结果**的渲染；方程解、因式分解、高等数学那些  
  走各自 formatter 的高级输出仍是 MathIO 风格（未 LaTeX 化）。科学计数法的 `×10^24` 也未转成 `\times 10^{24}`。
- **注意**：没有用第三方 LaTeX 库（项目坚持零额外依赖）。
- **成本**：低到中。**风险**：低。

## 4. 特殊函数扩充　✅ 已完成

每个函数相互独立，可**逐个提交**、逐个加测试。

**已完成（整数精确）**：`fib(n)`（快速倍增法，O(log n)）、`catalan(n)`、`doublefac(n)` —— 快速模式有上限、`/mode deep` 可取消。  
实现踩过一坑：catalan 原写成每步"acc*(n+k)/k"，`1×7/2` 被整数截断 ⇒ n=5 算出 36（正确 42）；改成分子分母各自累乘、最后除一次才对。

**待做（需要先解决表示/算法问题）**：

- `gamma`：Γ(1/2)=√π，而现有 `ExactTerm` 只能表示"有理数 × √整数"，**√π 不是精确可表示的形式** ⇒  
  要么限制为整数与半整数、半整数走数值（标 ≈），要么给类型加 π 的根式表示。**先做决策再动手**。
- `erf` / `zeta`：需要 BigFloat 上的级数/连分数（`exp` 已有，可复用），属实数分析，工作量比上面几个大。
- **建议优先**：`gamma`（Γ，整数与半整数给精确值）、`erf`、`fib`（含 `fib(n)` 通项与快速幂）、  
  `doublefac`（双阶乘）、`catalan`、`zeta`（ζ，至少 ζ(2)/ζ(3) 给闭式提示）。
- **现有基础**：`fac` / `primefac` / `nCr` / `nPr` 的整数快速路径可复用；`BigFloat` 已有 exp/ln/级数。
- **要改**：`FUNCTIONS` + `FUNCTIONS_META`（元数与签名）+ `eval_function` + 测试；涉及新报错文案才动 i18n。
- **成本**：低（每个函数 20~50 行 + 测试）。**风险**：低；唯一要小心的是**精确性判定**（该给 `=` 还是 `≈`）。

## 5. 统计与概率　✅ 已完成

**参数形态（用户已选定）：B + D 并存**
- **B 表达式 + 范围**：`mean(k, k, 1, 4)` → `2.5`（与既有 `sum`/`prod` 完全一致，零新语法）
- **D 方括号列表**：`mean([1,2,3,4])` → `2.5`（一眼看出"数据而非表达式"）
- 顺带天然支持 A：`mean(1,2,3,4)`（≥2 个裸参数即按数据列表处理）
- 判别规则（要写进帮助）：4 个参数且第 2 个是**变量** ⇒ 范围形态；否则按数据列表。**规则精确，不靠猜。**

**方差口径（用户已选定）：两个都提供**
`var` / `stddev` 除以 n（总体）；`var_s` / `stddev_s` 除以 n−1（样本）。与 Excel 的 VAR.P / VAR.S 一致。

**实现路线（已探明的关键约束，下次直接照做）**
1. **不要给 `Expr` 加"列表"变体**：`Expr::Number` 在 **14 个文件**里被穷尽匹配（含 calculus 全部子模块与各 solver），
   加变体会牵动一大片。改用**解析期脱糖**：`[e1, e2, …]` 解析成 `Function("list", [e1, e2, …])`，**零 AST 波及**，
   而且为第 8 项的矩阵 `[[1,2],[3,4]]` 铺好了同一套语法的路。
2. **统计函数在"构造函数调用处"就地展开**（`parse_identifier_or_function`），不改 `Evaluator`：
   - `mean(f, k, a, b)` → `sum(f, k, a, b) / (b - a + 1)`
   - `mean([x1…xn])` → `(x1 + … + xn) / n`
   - **方差用 `E[X²] − E[X]²`**：`var = sum(f²,…)/n − mean²`。选这个式子是因为它**不需要变量替换**
     （"先算均值、再逐项求偏差"需要把均值代回 f 的 AST，麻烦且易错）。
   - `var_s = var · n/(n−1)`；`stddev*` 再套一层 `sqrt`。
3. **测试**：期望值全部可手算 —— `mean([1,2,3,4]) = 2.5`、`var([2,4,4,4,5,5,7,9]) = 4`（总体）、
   `var_s(同组) = 32/7`、`stddev([1,2,3,4,5]) = sqrt(2)`。

**`median` / `percentile` / `corr` 也已完成** —— 排序问题用"**解析期就地求值 + 排序 + 替换成常量**"解决：
`concrete_values` 把数据点算成 `Number`（范围形态用 `evaluate_with_var` 逐点代入），排序后取位，
结果作为常量表达式返回。选这条路的代价是数据点必须在解析期可求值（引用 `/let` 变量没问题）。

**实测**：`mean([1,2,3,4])=5/2`、`mean(k,k,1,4)=5/2`、`mean(k^2,k,1,4)=15/2`、
`var([2,4,4,4,5,5,7,9])=4`、`var_s=32/7`、`stddev([1,2,3,4,5])=sqrt(2)`、`stddev_s=(1/2)*sqrt(10)`、
`median([3,1,2])=2`、`median([1,2,3,4])=5/2`、`percentile(25,[1,2,3,4,5])=2`、`corr([1,2,3],[2,4,6])=1`。

**另一个坑**：统计展开会**生成 `sum(...)` 节点**，而那个节点要靠 calculus 那一遍展开 ⇒
解析期预扫 `had_calculus` 必须把"输入里有统计函数名"也算进来，否则 `mean(k,k,1,4)` 会残留未展开的 sum
（症状是"未定义变量: k"）。

## 6. 单位与物理常量

**整类为空**（没有任何 mean / median / var）。

- **建议**：`mean`、`median`、`var`、`stddev`、`percentile(p, ...)`、`corr`（相关系数）、`sum` / `prod` 已能做的"级数统计"。
- **要决定的**：参数形态——是"多个数"还是"一个表达式 + 范围"（推荐后者，和 `sum(f,k,a,b)` 一致，能复用现有求值循环）。
- **成本**：中（新函数族 + 参数形态设计）。**风险**：低。

## 6. 单位与物理常量

- **常量**：`c`（光速）、`h`、`G`、`NA`、`k_B`…（进 `CONSTANTS`，注意与现有单字母变量 `e` / `i` 的**命名冲突**）。
- **换算**：`km` / `mile` / `℃` / `℉` 之类。**关键设计问题**：单位要有"带单位的量"类型，  
  而 `Number` 没有单位概念 ⇒ 要么引入量纲类型（成本高），要么只做**函数式换算**（`to_mile(km)`，成本低）。  
  建议**先只做函数式换算**，把量纲留作独立议题。
- **成本**：低（常量）/ 中（换算）。**风险**：低到中（命名冲突要提前查）。

## 7. 不等式求解

- **形态**：`x^2-4<0` → `-2 < x < 2`；需要引入**解集**的表示（区间/并集）与渲染。
- **可复用**：方程求解的全部通路（因式分解 + 根收集）⇒ 本质是"求根 + 定符号"，**因式分解已经把最难的部分做完了**。
- **要改**：比较运算符的解析（现在只认 `=`）、解集类型与三语渲染、`run_line` 的分派。
- **成本**：中到高。**风险**：中（解集渲染的多语言与格式一致性）。

## 8. 矩阵与线性代数

**成本最高**：要引入矩阵值类型与字面量语法。

- **可复用**：线性方程组的高斯消元（`solver_linear.rs` 已有）
- **建议先做**：`det`、矩阵乘法、转置、秩；逆矩阵与特征值往后放。
- **要改**：新类型（`Value::Matrix`）、字面量解析（`[[1,2],[3,4]]`）、各求值分支的"非标量"处理、  
  显示层、`EvalResult` 新增变体。**几乎所有层都会被牵动**。
- **成本**：高。**风险**：高——建议单独一个里程碑，不要和其它项混做。

---

## 已登记但未做的技术债

1. **左括号自动配对**（`(` 自动补 `)` 且光标落在中间）——  
   实现方向已找到（借 `Cmd::Complete` 通路，`Completer::update` 拿得到 `&mut LineBuffer`），  
   但在 `)(` 相邻分组下会错位（`(x-2)(x+3)+2x=12` → `(x-2x+3(+2x=12)`），两种实现都复现，  
   说明对 rustyline 循环补全的调用时机假设还没对上。详见 `change_logs/change_log40.md`。  
   **下一步**：给 `Completer::update` 加调用序列日志，在真实终端抓清流程再动手。
2. **clippy 的 2 条 error（疑似精度隐患）**：  
   `crates/hipercalc-core/src/bigfloat.rs:236` 与 `:347` 的 `clippy::approx_constant`（近似常量，涉及 `LOG2_10`）。  
   若那里用截断常量做范围归约，偏差会渗进 exp/ln 的结果 ⇒ **优先级应高于其余 11 条风格告警**。  
   （CI 的 clippy 目前是**非阻塞**阶段一，等于是个收集器；这两条要先查。）
3. **非 Windows 的计算中中断**：`cancel.rs:107-110` 明说不支持；README 已改成准确表述（不再是缺陷，是待办）。
4. **复数非整数次幂**：`number.rs:363-369` 暂不支持（`e^(i*pi)` 就落在这里）。

---

## English summary

Ordered by value-to-cost; do **one item per commit** (the repo's usual rhythm).

1. **User-defined functions** `f(x) = …` — highest value. Hard constraint first: function names are validated  
   against the whitelist **at parse time** (`parser.rs:656`), so `f(3)` is rejected before evaluation; the check  
   must be relaxed so unknown names reach the evaluator.
2. **Number bases & bitwise ops** — pure addition, lowest risk (`and/or/xor/not/shl/shr` as functions;  
   `hex/bin/oct` must be a **display mode**, since `Number` has no string type).
3. **LaTeX output mode** — MathIO is already close; add a `DisplayMode::Latex` branch (no external LaTeX crate).
4. **Special functions** — Γ / erf / fib / double factorial / Catalan / ζ, one commit each.
5. **Statistics** — `mean/median/var/stddev/percentile/corr`; biggest design choice is the argument shape.
6. **Units & physical constants** — start with function-style conversion; a real quantity type is a separate topic.
7. **Inequalities** — root finding already exists; needs a solution-set type and rendering.
8. **Matrices & linear algebra** — highest cost; touches nearly every layer; needs its own milestone.
