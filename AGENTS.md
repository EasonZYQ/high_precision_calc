# AGENTS.md

`hipercalc`：中文超高精度命令行计算器。单 crate Rust 项目（edition 2024），**含 62 项单元测试（分布于 9 个源文件）、暂无 CI、已发布到 GitHub（SSH 远端 `git@github.com:EasonZYQ/high_precision_calc.git`，默认分支 main）**。

## 硬性要求（用户指定）

- 思考过程与代码注释一律用中文，注释不要用英文。
- 本项目已是 git 仓库并发布到 GitHub（SSH 远端 `git@github.com:EasonZYQ/high_precision_calc.git`，默认分支 main）；可以执行 git 提交/分支操作，但提交前务必确认暂存区不含 `target/`、`.workbuddy/`、`.trae/` 及日志文件。
- 每次编辑代码后，要在工作目录下的 `change_logs/` 文件夹内新增一个变更日志文件（`change_log1.md`、`change_log2.md`……序号依次加一），用几句话简述本次项目编辑内容，可包含代码讲解。**该目录已加入 `.gitignore`，仅本地保留，不要提交到仓库。**
- 每次编辑代码或修改增加功能，如果需要的话应当对README.md和AGENT.md进行更新。

## 构建与验证

```powershell
cargo build
cargo run            # 交互式 REPL（rustyline）
cargo test           # 62 项单元测试
```

- 已有 `#[cfg(test)]` 单元测试（**62 项**，分布于 9 个源文件，`cargo test` 全绿），
  其余验证靠管道喂输入人工核对结果：  
  `Write-Output "1+2*3", "x^2-4=0", "x+y=5,2x-y=1", "sin(pi/6)", "/exit" | cargo run`
- 新增了**非交互入口**：`-e "表达式"`（裸结果、无耗时行）、`-f 脚本文件`、`--stdin`、
  `--lang <代码>`（强制语言，便于三语断言）、`-q`（不输出耗时）。退出码 0/1/2，参数解析在 `parse_args`（带单元测试）。
  **管道默认仍是 REPL**（不带参数时横幅/提示符不变），脚本化断言请用显式参数。
- 管道/重定向场景下 stdout 不是终端，**动态计时行不会出现**，每次运算后只多一行 `用时：N秒`；
  动态刷新（`\r` + 清行）只能在真实终端里肉眼确认，需要逐字节核对时用 Python 读输出文件并按 `\r` 切分查看。
- Windows 文件锁：程序还在运行时会构建失败（`failed to remove ... hipercalc.exe (os error 5)`）。  
  先 `taskkill /f /im hipercalc.exe 2>$null` 再 `cargo build`。
- 工具链 stable-x86_64-pc-windows-msvc。若报 `linker link.exe not found` 属于 VS/MSVC 安装环境问题，不是代码问题。
- `Cargo.toml` 有 `[profile.dev.package."*"] opt-level = 3`：**只优化第三方依赖**（num-bigint 等大数库在
  opt-level 0 下慢一个数量级以上，是 debug 慢的主因），本项目源码仍为 opt-level 0。不要删掉这段，
  也不要改成 `[profile.dev] opt-level`（那会牺牲本项目断点调试体验）。
- 性能排查用"程序内部 `用时：N秒` 行 + 外部硬超时"组合：外部超时用
  `WaitForExit(60000)`（**必须在 `ReadToEnd` 之前等**，否则读流阻塞会让超时形同虚设），超时即 `Kill()`。

## 模块职责

| 文件                    | 职责                                                                                     |
| --------------------- | -------------------------------------------------------------------------------------- |
| `bigfloat.rs`         | 任意精度浮点：值 = BigInt 尾数 / 10^precision；PRECISION=80、DISPLAY_DIGITS=20。四则/开方/幂/exp/ln/三角级数 + 规模保护 |
| `bigint_ext.rs`       | 大整数补充运算：按规模分派的自带长除法（绕开 num-bigint 的 BZ 断言缺陷）+ 自带整数平方根；带单元测试 |
| `calc_mode.rs`        | 计算模式开关：Fast（默认，超限给提示）/ Deep（死算，取消规模上限 + 完整精度输出）；低层护栏统一查 `is_deep()` |
| `i18n.rs`             | 界面语言（简/繁/英）：以简体原文为键的词条表 + 运行期整行模式翻译 + 系统语言探测（Win32 `GetUserDefaultUILanguage`） |
| `number.rs`           | `Number::{Exact, Approx}` 双重表示，运算先精确后数值回退                                              |
| `parser.rs`           | 递归下降解析 + 求值器 + `parse_and_eval`（返回 `EvalResult`）；绝对值/阶乘/多参函数                           |
| `trig.rs`             | 特殊角精确值表（度/弧度/反三角）                                                                      |
| `display.rs`          | mathio 符号输出 / lineio 20 位有效数字                                                          |
| `equation.rs`         | 方程信息提取、多项式/线性判定；`has_trig_of_var`（自变量是否为角度，供根收集的初值换算用）                                                                        |
| `solver_linear.rs`    | 线性方程组求解（高斯消元），解按显示模式 + `=`/`≈` 输出 |
| `solver_poly.rs`      | 多项式求根：系数提取、二次精确解、Durand-Kerner 全复根、单根牛顿                                                |
| `solver_nonlinear.rs` | 非线性方程组两阶段多维牛顿（粗扫 + 精收敛）                                                                |
| `solver_fit.rs`       | 多项式函数拟合：坐标 → 解析式（2 点一次、3 点二次…）；大写 `P` 为顶点；模板可带未知参数，欠定则输出参数关系 |
| `solver_triangle.rs`  | 三角形求解：`triangle(a=3 b=4 c=5)` / `triangle(a=3, b=4, c=5)`（括号内空白或逗号分隔，**只能是整个表达式的最外层函数**）→ 三边三角三高 + 面积周长两半径；SSS/SAS/ASA/AAS/SSA 走解析解，其余数值兜底，SSA 可给两解 |
| `solver_factor.rs`    | 多项式因式分解（单/多元，按模式区分实数域/有理数域）                                                            |
| `solve_aux.rs`        | `=`/`≈` 前缀、周期通式、根收集等辅助工具                                                               |
| `state.rs`            | 会话状态持久化：显示/角度/计算模式 + `/let` 变量 + `/set` 颜色的读写（`~/.hipercalc_state`）                              |
| `main.rs`             | REPL、`/` 指令、`/help` 正文与着色器、颜色配置（`/set` 持久化）、rustyline 实时高亮、变量存储、历史持久化、运算计时（`Timing`） |
| `README.md`           | 功能手册 + 代码架构文档；改动功能后须同步                                                                 |

## 容易改错的地方

### 精度（bigfloat.rs）

- `BigFloat::div` 中 `result_precision = a.precision + scale - b.precision` 的修正不可删改——算错会静默污染 sqrt/pi/三角结果（症状：结果整体缩小 10 的幂次倍）。  
  该值恒 ≥ 0（`scale == 0` 时必有 `a.precision ≥ target+MARGIN+b.precision`），因此**不要再加"反向缩放"分支**（旧分支不可达且量级少乘 10^b.precision）。
- `sqrt` 用整数平方根做初值 + 牛顿迭代；`atan` 先用 `2*atan(x/(1+√(1+x²)))` 缩半参数再展级数；`ln` 先按 2 的幂把参数缩放进 [1,2) 再用 atanh 级数（大参数也必须保证 20 位正确）。改级数前先确认收敛域。
- `exp` 必须**先缩半参数**（`exp(x)=exp(x/2^k)^(2^k)`，`|x/2^k| ≤ 1`）再用**相对**判据 `|term| ≤ |result|·10^-work_prec` 收敛，并受 `EXP_ARG_LIMIT_LOG10` 限制。  
  不要改回绝对判据/无上限——旧实现遇到大 |x| 时级数永不收敛、每项整数位数随 |x|·k 爆炸（卡死 + 静默错误）。
- `pow` 带规模保护并返回 `Result`：整数指数走快速幂前先 `check_pow_scale`（结果位数 ≤ `MAX_RESULT_DIGITS`）；  
  一般情形先检查 `|b·ln a|` 量级。指数超出 u32/i32 时**不能**静默落到 `exp(b·ln a)`（旧实现返回完全无关的量级）。
- `to_significant_string`：整数部分超过 `max_digits` 时必须走 `to_scientific_string`（科学计数法保留量级），  
  不能只按字符串截断（旧实现把 `10^25/3` 显示成 `3.3e19`）。
- **大整数除法/开方必须走 `bigint_ext`**（`bigint_ext::div`、`bigint_ext::int_sqrt`）：
  num-bigint 0.4.7 的 Burnikel-Ziegler 除法在"被除数 ≈ 除数²"形状下会触发
  `debug_assert!(ah < b)` 直接 panic（实测 `sqr(10^100000)`、`(10^200000+pi)/(10^100000+pi)`）。
  `bigint_ext` 按 limb 规模分派：小规模仍用 num-bigint（快），大规模改用自带 Knuth D 长除/牛顿开方。
  **不要**把 `BigInt::sqrt` 或裸 `/` 加回这两个热路径，也不要删掉那两张单元测试表。
- 试除型循环（`simplify_radical`、`simplify_sqrt`、`extract_quadratic_factor`）的**上限要按工作量折算**，
  不能只限次数：每次试除都要对 n 取模/长除，代价 O(n)，固定 1e6 次在大 n 上会白跑几十秒。
  参考常量：`QUADRATIC_WORK_BUDGET = 40000`（试算次数 × 次数）、`max_p = 2e6 / limb 数`。
- 调用方注意签名：`BigFloat::exp`、`BigFloat::pow` 都返回 `Result`，`Number` 层用 `?` 传播到 `eval_function`。
- **常数缓存**：`pi` / `e` / `ln2` 三者按目标精度缓存在 `pi_cache()` / `e_cache()` / `ln2_cache()`（`OnceLock`+`Mutex`+`HashMap<usize, BigFloat>`），
  经 `cache_get`/`cache_put` 存取。它们都是迭代/级数计算，而牛顿迭代每轮求值都要用（pow 用 ln2、三角归约用 π、
  `ExactExpr::to_bigfloat` 每遇到 Pi/E 项都要重算）——**新增涉及这三者的代码不要绕过缓存直接写级数循环**，
  也不要"优化"成去掉缓存。缓存只在未命中时现算，锁中毒静默跳过，不影响正确性。
- **小整数 `ln` 缓存**：`int_ln_cache()` 以 `(BigInt, usize)`（底数, 目标精度）为键缓存 2..=10^6 的整数 `ln`，
  容量 64 满则清空。`pow(a,b) = exp(b·ln a)` 在牛顿迭代中反复调 `ln(a)`，故 `3^x=27` 曾比 `2^x=8` 慢 5 倍。
  取键时**必须按 precision 归一再判整数**（AST 常数经 `to_bigfloat` 后形如 `3·10^80`，`precision == 0` 判不出来）；
  只收小整数键（任意参数的 `ln(1+x)` 不入表），不要改成无界缓存。

### 符号运算（number.rs）

- 运算管线顺序：清零项 → `normalize_terms`（`Sqrt(x,1)`→`Rational`）→ `simplify_expr`。缺一步会导致分母有理化/约分失败。
- `simplify_expr` 求 GCD 只用各系数的**分子** + 表达式分母；把系数自身分母算进去会阻止约分（如 `4*sqr(2)/2` 无法化到 `2*sqr(2)`）。
- `div_exact` 需先处理除数分母 ≠ 1 的情况（`sqr(2)/2` 这类），再走单个 sqrt / 共轭有理化分支。
- "除数为单个 sqrt 项"分支：`self/(c·√r) = self·√r·(1/(c·r))`，`1/(c·r)` 要用 `scale_term_rational` 乘进**各项系数**。  
  只取 `c.numer()*r` 当分母会丢掉系数分母（`1/(sqr(2)/2)` 曾少除 2 倍）。
- `Number::int_pow` 展开 `(a√b)^n` 必须用 `a` 的**有理数**幂：偶次 `a^(2m)·b^m`、奇次 `a^(2m+1)·b^m·√b`。  
  用 `numer/denom` 整数截断会把 `1/2` 变成 0，且偶数次幂会少乘一次 `a`（`(2√3)^2` 曾得 6）。
- `Number::sqrt` 对有理数 `p/q` 走 `sqrt(p·q)/q` 得到精确根式（`sqr(1/2)` → `(1/2)*sqrt(2)`），不要删成纯数值回退。
- `Number::div` 除数为零时只做 `debug_assert!` + 返回 0 兜底（调用方须先判 `is_zero()`），  
  不能调用 `BigFloat::div`（其 `assert!` 会让进程 panic）。
- `Number::pow` 的整数指数分支先调 `check_pow_scale` 做规模保护。

### 解析器（parser.rs）

- 顶层入口 `parse_system()`（逗号分隔方程组）→ `parse_equation()`（单个 `=`）→ 表达式。不要另起入口绕过它。
- `Expr::Function(name, Vec<Expr>)` 已是**参数列表**：绝大多数单参，仅 `log(b, x)` 两参。内部 `"fact"`（`!` 生成）与 `"abs"`（`|x|` 生成）不在白名单内。
- 变量：单字母 `x y z a b...`（`VALID_VARIABLES`，大写段只到 W）+ 全大写存储变量（`/let`，可含 `_`：`X`、`AB`、`PI_VAR`）+ `ans`；`pi`/`e` 是常数。
- **函数白名单唯一来源是 `parser::FUNCTIONS`**（解析校验、多字母拆分判断、`main.rs` 高亮共用），  
  新增函数只需改它 + `Evaluator::eval_function`（旧实现有三份重复数组，容易漏改）。`fact`/`abs` 作为内部名不要加进白名单。
- `eval_function` 签名是 `(name, args: &[Number])`：单参函数开头统一校验 `args.len()==1`，`log` 校验 `len()==2` 并检查底数 >0 且 ≠1。
- `log` 的底数判定必须精确（`as_rational().is_one()` 或与 1 的偏差 > 1e-40），**不要用 `rounded(0) == 1`**  
  （会把 0.5/0.9/1.2 误判成 1）。
- 大参数防护：`check_exp_range`（exp/双曲，|x| ≤ 10^6）、`check_trig_range`（三角，弧度 |x| < 10^78，度模式按 ×π/180 折算）。  
  这些是防卡死/防结果失效的护栏，不要删。**`tanh` 是例外**：|x| > 100 时 80 位精度下已饱和，直接返回 ±1
  （阈值不能取 10^10——那会让 5·10^5 < |x| < 10^10 先撞上 exp 的参数上限而误报错，|x| > 10^10 才返回 1，行为不一致）。
- 新增数学函数只需改 `parser::FUNCTIONS` 与 `Evaluator::eval_function` 两处（高亮常量引用前者自动同步）。
  反双曲/双曲倒数都走现成的 `BigFloat` 组合（`ln`/`sqrt`/`exp`），**不要**新写级数；
  `cbrt` 走 `Number::cbrt()`：先判完全立方（`bigint_ext::int_nth_root` 回验）给精确值，否则 `BigFloat::nroot`。
- `BigFloat::nroot(k)` 是"exp(ln/k) 取初值 + 牛顿迭代细化"，牛顿步是 `y ← ((k-1)y + x/y^(k-1))/k`；
  直接返回 exp(ln/k) 会在尾部丢几位有效数字，别改成那种写法。
- 两参函数登记在 `parser::TWO_ARG_FUNCTIONS`（`eval_function` 按它校验参数个数；`log` 有专门的报错文案，单独处理）；
  新增两参函数记得同时加进该表与 `FUNCTIONS`。
- **三套除法语义不可混用**：`Number::idiv`/`modulo` 是 Euclidean（`div_euclid`/`rem_euclid`：余数恒非负、`b<0` 时结果与 `floor(a/b)` 不同）、
  `bigint_ext::div` 是截断除法（余数符号随被除数）、`Number::div` 是有理数精确除法。改动任何一个都要看 `mod_and_idiv_are_euclidean` 测试。
- `bigint_ext::is_prime` 是小素数试除 + Miller-Rabin；试除循环里**必须**有 `if p >= n { break }`，
  否则 n 本身会被自己的试除判成合数（会让 `isprime(101)` 返回 0、`nextprime` 死循环）。
- 命令行 `--lang` 与 `-q/--no-timing` 都是**会话级覆盖**：只改 `i18n` 的当前语言 / `calc_mode::timing_enabled()`，
  **不写状态文件**；要落盘必须走 `set_language()`（同时更新 `AppState::lang_base`）或 `/timing on|off`
  （同时更新 `AppState::timing_base`）。`persist_state` 只写 `lang_base` 与 `timing_for_persist(state)`。
  **坑**：早期 `persist_state` 直接落盘 `calc_mode::timing_enabled()`，于是跑一次 `hipercalc -q ...`
  就把用户配置永久改成 `timing=off`（症状：之后每次启动都不显示耗时行）；`cli_q_does_not_leak_into_saved_timing` 守住这个不变量。
- 常数 `tau`（= 2π）与 `phi`（= (1+√5)/2）必须放在 `parse_identifier_or_function` 的常数 match 里
  （`_ =>` 多字母拆分之前），否则 `tau` 会被拆成 `t*a*u`。`phi` 用 `Number::phi()`（add/mul/sqrt 管线拼出精确形式）。
- 标识符拆分：多字母串只消费第一个字符并回退位置（`xy^2 = x*(y^2)`）；以 `e` 开头的串先取常数 e（`2ex = 2*e*x`），  
  所以拆分条件里**不能**再写 `c != 'e'`。
- 对数精确识别（`power_of_two`/`power_of_ten`/`factorize_rational`/`rational_log_exact`）：底真同为某数整数次幂时返回精确有理数。`power_of_two` 必须用 `1<<k == n` 精确比对，不能只判移位后剩 1（`3>>1==1` 会误判 3 是 2 的幂）。
- 绝对值 `| |` 依赖 `Parser.abs_depth` 区分开/闭符：`abs_depth==0` 时 `|` 是隐式乘法开始（`2|x|`），`>0` 时是闭合符（`|5|`）。删掉 abs_depth 会把闭合 `|` 当乘法吞掉导致解析错乱。
- 逆三角的角度转换在 `parser.rs::to_degrees_if_needed`（度数模式把 pi 倍数换算成度），不要挪进 `trig.rs`。

### 求解器

- `solver_factor.rs`：分解域由显示模式决定（MathIO 实数域可拆 sqrt、LineIO 有理数域）；
  `format_sqrt_pair` 的两个因式之间必须有 ` * `（旧实现直接拼接 `(x - sqrt(2))(x + sqrt(2))`）；
  一元分解走到 `rdeg == 2` 残余说明有理根已枚举失败 ⇒ LineIO 下必须置 `has_irreducible`（提示"有理数域内不可再分解"），
  MathIO 下若 `push_quad_factor` 成功拆 sqrt 则不加提示；多元的二次残余仍按"已完整处理"不提示（未做完全因式分解尝试）；
  下列"上限"在 **Fast** 下生效，`calc_mode::is_deep()` 为真时全部放开（死算）：
  - `simplify_sqrt`：先判完全平方再做小质数试除，**试除上限 10^6**（旧实现 10^13 = 万亿次循环，界面等效卡死）。
  - `int_divisors`：试除在 **u64** 上做（Fast 下输入都 < 10^13），不要改回 BigInt 取模（debug 下慢 100 倍以上）；
    Deep 下取消 1e13 限制，超过 u64 时走 BigInt 试除分支。
  - `extract_quadratic_factor` 有 `QUADRATIC_TRY_BUDGET = 5000` 次长除预算；`QUADRATIC_BMAX_LIMIT = 200000`（Deep 下取完整柯西界）。
  - `rational_root_candidates` 的 200 个候选上限同样只在 Fast 下生效。
  - 结果用 `FactorOutcome { factors, has_irreducible, truncated }`：截断必须提示"结果可能不完整"，  
    且**要覆盖 rdeg == 2 的剩余**（旧实现只在 rdeg ≥ 3 时检查，二次剩余被静默漏报）。
  - `factor_expr(evaluator, expr, mode)` 需要 `Evaluator`：`ans` 与 `/let` 存储变量按数值代入，  
    且变量列表要过滤掉已存储的大写变量。
- `solve_aux.rs::format_periodic_roots(roots, var, angle_mode)`：周期通式按"单周期等差或交替两族间距"识别。
  **入参若为度值会先 `×π/180` 折算成弧度**，之后判定完全沿用同一套 π 逻辑，标签再按单位分派
  （`pi_frac_str` → `π/6`；`deg_str` → `30`）—— 两种模式因此完全同构，**不要在判定里直接混用度值**。
  多族保持残差序避免 π/6 优先的事实排序。
  - **判定前必须保留 `±3π` 窗口**：牛顿从远处初值会落到很远的根（`sin(x)+cos(x)=1` 会混进 ≈29.8），
    它们让相邻差不再规整、把整条通式判否；窗口只作用于判定，**不要**用它裁剪回退分支要列出的根。
  - 周期与残基用 `pi_ratio`（分母上限 `PI_DEN_MAX = 12` 的**通用有理逼近**）识别，
    取代了原先的 `d_m` / `r_c` 固定候选表——固定表覆盖不到 `5π/3`，`cos(x)=0.5` 在两种模式下都会退化成数值列表。
  - 残基去重必须是"相对 + 绝对"容差（`tol·max(|a|,|b|) + 1e-40`）并**按识别标签再去重**，
    否则接近 0 的残基（如 `x*sin(x)=1e-30`）会让同一族重复输出多遍。
  - 族的分隔符是 `"  或  "`（两侧各两个空格）：要与 `i18n.rs` 的词条键逐字一致，
    否则英文模式下 `或` 会残留成中文。
- `solve_aux.rs::collect_all_roots` 的初值（`0, ±1..±6, ±1/2, ±3/2, ±k·π, ±k·π/2`）是**按弧度校准**的。
  当方程含"以未知量为角度的前向三角函数"（`equation::has_trig_of_var`）且当前是**角度模式**时，
  必须把整套初值 `×180/π`：否则 `±k·π ≈ ±3.14°` 太小、牛顿 20 步走不到 `30°/150°`，
  而 `±1..±6` 又会过冲落到远端根，根集合残缺 ⇒ 周期通式判不出来。
  换算后 `Δx_度 = (180/π)·Δx_弧度`，牛顿轨迹与弧度模式**在物理上完全一致**。
  **非三角方程（`1/x=2`、`ln(x)=1`）的自变量是普通实数，绝不能换算**——实测会把根全丢光。
- `solver_poly.rs::newton_solve`：必须有发散保护（`NEWTON_ABS_LIMIT = 10^6`，修正量与 |x| 双判），
  否则指数/对数方程的某些初值会让 x 跑到 1e8 量级、后续 exp 中间量位数爆炸（卡死）。
  收敛阈值 `zero_compare`、步长 `h` 都在**循环外**算一次（循环内重算会白做大量 BigInt 除法）。
  **所有 `evaluate_with_var` 的结果都要先判 `is_complex()` 再 `to_approx()`**：
  `ln(-1)`、`sqrt(-4)` 会返回复数，而 `Number::to_approx` 对复数带 `debug_assert`
  （初值里就有 `-1`，所以 `ln(x)=1`、`sqrt(x)=2` 曾经直接 panic 退出）。
- `durand_kerner` 的次数护栏返回的错误以 `solver_poly::DEGREE_GUARD_PREFIX` 开头；
  `main.rs::handle_equation` 据此判断"明确拒绝"并**不再回退牛顿单根**（高次多项式每次求值都要算 x^k，
  回退会让界面长时间无响应且只给一个根）。新增类似护栏时要么复用该前缀，要么同步改 main 的分支。
- 高次多项式（`is_polynomial` 为真但 `collect_terms` 只认字面量数字指数）依赖解析期的
  `fold_const_int`：`x^(2-1)` 会在解析阶段折叠成 `x^1`，否则会被判成"非多项式"交给牛顿法
  （`x^(2-1)-x=0` 曾输出一长串伪根）。改动 `parse_power` 时不要把这个折叠去掉。
- `solver_poly.rs::durand_kerner`（高次多项式全复根）的实现有四条不可回退的约定：
  - **初值半径** `R = min(柯西界, max(|a₀/aₙ|^(1/n), 1))`，n 个起点按 `2π(k+0.5)/n` 撒在半径 R 的圆上。
    柯西界 1+max|a_i| 对稀疏多项式过宽（`x^96+…+1` 的界为 2 而根模为 1），会拖到几百轮不收敛；
    旧实现的 `(0.4+0.9i)^k` 几何衰减初值更差（x^100=1 曾 368 秒且输出错误根）。首轮未收敛才退回柯西界半径重试。
  - **两阶段精度**：先 40 位粗收敛（`rough_prec`，步数上限 300），再 80 位精化（上限 200）。
    每轮是 O(n²) 复数乘除，成本由 BigInt 位数决定，低精度阶段单轮开销约为全精度的 1/2~1/3。
  - **顺序更新**（Gauss-Seidel 型，用已更新的 `zs[j]`）而非"全部同时更新"，收敛明显更快；
    且**未收敛必须返回 `Err`**，绝不能把中间迭代值当根输出（调用方 `solve_poly_full` 用 `?` 传播）。
  - **Fast 次数护栏** `DK_MAX_DEGREE_FAST = 200`：n≈200 约 25 秒、n≥300 超过 1 分钟，故 Fast 下超限直接提示
    `/mode deep`；新增其他高复杂度的数值路径时参照此风格加护栏。
- `solver_poly.rs` 的 Cx 复数运算（`cx_add`/`cx_sub`/`cx_mul`/`cx_div`/`poly_eval_cx`）都带 `prec` 参数，
  DK 两阶段才能用不同精度；**不要把它们改回硬编码 `bigfloat::PRECISION`**。
- `solver_nonlinear.rs`：两阶段牛顿参数（粗扫 `16,12,true` 前向差分 / 精收敛 `80,45,false` 中心差分）影响速度与精度，改动需回归 `x+sin(y)=1, y+cos(x)=1` 类系统；"步长极小"的提前返回必须再过残差校验。
- `solver_linear.rs::format_linear_solution(sol, mode)` 需要 `DisplayMode`：解要按模式输出并带 `=`/`≈`。
- 单方程求根输出同样按显示模式：`solver_poly::format_solution(sol, mode)`（MathIO 符号、LineIO 小数），
  前缀用 `main.rs::solution_prefix`（复数取两分量都"能精确呈现"才给 `=`）；
  非多项式根的回填 `solve_aux::float_to_exact_rational` 返回 `(p, q)` 而不是字符串——**不要退回固定 `x = 1/2` 式的输出**
  （旧实现既忽略显示模式，又给出与 MathIO 风格不一致的 `1/2`）。
- `solve_aux::result_prefix` 对 `Number::Approx` **一律**给 `≈`，哪怕数值恰好是 0。
  所以"数学上是精确 0"的结果必须返回 `Number::from_int(0)`，否则会显示成 `≈ 0`
  （`arcsinh(0)`/`arccosh(1)`/`arctanh(0)` 三者都靠显式零短路才给 `= 0`；新增这类函数时别忘了）。

### 复数（complex.rs + Number::Complex）

- `Number` 有**三个变体**：`Exact` / `Approx` / `Complex(Box<ComplexNum>)`（`ComplexNum { re, im }` 的实部虚部又是 `Number`，
  因此"精确高斯有理数"与"带根号/数值的复数"统一表示）。`Number::from_complex` 在虚部为 0 时**自动退化为实数**，
  保证显示与判定一致；反向用 `to_complex()` 提升。
- **`add`/`sub`/`mul`/`div` 必须在入口先判 `is_complex()`**：它们的 match 带 `_` 兜底臂，
  漏判会静默落到 `to_approx()` 数值路径**丢掉虚部**（编译器不会报错，这是本模块最大的坑）。
- `to_approx()` 对复数只取实部并 `debug_assert!(false)`——**parser 必须先分流**（`eval_function` 开头的
  `if args.iter().any(|a| a.is_complex())` 就是这道闸门）；`floor/ceil/round/frac` 同理在入口拦截。
- 但**分量操作 `re`/`im`/`conj`/`arg` 不受"实参含复数"限制**（`COMPONENT_FUNCTIONS` 常量）：实数就是虚部为 0 的复数，
  这四个函数对实数同样有定义。漏了这条会让 `re(2)`、`arg(-1)` 误报"未知函数: re"
  （它们在 `FUNCTIONS` 白名单里、却只有 `eval_complex_function` 有实现）。新增同类"对实数也成立"的函数时照此办理。
- 复数函数分派集中在 `Evaluator::eval_complex_function`：支持 `abs/sqr/sqrt/exp/ln/sin/cos/tan/re/im/conj/arg`，
  其余一律 `复数不支持该函数: xxx`。新增函数时要在这里明确表态（要么支持、要么明确报错，不要静默走实数路径）。
- 数值超越函数复用 `BigFloat`（**内部一律弧度制**，与 `/mode deg|rad` 无关）：`exp(a+bi)=e^a(cos b+i sin b)`、
  `ln(z)=ln|z|+i·arg(z)`（`arg` 用自写的 `atan2` 做象限修正）、`sin/cos` 用 `sin a cosh b` 那一套。
- 显示走 `display::format_complex`（`a + bi`、虚部 ±1 省略系数、分数/根式加括号）；
  `solve_aux::result_prefix` 对复数要求"两部分都能精确呈现"才给 `=`。
- `i` 是**常数**（`parse_identifier_or_function` 的常数臂），并且已从小写单字母变量表 `VALID_VARIABLES` 中移除——
  不要再把 `i` 当未知数（`i^2-4=0` 现在会提示"方程中没有变量"）。`2i` 走隐式乘法、`i2` 报未知标识符。
- `sqrt`/`ln` 收到**负实数**时改走复数域（`sqrt(-4)=2i`、`ln(-1)=iπ`），这是相对旧版本的行为变更；
  奇次根（`cbrt`）仍是实数路径。
- 状态文件与 `/save` 脚本里的复数统一用 `E:<MathIO 形式>`（`E:3 + 4i`）——`i` 是常数，重新解析即可精确还原。

### 工作精度与显示格式（运行期可配）

- `bigfloat.rs` 的 `PRECISION` / `DISPLAY_DIGITS` **已改为运行期值**：
  用 `bigfloat::precision()` / `display_digits()` 读取，`set_precision` / `set_display_digits` 修改
  （默认值常量是 `DEFAULT_PRECISION = 80` / `DEFAULT_DISPLAY_DIGITS = 20`）。
  **新代码不要引用已删除的 `PRECISION` 常量**，一律走访问器。
- 修改任何与精度相关的硬编码阈值时都要按 `precision()` 相对化；已梳理过的位置（改精度后必须回归）：
  `solver_poly` 的有限差分步长 `h`、`zero_compare`（precision/2+10）、残差校验与 DK 的 `tol`、
  `rough_prec`/`rough_tol`、复根虚部阈值（3·precision/8）；
  `solver_nonlinear` 的两阶段精度与容差、`same_solution` 容差；
  `parser` 的 `log` 底数判 1 阈值、`tanh` 饱和阈值、`check_trig_range` 的 10^(precision-2) 与报错文案；
  `solve_aux::finite_decimal_fits` 用 `display_digits()`。
- **保持常量的**：`MARGIN`（算法余量）、`MAX_RESULT_DIGITS`/`EXP_ARG_LIMIT_LOG10`（规模护栏，与用户精度无关）。
- 常数缓存（`pi_cache`/`e_cache`/`ln2_cache` 键为 `usize`、`int_ln_cache` 键为 `(BigInt, usize)`）天然按精度分桶，
  切精度**不需要失效处理**；但 `main()` 必须在任何运算之前 `set_precision(...)`（越早设定越省重复计算）。
- 显示开关：`sci_allowed()`（`/mode sci`，关闭时 `format_significant` 不走科学计数法分支）、
  `group_enabled()`（`/mode group`，只在 `to_significant_string` 与 `format_mathio` 的出口经 `group_integer_part` 加千分位；
  该函数只处理纯十进制串，分数/根式/科学计数法原样返回）。
- `/mode` 的取值子命令（`prec|digits|sci|group`）由 `handle_mode(parts, state)` 处理（签名从 `&str` 改成 `&[&str]`）；
  四项设置与 `timing` 一样立即 `persist_state()`，状态文件键为 `prec=`/`digits=`/`sci=`/`group=`。
  `mode_info()` 只在非默认时追加精度/显示信息（默认横幅保持不变）。

### 界面语言（i18n.rs）

- **main.rs 里所有控制台输出一律用 `lprint!` / `leprint!` / `lprint_inline!`**（内部走 `i18n::t`）；
  直接写 `println!` 的新文案不会随语言切换，是本功能最容易踩的坑。
- 其它模块（parser / number / solver_* / state）**保持中文字面量不变**——它们就是词条表的键：
  新增用户可见文案时，必须把**简体原文逐字**加进 `i18n::TABLE`（含 `{0}`/`{1}` 占位符编号），
  否则该条在英/繁模式下保持中文（不报错，但会漏译）。占位符按 `format!` 里 `{}` 的出现顺序编号。
- 续行字符串（`"…\` + 换行缩进）会把缩进带进运行时字符串，导致词条匹配不上：写提示文案时**用单行**。
- 匹配语义：整段与行内片段都支持；单字词条只允许整段精确匹配（防误伤）；"最优匹配"=字面部分总长最长者优先，
  所以"数值求根次数过高（{0} 次…）"这类整句词条会盖住更短的前缀词条。
- 类别/颜色等"英文键名(中文标注)"的显示统一走 `label_with_hint`（/mode 用法）与 `option_name`（/set 列表），
  不要在别处再拼 `format!("{}({})", ...)`。
- `/lang` `/language`：无参数时输出三行语言列表并进入选择（Windows 控制台用 `rawkey` 原始按键读 ↑/↓/回车/Esc；
  非终端/非 Windows 退回"输入序号或代码"，空行=确认）；带参数（`/lang en`、`/lang 2`）直接切换。
  Esc 取消时**不要**写状态文件；切换后立即 `persist_state()`（`lang=` 行），且只影响后续输出。
- 语言探测只在状态文件缺少 `lang=` 行（首次启动/旧文件）时进行：Windows 用系统界面语言，
  中文→简/繁，其余→英文。**不要改成优先看 `LANG` 环境变量**（Git Bash 里常是 en_US.UTF-8，会误判成英文）。
- 状态文件里的注释行、`debug_assert!` 文案、测试断言文案**不要**加进词条表（它们是文件内容/内部信息）。
- 插值本身带 ANSI 颜色（如启动横幅里的彩色 `/help`）时，`t()` 的"按纯文本段匹配"会把整行切碎而漏译；
  这类消息要在**构造期**用 `i18n::fmt("{0} … {1}", &[&a, &b])` 翻译（模板需与词条表逐字一致）。
- `/lang` 的原始按键菜单必须让 `LANG_MENU_LINES`（上移行数）**等于实际打印行数**（表头 + 3 语言 + 提示 = 5），
  每行都以换行结束；行数对不上时每按一次方向键就会多滚出一份菜单。箭头颜色取 `colors.operator`（运算符色）。
- `mode_info()` 末尾附加当前语言（`i18n::get().native_name()`），启动横幅与 `/mode` 共用。

### 多项式拟合（solver_fit.rs）

- **识别在 `parser::looks_like_polynomial_fit`，必须在 `parse_system` 之前**（`parse_and_eval` 里插在 `parse_sd` 之前）；
  认领规则是"行首 `(` 或 `P(`/`p(` + 第一个括号组内 depth==1 层有逗号 + `)` 之后只允许 行尾/`(`/字母"，
  这三重约束保证 `(1+2)*3`、`f(1,2)`、`P(2)`、`(1,2)+3`、`P+1`、`2P` 都**不认领**（行为与旧版一致）。
  **认领后失败即报错、不回落**（回退只会给出误导性的"缺少右括号"）；**不要改 `parse_atom` 的括号分组**来容纳逗号。
- 顶点**只认大写 `P`**（小写 `p(x,y)` 给专门提示）；`P` **不是保留标识符**，变量仍可叫 `P`。
- 约束计数：`constraints = 普通点数 + 2×顶点数`；无模板时次数 = constraints − 1，顶点行是**解析导数** `[0,1,2a,3a²,…]`；
  用 `solver_linear::gaussian_elimination` 解（先判 `unique`；`infinite` ⇒ 重复/退化；`None` ⇒ 矛盾）。
- 带模板时用"单位向量代入法"线性化：`φ_j = extract_polynomial(只有 p_j=1) − g`（**必须减 g**，
  否则含常数偏移的模板如 `A*x + b` 会算错）；矩阵解用自写 `rref`（要拿"主元变量 = 自由参数的表达式"，
  `gaussian_elimination` 只给唯一解）。解与系数都是 `LinForm`（自由参数的线性形式），
  解析式按次数相加 ⇒ 自动展开合并同类项；欠定时输出关系行 + `自由参数:` 行。
- 两道**必要**校验：`check_param_linearity`（样本参数值比较"直接代入模板"与"线性组合"）与
  `verify_solution`（把解代回模板逐个坐标复验）——单位向量法只对"参数线性"的模板成立，
  它们负责拦住 `a*b*x + c`、`a^2*x + b` 这类模板。
- 输出**恒为 `y = …`**（模板左值只是装饰，一律忽略）；二次函数在"系数全为常数、实际二次、首项非 0、m≠0"时
  追加顶点式 `y = a*(x - m)^2 + k`；`m = 0` 时顶点式与一般式逐字相同 ⇒ 不重复输出。
- 系数是 `Number`（可为 `pi`/无理/自由参数形式），**不要**改用 `solver_factor::format_poly`（只吃 `BigRational`）。
- 护栏：`FIT_MAX_SIZE_FAST = 16` 且用 `calc_mode::is_deep()` 放行 Deep；新文案都进 `i18n::TABLE`。
- 顺带修掉的既有 bug：`number::check_pow_scale` 对**零底数**（`0^2`）会估出 `inf` 位数而误报错——
  现已提前 `if self.is_zero() { return Ok(()) }`，不要删。

### 三角形求解（solver_triangle.rs）

- **输入形式是 `triangle(a=3 b=4 c=5)` 函数调用，识别在 `parser::parse_triangle_call`，必须排在 `parse_system` 之前**
  （插在拟合守卫之前/之后都可以，两者首字符不同、不冲突）。**旧版的裸写 `a=3 b=4 c=5` 已取消**，
  原因：那种写法与"方程组 / 隐式乘法"边界太近（`a=3 b=4 c=5` 本会被读成 `a = 3*b`），很容易误解，
  改用显式函数调用后语义一目了然（`a=3 b=4 c=5` 现在回到"位置 5 处多余的字符"）。
- **与 `sd`/`fac` 同一约定：只能是整个表达式的最外层函数**。判定分两层，缺一不可：
  ① `triangle` 标识符必须出现在**输入开头**（`contains_triangle_ident` 做词边界匹配，别处出现即报"必须是整个表达式的最外层函数"）；
  ② 配对右括号之后**不能再有内容**（否则同样报最外层错误）。认领后失败**不回落**——回落只会给出误导性提示。
  注意 `EvalResult` 没有实现 `Debug`，测试里取错误串要用自写的 `perr()` 而不是 `unwrap_err()`。
- **括号内的分隔符是"顶层逗号**或**空白"**（`split_triangle_items` 做深度感知，`log(2,8)` 里的逗号不会被切开）。
  旧的 `looks_like_triangle` 因为要避开 `,`（方程组）才禁止 RHS 含逗号，改成函数形式后这个限制消失了。
- **内部角度一律弧度**：输入用 `angle_to_radians` 按 `Evaluator::angle_mode` 换算（`deg` 时乘 `π/180`，
  保精确），输出用 `parser::radians_to_degrees` 精确转回（`Pi(coeff) → Rational(coeff*180)` ⇒ `π/2` 显示 `90`）。
  **反三角必须先走 `trig::try_exact_*` 再转换**，顺序反了 `arccos(0)` 就退化成 `≈ 90`。
  `rad` 模式下 `A=30` 是 30 弧度 > π ⇒ 会报"角度必须在 (0,180°) 范围内"，这是预期行为。
- **面积/角度公式约定**：`a = 2R·sinA`（**不要写成 `2R·sinA/2`**，SSA/AAS 两条路径都踩过）；
  `S = a·b·c/(2·2R)`；`R = abc/(4S)`、`r = S/(p/2)`、`hA = 2S/a` 全走 `Number` 精确运算。
- **SSS 必须先验三角不等式再调海伦公式**——矩形不成立时乘积为负，`BigFloat::sqrt` 会**直接 panic**
  （不是返回 Err）。`solve_sas` 里的 `sq` 同理要先判非负。
- **SSA 双解**：`h = b·sinA`；`a < h` 无解、`a = h` 一解（直角）、`h < a < b` **两解**、`a ≥ b` 一解。
  比较一律走 `Number` 的 `sub().is_negative()/is_zero()`，**`Number` 没有 `PartialEq`**。
- **高度归约**只做"能唯一确定"的方向：`hA·a = 2S`（高×底边→面积）、`hA = b·sinC = c·sinB`（高×邻角→邻边）、
  `a·hA = b·hB = 2S`（两条高+一条边→另一条边）。`sinC = hA/b` 这类**反解是两义的，不要放进归约**
  （会给出错误的对角），交给数值兜底。「三个高」只能定形状，会补一行最简整数比 `a : b : c`。
- **数值兜底**：未知量 `(A, B, s=2R)`（`C = π−A−B`、`a = s·sinA`、`hA = s·sinB·sinC`），
  残差按**已知值本身归一化**（角度按 π），多起点 Gauss-Newton 解法方程 `JᵀJ Δ = −Jᵀ r`（3×3 自写列主元消元），
  收敛后**去重 + 回代验证**。法方程奇异 ⇒ 报"信息不足"而不是"未收敛"（两者提示语义不同）。
- **已知量是"真值"**：`verify_givens` 逐项回代比对（超定矛盾在这里拦住），通过后 `to_solution` 把
  已知的边/角/高原样写回 ⇒ `A=30` 显示 `30` 而非 `29.9999…`。不要省掉写回，否则用户输入精确值时输出会变丑。
- 输出固定 5 行，`=`/`≈` 由 `solve_aux::result_prefix` 逐值判定；标签 `面积/周长/外接圆半径/内切圆半径`
  是**词条**（`t()` 支持 2 字以上的行内匹配），符号标签 `a…r` 不翻译。多解时由 `handle_triangle` 加 `解 N:` 分段。
- 顺带修掉的既有 bug：计时行曾写成 `println!("{}", t.dimmed())`，绕过 `i18n` ⇒ 英/繁模式下留中文；
  现已改回 `lprint!`。**任何用户可见输出都必须走 `lprint!`/`leprint!`/`lprint_inline!`**。

### 计算模式（calc_mode.rs）

- `CalcMode::{Fast, Deep}` 存在 `AtomicBool` 里（低层数值函数拿不到 `Evaluator`，且 REPL 单线程执行运算）。
- **新增任何规模护栏时都要用 `crate::calc_mode::is_deep()` 判断是否放行**，否则 Deep 模式会"半死算"；
  放行前别忘了 Deep 下可能出现的极端规模（例如 `int_divisors` 需要 BigInt 兜底分支）。
- 反向要求：**牛顿迭代的发散保护（`NEWTON_ABS_LIMIT`）不属于规模护栏，两种模式都必须保留**，
  否则 `2^x=8` / `e^x=2` 会立刻回到卡死状态。
- 显示差异集中在 `bigfloat::to_significant_string`（Deep 完整精度、不走科学计数法）与
  `solve_aux::finite_decimal_fits`（Deep 下有限小数一律算"能完整显示" ⇒ 前缀 `=`）。
- 模式只影响行为，不影响 `Evaluator` 里的显示/角度模式；`/mode` 无参数时由 `mode_info()` 拼成 `LineIO/Radian/Fast`。

### REPL（main.rs）

- prompt 必须是纯文本 `"> "`；prompt 里放 ANSI 颜色会让光标位置偏移。
- 运算计时（`Timing`）：**计时范围是"解析 + 结果生成"整段**（结果格式化阶段——通式识别、根收集、因式分解——常比 parse 本身更耗时，只包 parse_and_eval 会导致动态计时从未出现）；handler 只生成结果字符串，统一在单次打印前 `stop_dynamic()`。动态行用 `\r` 刷新、无换行，所以**清行必须在结果打印之前**，且要先 `join` 刷新线程（否则线程可能在结果之后又写一次，画面错位）；停止线程靠关闭 `mpsc` 通道（`recv_timeout` 立即返回），不要用 `sleep` 轮询等待。`is_terminal()` 为假的管道场景要跳过全部 `\r` 输出。
- 结果文本的生成统一在 `run_line(input, state) -> (String, bool)`（REPL 与非交互入口共用）；
  **打印结果行必须走 `lprint!`**——写成 `println!` 会绕过翻译，英文模式下标签/提示不翻译（批次 1 踩过这个坑）。
  计时与输出由调用方负责：REPL 用 `Timing`，非交互模式只打印裸结果。
- `/timing on|off` 开关存在 `calc_mode::timing_enabled()`（AtomicBool，默认开），持久化为状态文件 `timing=` 行；
  **落盘值取 `AppState::timing_base`（经 `timing_for_persist`），不要直接写运行期开关**——否则 `-q` 会污染配置；
  `/timing on|off` 与 `/reset all` 都要同时更新 `timing_base`。
  `Timing::begin()` 在关闭时不起刷新线程、`elapsed_text()` 返回空串（调用方要先判空再打印，否则会多出空行）。
- Tab 补全与内联提示的逻辑抽成纯函数 `completion_candidates(token, vars)` 与 `inline_hint(line)`（都有单元测试），
  `Completer`/`Hinter` 只做转发；`CalcHelper` 除 `colors` 外还带 `vars`（存储变量名），
  **增删变量后要连颜色一起刷新**（主循环里两处同步点已改）。候选集只含**多字母**函数/常数/变量名与指令，
  单字母 token 一律不给候选——否则会把 `xy` 这类隐式乘法补成函数名。
- 指令清单 `COMMANDS`（补全用）与 `COMMAND_HINTS`（内联提示用）、`SIGNATURES`（函数参数提示用）都在 main.rs 顶部；
  新增指令/两参函数时记得同步这几张表（有 `commands_table_is_consistent` 测试兜底）。
- `calc_mode::quiet()`：输出宏 `lprint!`/`leprint!`/`lprint_inline!` 在静默状态下跳过打印（`/load` 回放脚本时开启）。
  开启后**必须**恢复，否则后续输出全被吞掉。
- `/save` 用 `state::encode_var_for_script`（精确值走 MathIO 符号、近似值走完整十进制串）——它与状态文件内部的
  `encode_var`（`E:`/`A:` 前缀）**不是一回事**，不要混用（后者喂给求值器会解析失败）。
- `/reset all` **不重置语言**（避免误删用户偏好），但会重置模式、颜色、精度、计时开关。
- `/set` 改色后要立即 `rl.helper_mut().unwrap().colors = state.colors.clone()`，否则高亮不更新；
  `/set` 成功后立即 `persist_state()`（颜色写入状态文件的 `color:<类别>=<颜色名>` 行）。
  类别与颜色名的唯一来源是 `COLOR_CATEGORIES` / `COLOR_OPTIONS` 两张表（英文名 + 中文标注 + 颜色值），
  `/set` 解析、当前颜色列表与状态文件读写都走 `parse_color` / `color_name` / `ColorConfig::{set_category,get_category,to_pairs}`，
  **不要在别处再写一份 match 颜色名**。
- 文本着色只有两个入口，共用底层 `colorize_impl(line, colors, command_anywhere, base)`：
  - `colorize_text(line, colors, command_anywhere)` → `base=None`，未识别文本**原样**输出。
    输入行传 `false`（只有行首 `/xxx` 是指令，行内 `/` 是除法）、`/help` 与用法提示传 `true`。
  - `colorize_result(line, colors)` → `base=Some(colors.result)`，未识别文本用 result 色打底。
    **所有计算结果都必须走它**（`format_result_line`、`handle_factor`、`handle_equation`、`handle_system`、
    `handle_fit`、`handle_triangle`、`format_solutions`、`/let` 与 `/var` 的值显示）。
  两点不能忘：
  ① **不要**回到"整行刷单一 `result` 颜色"（`line.color(colors.result).bold()`）——那样结果看起来和没高亮一样，
     用户明确要求结果与输入用同一套高亮；
  ② 底色**不要加粗**：`result` 默认 `BrightWhite`，加了 bold 会把同行的数字（`number` 默认 `White`，且
     `colorize_impl` 里的数字分支不 bold）衬得过暗，纯白加粗标签 / 灰白数字的对比很刺眼。
  多行/多段结果要**整体着色**后再拼接（如 `format_solutions` 是 `colorize_result(&parts.join(", "))`），
  否则分隔用的 `, ` 会掉出底色。
  `/help` 正文是 `HELP_TEXT` 常量（无边框简版），`【…】` 小节标题用 result 颜色加粗；仅输入 `/set` 时
  `print_set_usage` 先给用法（类别、颜色均带中文标注）再列当前颜色（颜色名用各自的颜色显示）。
  `OPS` 含 `!`/`=` 以及**只在结果里出现**的 `≈`/`×`/`·`，`CONSTANTS` 含 `π`；`/mode`、`/let`、`/del` 的用法提示用 `colorize_text` 生成，
  「用法/提示」标签用 `prompt` 颜色，未知指令/模式/颜色用 `error` 颜色。
  验证颜色**必须**加 `CLICOLOR_FORCE=1`（`colored` 在非 TTY 下自动关闭颜色，管道里看不到 ANSI，
  这也意味着纯文本输出与着色改造前逐字节一致）。
- `sd(x)`（`EvalResult::SdValue`）的前缀**按互换后的显示模式**判定：MathIO→小数用 `result_prefix(_, LineIO)`、
  LineIO→符号用 `result_prefix(_, MathIO)`；不能无条件写 `≈`（旧实现把 `sd(1/2)` 的精确结果标成 `≈ 0.5`）。
- 变量存储：`/let`（名须全大写，含 `_`）、`/var`（列出）、`/del A`（删单个）、`/del all`（清空，**严格匹配小写 `all`**），存于 `Evaluator.vars`；
  未知数提取统一走 `unknown_variables()`（`handle_equation` 与 `handle_system` 共用），
  且 `extract_linear_rec` **必须**把存储变量与 `ans` 代入常数项——否则 `x+A=5, y=1`（A=4）会被解成 `x = 5`。
- `handle_let` 解析参数用"剥离 `/` + 命令词（`take_while(is_ascii_alphabetic)`）"的方式，
  **不要**用"跳到第一个大写字母"的写法——那会把 `/let 1A=5` 静默当成 `A=5` 存下来。
- 会话状态持久化（`state.rs`）：`~/.hipercalc_state` 存显示/角度/计算模式、全部变量与 `/set` 颜色，
  启动 `state::load()` 恢复、每次变更后 `persist_state()` 落盘、退出时再存一次。
  编码约定：精确值存 `E:<MathIO 表达式>`（重新解析还原，无损），近似值存 `A:<value>:<precision>`
  （BigFloat 内部十进制，无损），颜色存 `color:<类别>=<颜色名>`（`state::load` 只返回原始
  `(类别, 颜色名)` 字符串，**合法性由 main.rs 用 `parse_color`/`set_category` 校验后应用**，非法项跳过）。
  **删除状态文件即恢复默认**——不要把默认值也写进文件逻辑里造成"删不掉"。
- 历史持久化：`history_path()`（`%USERPROFILE%/.hipercalc_history`），启动 `load_history`、退出 `save_history`，每行 `add_history_entry`。
- 新增 `src/*.rs` 模块要在 `main.rs` 顶部加 `mod` 声明。
- 改动任何功能/输出后同步更新 `README.md`（第一部分功能手册 + 第二部分代码架构）。
