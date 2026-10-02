# HiPerCalc — Ultra-Precision CLI Calculator (Chinese)

[简体中文](README.md) · **English**

HiPerCalc is an interactive command-line calculator written in Rust, with **arbitrary-precision decimals**, **exact symbolic arithmetic**, **equation / system solving**, **polynomial factorization**, and more. It uses 80 decimal digits internally, displays 20 significant digits, and ships with a syntax-highlighted REPL with live highlighting.

---

# Part 1: Feature Guide (User Manual)

## 1. Getting Started & Basic Usage

```powershell
cargo build
cargo run
```

After starting you get a `> ` prompt. Type an expression and press Enter to evaluate; `Ctrl+C` interrupts, `/exit` quits.

```
> 1+2*3
= 7
用时：<1秒
```

After the first run, input history is saved to `~/.hipercalc_history`; press `↑` to recall it next time.

### Build Configuration Note (Debug Speed)

`Cargo.toml` enables optimization for **third-party dependencies** only:

```toml
[profile.dev.package."*"]
opt-level = 3
```

The project's own source stays at `opt-level = 0` (so breakpoints / variable inspection still work), but `num-bigint` /
`num-rational` etc. are over an order of magnitude slower at opt-level 0 in multiplication/division/modulo — they carry
almost all the heavy lifting. With this flag, equation solving in debug builds is typically **9–13× faster**
(e.g. `e^x=2` from ~27s to 3s, `2^x+3^x=10` from ~50s to 4s); the first build recompiles dependencies (a bit slower), afterwards it is incremental.

### Command-Line Entry (Non-Interactive Mode)

With no arguments it starts the interactive REPL. For scripting, pass one of:

```text
hipercalc -e "1+2*3"           # evaluate a single expression, print the bare result line (no timing line)
hipercalc -f script.hc         # run a script line by line (# comments, blank lines skipped)
hipercalc --stdin < input.txt  # stream-execute from stdin
hipercalc --lang en -e "1/0"   # force UI language (overrides the state file for this run, does not persist)
hipercalc -q -e "1+1"          # suppress the timing line (this run only, does not persist)
```

- Exit codes: `0` all succeeded, `1` an evaluation error occurred, `2` bad arguments or file read failure;
- `-f` / `--stdin` keep going after errors (the error lines still print); if any error occurred the final exit code is `1`;
- Non-interactive mode never prints the timing line (`-q` is mainly for interactive/piped mode);
- `-q` / `--no-timing` and `--lang` are **session-only overrides**, they do not write to the user config —
  to disable timing permanently use `/timing off` in interactive mode;
- **Piping/redirecting does not switch to banner-less mode**: with no arguments you still get the banner + `> ` prompt + timing line, so existing verification habits keep working.

### Timing Display

Every computation is timed:

- **While running**: a `用时：N秒` line refreshes dynamically (whole seconds, only on a real terminal; skipped when piped/redirected);
- **When done**: the result is printed on that line (covering the dynamic timer), then a final `用时：N秒` is printed below (sub-second → `用时：<1秒`).

```
> 2^x=8
用时：0秒            # dynamic refresh: 1s → 2s → … → 7s
x = 3
用时：8秒
```

Disable: `/timing off` in interactive mode (persists); `-q`/`--no-timing` on the command line only for that run.
> Upgrade note: an early version had a bug — running with `-q` would write `timing=off` into the user config, so all later startups lost the timing line. If you hit that, `/timing on` restores it (that write path is fixed).

## 2. Basic Arithmetic

| Symbol | Meaning | Example |
|---|---|---|
| `+ - * /` | add, subtract, multiply, divide | `7/2` → `= 3.5` |
| `( )` | parentheses (grouping) | `(1+2)*3` → `= 9` |
| `^` | power (right-associative, higher than `*`/`/`) | `2^3^2` = `2^(3^2)` = `512` |
| `ans` | previous result (initially 0) | `ans+1` |

Powers support a wide range:
- Negative exponents: `2^-2` → `= 0.25` (integer exponents return an exact rational);
- Fractional exponents: `8^(1/3)` → `≈ 2` (non-integer exponents go through high-precision numeric path, prefixed `≈`);
- Negative base: rational exponents with odd denominator evaluate in the reals, `(-8)^(1/3)` → `≈ -2`; even denominator or irrational exponents are undefined over the reals and raise an error.

**Scale guard**: powers estimate the number of decimal digits of the result; if it exceeds ~`10^6` digits (or the exponent is too large for fast integer power) it errors out instead of returning a truncated wrong value:

```
> 2^100000
= 9990020...（约 3 万位整数，整串输出）
> 2^10000000
错误: 幂运算结果约有 3750088 位十进制，超过支持上限（约 10^6 位）
> 2^10000000000
错误: 幂运算结果超出支持范围（指数 × ln(底数) ≈ 10^10，结果约 10^10 位十进制）
```

## 3. Implicit Multiplication & Scientific Notation

- **Implicit multiplication**: a number, variable, or parenthesis adjacent to another is multiplied automatically.
  `3x`, `2(3+4)`, `(1+2)(3+4)`, `2|x|` are all valid; the constant `e` can also be a factor:
  `2ex` → `2*e*x`, `xe` → `x*e`, `ee` → `e^2`.
- **Scientific notation (input)**: a number followed by `e`/`E` and an integer exponent.
  `1e3` → `= 1000`, `2.5e-2` → `= 0.025`, `1e0` → `= 1`.

## 4. Constants & Precision

| Constant | Value |
|---|---|
| `pi` / `π` | π ≈ 3.14159… |
| `e` | Euler's number ≈ 2.71828… |

- Constants: `pi`/`π`, `e`, `i` (imaginary unit), `tau` (= 2π, exact, shown as `2*pi` in MathIO), `phi`/`φ` (golden ratio `(1+√5)/2`, exact);
- Internally everything runs at the **working precision (80 digits by default)**, adjustable with `/mode prec N` (20 ~ 2000);
- Display rounds to **20 significant digits** (finite decimals print in full);
- **Big-number display**: when the integer part exceeds 20 digits, switch to scientific notation (keeping the magnitude):

```
> 10^25/3
≈ 3.3333333333333333333×10^24
> exp(60)
≈ 1.1420073898156842837×10^26
> 123456789012345678901234567890/7
= 17636684144620811271604938270      （整数原样整串输出）
```

- **Argument limits**: `exp`/`sinh`/`cosh` error when `|x| > 10^6` (result digits explode);
  `tanh` is the exception (for `|x| > 100` it's already saturated at 80-digit precision, returns ±1 without error);
  trigonometric functions error when the radian argument `|x| ≥ 10^78` (the π argument reduction becomes unreliable at 80 digits).

### Calculation Modes `/mode fast` / `/mode deep`

| Mode | Behavior |
|---|---|
| **Fast** (default) | Keeps scale guards: `exp`/power/trig/factorial/scientific-notation exponent out of range → immediate message; factorization candidate enumeration has a budget, over budget → "result may be incomplete"; shows 20 significant digits (scientific notation when integer part > 20 digits) |
| **Deep** | **Brute force**: removes all the above limits, **full-precision output** (no truncation, no scientific notation). Extreme input may hang, produce huge output, or even exhaust memory — judge for yourself |

```
> 10^25/3
≈ 3.3333333333333333333×10^24        # Fast
> /mode deep
已切换到死算模式 (Deep)：不设规模上限、完整精度输出（不走科学计数法）；极端输入可能长时间无响应或产生超长输出
> 10^25/3
≈ 3333333333333333333333333.33333333333333333333333333333333333333333333333333333333333333333333333333333333
> 1/2^70
= 0.0000000000000000000008470329472543003390683225006796419620513916015625
> 2^10000000                             # Fast rejects; Deep really computes (~3 million digits)
```

Notes:

- Deep only lifts the **scale** limits, it does **not** lift the Newton divergence guard — otherwise `2^x=8` would hang again;
- In Deep, `2^10000000000`, `1e10000000000` and the like will genuinely try to build astronomically many digits and almost certainly exhaust memory — avoid;
- `sqrt` simplification and factorization candidate enumeration may run very long in Deep (e.g. square-factor trial division of a large Δ);
- All three modes (display / angle / calc) persist in `~/.hipercalc_state` and restore on restart (see §10).

## 5. Function Reference

### Function Forms (Whitelist)

| Category | Function | Description |
|---|---|---|
| Roots | `sqr(x)`、`sqrt(x)` | square root (negative → pure imaginary root, see "Complex Support") |
| Complex | `re(z)` `im(z)` `conj(z)` `arg(z)` | real / imaginary / conjugate / argument (radians); **also work on reals** (`re(2) = 2`, `im(2) = 0`, `arg(-1) = π`) |
| | `cbrt(x)` | cube root; negatives allowed (odd root); perfect cubes are exact (`cbrt(-27)` → `= -3`, `cbrt(1/8)` → `= 0.5`) |
| Absolute | `abs(x)`、`|x|` | both forms are equivalent |
| Trig | `sin cos tan cot sec csc` | affected by angle mode (radians/degrees) |
| Inverse trig | `arcsin arccos arctan arccot arcsec arccsc` | special values return exact angles |
| Log | `ln(x)` | natural logarithm, `x>0` |
| | `log(b, x)` | arbitrary-base logarithm: `log(2, 8)` → `= 3` |
| | `log10(x)`、`log2(x)` | common / base-2 logarithm |
| Exponential | `exp(x)` | `e^x` |
| Rounding | `floor(x) ceil(x) round(x)` | floor / ceiling / round |
| Fraction | `frac(x)` | `x - floor(x)`, in `[0,1)` |
| Sign | `sign(x)` | `-1 / 0 / 1` |
| Hyperbolic | `sinh cosh tanh` | hyperbolic sine/cosine/tangent |
| | `coth sech csch` | hyperbolic cotangent/secant/cosecant (`coth(0)`, `csch(0)` are undefined and error) |
| Inverse hyperbolic | `arcsinh(x)` | inverse hyperbolic sine (defined on all reals) |
| | `arccosh(x)` | inverse hyperbolic cosine, domain `x >= 1` |
| | `arctanh(x)` | inverse hyperbolic tangent, domain `|x| < 1` |

Adding a function only touches **two places**: the `FUNCTIONS` constant in `parser.rs` (parsing validation, multi-letter splitting, and REPL highlighting all share it) and `Evaluator::eval_function`. `fact`/`abs` are internal names — don't add them to the whitelist.

### Postfix Factorial `!`

Binds tightly to its operand, **higher precedence than power**:

```
> 5!
= 120
> (2+3)!
= 120
> 2^3!        # = 2^(3!) = 2^6
= 64
```

The argument must be a non-negative integer, upper limit 10000.

### Absolute Value `| |`

Same precedence as parentheses; nesting and combinations all work: `|-5|`, `|x-3|`, `2|x|`, `|2*3-1|`, `||-3||`.

### Exact Logarithm Recognition

- `log2`/`log10` return an exact integer when the argument is a power of 2/10: `log2(8)` → `= 3`;
- `log(b, x)` returns an exact rational when base and argument are both integer powers of the same number:

```
> log(3, 27)
= 3
> log(8, 1/2)
≈ -0.33333333333333333333   （LineIO：-1/3 is a repeating decimal；MathIO shows = -1 / 3）
> log(12, 144)
= 2
> log(2, 2^100)
= 100
> log(e, e)
= 1
```

Everything else uses `ln(x)/ln(b)` high-precision approximation, e.g. `log(2, 3)` → `≈ 1.5849625007211561815`.

## 6. Display Modes & `=` / `≈` Semantics

| Mode | Behavior |
|---|---|
| **MathIO** (math display) | Prefers **exact symbolic** form: integer, reduced fraction, radical, `pi`, `e`; falls back to decimal when not exactly representable |
| **LineIO** (linear display, default) | Always decimals (full digits if terminating, else 20 significant digits; scientific notation when integer part > 20 digits) |

The prefix before the result indicates **exactness**:

- `= ` means the output is exact (integer / fraction / radical in MathIO; integers and terminating decimals that fit in LineIO);
- `≈ ` means approximate (infinite decimals, irrationals, and — in LineIO — finite decimals whose significant digits exceed 20, e.g. `1/2^70` gets rounded to 20 digits and is therefore marked `≈`).

> Under `/mode deep` nothing is truncated, so a finite decimal (like `1/2^70`) is always marked `=`.

`sd(x)` is a forced display conversion: MathIO → decimal, LineIO → tries symbolic form. It must be the outermost function of the whole expression. The prefix is decided by the **converted** form: LineIO `sd(1/3)` → `= 1 / 3` (exact symbolic), `sd(sin(1))` → `≈ 0.84147…` (still only a decimal); MathIO `sd(1/2)` → `= 0.5` (a terminating decimal that fits), `sd(1/3)` → `≈ 0.33333…` (repeating).

## 7. Equation Solving

Input containing a single `=` is treated as an equation and solved:

| Input | Output |
|---|---|
| `x^2-4=0` | `x = 2, x = -2` |
| `x^2+1=0` | `x = 0 + 1i, x = 0 - 1i`（complex roots） |
| `x^3-6x^2+11x-6=0` | `x = 1, x = 2, x = 3`（all roots） |
| `1/x=2` | `x = 0.5` |
| `2^x=8` | `x = 3` |
| `e^x=2` | `x ≈ 0.69314718055994530942` |

> Roots are printed per the display mode with `=`/`≈` just like ordinary results: LineIO always decimals
> (`1/x=2` → `x = 0.5`, `x^2=2` → `x ≈ 1.4142135623730950488`),
> MathIO prints exact roots symbolically (`x = 1 / 2`, `x = sqrt(2)`).
>
> Exponential/log equations go through numeric Newton iteration and may take a few seconds in debug builds (each step needs high-precision `ln`/`exp`);
> if Newton diverges (correction or |x| exceeds 10^6) the guess is dropped immediately, so it never hangs.
> High-degree equations (degree > 200) error directly in Fast mode with `数值求根次数过高…请先执行 /mode deep`,
> **without** degrading to single-root Newton (which would leave the UI unresponsive and return only one root).

### Infinite Solutions: General Form

Trigonometric equations have infinitely many solutions; the program gives **all of them** as a periodic general form, whose unit follows the current angle mode (radians give `k·π`, degrees give degrees):

```
> /mode rad
> cos(x)=0
x = π/2 + k·π，k 为整数
> sin(x)=0.5
x = π/6 + k·2π  或  5π/6 + k·2π，k 为整数
> sin(x)=0
x = k·π，k 为整数

> /mode deg
> cos(x)=0
x = 90 + k·180，k 为整数
> sin(x)=0.5
x = 30 + k·360  或  150 + k·360，k 为整数
> sin(x)+cos(x)=1
x = k·360  或  90 + k·360，k 为整数
```

**Recognition principle** (`solve_aux::format_periodic_roots`): the collected real roots are first **converted to radians**,
then we check whether adjacent differences are "all equal" (single family) or "strictly alternating between two values" (two families) ⇒ derive the common period `d`,
then express `d` and each residue as `(p/q)·π` via **rational approximation with denominator ≤ 12**, and finally render in the current unit (radians `π/6` / degrees `30`). Three conventions:

- Only roots within the `±3π` window are considered before detection — Newton can land on far-away roots (e.g. `sin(x)+cos(x)=1` picks up ≈29.8), which would break the differences; the window **only affects detection**, not the full root list shown in the fallback branch;
- When the equation contains a **trig function of the unknown** (`sin(x)` and friends), initial guesses in degree mode are scaled by `×180/π`, making the Newton trajectory physically identical to radian mode; non-trig equations (`1/x=2`) keep their guesses;
- Equations that don't form a periodic structure (e.g. `sin(x)=x/10`) are **not** forced into a general form — roots are listed individually.

### Solving Strategy

- Polynomial equations: linear/quadratic **exact**; higher-degree polynomials use **Durand-Kerner iteration for all complex roots**;
- Equations with transcendental functions (cos, ln, log, …): **Newton iteration** with multiple near-to-far initial guesses (including π/2 integer multiples to cover trig zeros), with a final residual check to reject spurious roots;
  in degree mode, if the equation contains a **trig function of the unknown**, guesses are scaled by `×180/π` so the trajectory matches radian mode (otherwise `±k·π ≈ ±3.14°` is too small and `±1..±6` overshoots);
  guesses that fall into the complex domain (e.g. `ln(-1)`) are dropped;
  a correction or |x| above 10^6 is treated as divergence and the guess is dropped (common divergence shape for exp/log equations);
- If a result is close to a simple fraction/radical it is back-filled to the exact value (e.g. `abs(x)=1` → `x = 1, x = -1` rather than an approximation);
  the back-filled value is printed per the display mode (LineIO `1/x=2` → `x = 0.5`, MathIO → `x = 1 / 2`);
  tolerance scales with magnitude, so tiny roots are never back-filled to `0` (e.g. `arctan(x)=1e-30` → `x ≈ 0.000000000000000000000000000001`).

## 8. Systems of Equations

Comma-separated equations form a system:

```
> x+y=5, 2x-y=1
x = 2, y = 3
```

- **Linear systems**: Gaussian elimination; no solution / infinitely many solutions are reported; solutions print per the display mode with the `=`/`≈` prefix (LineIO `3x=1, y=1` → `x ≈ 0.33333333333333333333, y = 1`; MathIO → `x = 1 / 3, y = 1`);
- **Nonlinear systems**: multi-dimensional Newton; first a **coarse scan** over a grid of initial guesses (forward differences, loose tolerance) collects candidates, then **fine convergence** (central differences, high tolerance); multiple solutions are deduplicated;
  after convergence the residual is re-checked (to avoid spurious flat-region roots), and any component above 10^6 is treated as divergence and dropped.

## 9. Factorization fac / factor

- `fac` and `factor` are equivalent and must be the outermost function of the whole expression;
- The factorization domain is decided by the current display mode:
  - **MathIO → real domain**: quadratic factors may be split into `sqrt` radicals;
  - **LineIO → rational domain**: integer/fraction coefficients only; reports when irreducible;
- Supports **1~3 variables**, large coefficients, and homogeneous lifting;
- `ans` and `/let`-stored uppercase variables are substituted by their current value (consistent with equation solving):
  `/let A=4` then `fac(x^2-A)` → `= (x - 2) * (x + 2)`;
- **Scale limits**: rational-root candidates (abandoned when coefficients > 10^13) and quadratic-factor enumeration both have budgets,
  and the budget is counted by **work** (trial divisions × degree ≤ 4×10^4), so high-degree polynomials automatically try fewer divisions;
  over budget, the result gets a `（候选枚举超出规模上限，结果可能不完整）` note instead of hanging.

```
> fac(x^2-4)
= (x - 2) * (x + 2)
> fac(x^2-y^2)
= (x - y) * (x + y)
> fac(x^3-8)
= (x - 2) * (x^2 + 2x + 4) （有理数域内不可再分解）
> fac(x^2-2)
= (x^2 - 2) （有理数域内不可再分解）      # LineIO：irreducible over the rationals
> /mode mathio
> fac(x^2-2)
= (x - sqrt(2)) * (x + sqrt(2))          # MathIO：splits into radicals over the reals
> fac(100000000000000*x^2 + 100000000000001*x + 1)
= (100000000000000*x^2 + 100000000000001*x + 1) （候选枚举超出规模上限，结果可能不完整）
```

> In LineIO a retained quadratic factor (like `x^2-2`) is definitely irreducible over the rationals (rational-root enumeration already failed), so it always gets `（有理数域内不可再分解）`; in MathIO the same quadratic factor gets no note when it can be split into `sqrt` radicals.

## 10. Variable Storage

| Command | Action |
|---|---|
| `/let A = 5` | store a variable (**name must be all uppercase**, may contain underscores: `X`, `AB`, `PI_VAR`) |
| `/var` | list all stored variables |
| `/del A` | delete a specific variable |
| `/del all` | clear all variables (matches lowercase `all` exactly; a variable literally named `ALL` can still be deleted individually) |

Stored values auto-substitute in ordinary expressions and never clash with equation unknowns (unknowns use lowercase `x y z…`):

```
> /let X=2
X = 2
> X
= 2
> 10X
= 20
> log(X, 8)      # stored variables can be function arguments (base)
= 3
> /let Y = X * 3 # new variables can reference existing ones
Y = 6
```

In equations and `fac`, stored uppercase variables are treated as known constants: `/let B=2` then `B+x=5` → `x = 3`.

### Persistence (restores after restart)

Variables, the three modes (display / angle / calc) and `/set` colors are saved to `~/.hipercalc_state`:

```
> /let A=5
> /mode mathio
> /set functions red
> /exit                # quit
--- restart ---
已恢复上次会话设置，含 1 个存储变量（/var 查看，/del all 清空）
当前模式: MathIO/Radian/Fast/简体中文
> A+1
= 6
```

- Storage is lossless: exact values are saved symbolically (`1/3`, `(1/2)*sqrt(2)`, `pi`), approximate values save the full 80-digit internal representation;
- Variables, modes and colors are written to disk immediately after every change (`/let`, `/del`, `/mode`, `/set`), so an unexpected exit loses nothing;
- **Deleting `~/.hipercalc_state` restores defaults and clears variables/colors.**

## 11. Command Reference

| Command | Description |
|---|---|
| `/help` | show help (compact no-border version, body colored per keyword) |
| `/clear` | clear screen |
| `/mode mathio` / `/mode lineio` | switch display mode |
| `/mode deg` / `/mode rad` | degrees / radians |
| `/mode fast` / `/mode deep` | fast mode (scale guards) / brute-force mode (no limits, full precision) |
| `/mode prec N` / `/mode digits N` | working precision (20~2000) / displayed significant digits (1~precision) |
| `/mode sci on\|off` / `/mode group on\|off` | scientific notation / thousands separators |
| `/lang` / `/language` | switch UI language (简体中文 / 繁體中文 / English); no argument → selection menu |
| `/timing on` / `/timing off` | show/hide the "用时：N秒" line (default on, persists) |
| `/reset` / `/reset all` | clear variables & ans / also restore default modes & colors |
| `/save <path>` / `/load <path>` | save the session as a replayable script / replay a script |
| `/mode` | no argument → show usage (options annotated in Chinese) and current modes |
| `/set <category> <color>` | set highlight color; bare `/set` shows usage and current colors |
| `/let A = 5` | store a variable (uppercase name) |
| `/var` | list variables |
| `/del A` | delete a specific variable |
| `/del all` | clear all variables |
| `/exit` / `/quit` | quit |

### Polynomial Fit (type coordinates to get the function)

Type coordinates and it finds the polynomial through them: **2 points → linear, 3 → quadratic, and so on**. Append a "template" expression and it solves the unknown parameters; if parameters are under-determined it prints the parameter relationships.

```
> (1,2) (3,4)
y = x + 1

> (0,1) (1,3) (2,7)
y = x^2 + x + 1
y = (x + 1 / 2)^2 + 3 / 4          ← quadratics get a vertex form appended

> P(1,3) (0,0)                     ← P marks the vertex (two constraints: f(1)=3 and f'(1)=0)
y = -3*x^2 + 6*x
y = -3*(x - 1)^2 + 3

> (0,1) (1,3) (2,7) y = a*x^2+b*x+c
a = 1, b = 1, c = 1
y = x^2 + x + 1
y = (x + 1 / 2)^2 + 3 / 4

> (0,1) (1,3) y = a*x^2+b*x+c      ← under-determined: relationships, and the expression uses them
a = 2 - b
c = 1
自由参数: b
y = (2 - b)*x^2 + b*x + 1
```

| Rule | Description |
|---|---|
| Coordinates | `(x, y)`; components may be negative, decimals, fractions, `pi`, `sqrt(2)`, any expression |
| Vertex | **only uppercase `P`**: `P(x, y)` is the function vertex (two constraints). Lowercase `p` is still an ordinary variable; `p(x,y)` suggests using uppercase |
| Constraint count | 1 per ordinary point, 2 per vertex; count − 1 = degree |
| Template | follows the points; `y = …` / `f(x) = …` / no left-hand side (output is always `y = …`, the left side is decorative) |
| Unknown parameters | letters in the template whose value can't be determined (the independent variable is fixed as `x`); `pi`/`e`/`i` and `/let`-stored variables count as **known** |
| Template simplification | any spelling is expanded and like terms merged, e.g. `a*x^2+a*x+b*x+c` ⇒ `(a + b)*x^2 + …` |
| Under-determined | when points are insufficient, prints "pivot parameter = expression in free parameters" plus a `自由参数: …` line; the expression uses the relationships too |
| Vertex form | appended only when "actually quadratic (leading coeff ≠ 0) and all coefficients constant", like `y = a*(x - m)^2 + k` |
| Scale limit | Fast mode caps "constraints/parameters" at 16 (degree ≤ 15), above → `/mode deep`; Deep lifts it |
| Unsupported | the template must simplify to a **polynomial in x** (`sin/log` etc. error out); over-determined and contradictory → reports contradiction (no least squares) |

Example: `/let A=2` then `(0,1) (1,3) y = A*x + b` ⇒ `b = 1` and `y = 2*x + 1` (`A` known, only `b` solved).

### Triangle Solver (`triangle(...)` gives you everything)

Put the known sides / angles / heights inside `triangle(...)` and it computes the full triangle:

```
> triangle(a=3 b=4 c=5)
a = 3, b = 4, c = 5
A ≈ 36.869897645844021297, B ≈ 53.130102354155978703, C = 90
hA = 4, hB = 3, hC = 12 / 5
面积 = 6, 周长 = 12
外接圆半径 = 5 / 2, 内切圆半径 = 1

> triangle(A=30, b=5, C=60)        ← commas work too
a = 5 / 2, b = 5, c = 5*sqrt(3) / 2
A = 30, B = 90, C = 60
hA = 5*sqrt(3) / 2, …

> triangle(hA=4 a=3 b=4)           ← height + base → area, then reduces to SAS
a = 3, b = 4, c = 5
A ≈ 36.869897645844021297, B ≈ 53.130102354155978703, C = 90
…

> triangle(a=4 b=5 A=30)           ← SSA two solutions, listed separately
解 1:
a = 4, b = 5, c ≈ 7.4526260181213923367
…
解 2:
a = 4, b = 5, c ≈ 1.2076280197229941309
…

> triangle(hA=2 hB=3 hC=4)
错误: 已知信息不足，无法确定三角形（形状已确定 a : b : c = 6 : 4 : 3，仅缺一个长度）
```

| Rule | Description |
|---|---|
| Input | `triangle(a=3 b=4 c=5)`; inside the parentheses, `<notation>=<value>` items separated by **whitespace or commas** (a comma that belongs to a function call, e.g. `log(2,8)`, is not a separator) |
| **Outermost only** | Same convention as `sd(...)`/`fac(...)`: `triangle(...)` must be the outermost function of the whole expression. `2*triangle(...)` or `triangle(...)+1` errors with "must be the outermost function of the whole expression" |
| Notation | sides `a,b,c`; angles `A,B,C` (`A` opposite `a`); heights `hA,hB,hC` (`hA` is the height onto side `a` = BC) |
| Angle unit | follows `/mode deg\|rad`: `deg` → `A=30` is 30°; `rad` → write `A=pi/6` (writing `A=30` errors because 30 radians > 180°) |
| Values | any expression: `a=1/2`, `b=sqrt(2)`, `hA=3*sin(30)` |
| Strategy | SSS/SAS/ASA/AAS/SSA and "height-reducible" cases go through **formula solutions**; the rest fall back to **numeric solving** |
| Height reduction | `hA·a = 2S` (height×base→area), `hA = b·sinC = c·sinB` (height×adjacent angle→side), `a·hA = b·hB` (two heights→side ratio) |
| Multiple solutions | `SSA` (`b·sinA < a < b`) and "two sides + area" give two solutions, listed as `解 1:` / `解 2:` |
| Exactness | `triangle(a=3 b=4 c=5)` gives `C = 90` (exact), `面积 = 6`, `R = 5 / 2`, `r = 1`; non-special angles get `≈` |
| Known-first | after verification, known values are written back verbatim (`A=30` shows `30`, not `29.9999…`) |
| Validation | triangle inequality, angle sum, over-determination consistency are all checked; contradictions report clearly |
| No conflict | `a=3` (equation), `a=3, b=4, c=5` (system), `x=1 y=2` etc. keep their existing behavior; the old **bare** form `a=3 b=4 c=5` was removed (it now reports "多余的字符"), so it can never be confused with systems or implicit multiplication |

### Complex Number Support

`i` is the **imaginary-unit constant** (same level as `pi`/`e`) and can be used directly:

```
> (2+3i)*(1-i)
= 5 + i
> i^2
= -1
> 1/(1+i)
= 0.5 - 0.5i
> sqrt(-4)
= 2i
> ln(-1)
≈ 3.1415926535897932385i
> abs(3+4i)
= 5
> exp(i*pi/2)
≈ 1i
```

| Capability | Description |
|---|---|
| Arithmetic | add, subtract, multiply, divide (division via conjugate); `Gaussian` rational results are **exact** |
| Integer powers | fast power (`i^100 = 1`, `(2i)^3 = -8i`), negative integer powers give the reciprocal |
| Roots | `sqrt`/`sqr`: negative reals give a pure imaginary root (`sqrt(-4) = 2i` exact, `sqrt(-2) = √2·i`); square root of a complex goes through polar form |
| Transcendentals | `exp`, `ln`, `sin`/`cos`/`tan` (numeric; `BigFloat` internally uses **radians**) |
| Component ops | `abs(z)` modulus, `re(z)` real part, `im(z)` imaginary part, `conj(z)` conjugate, `arg(z)` argument (radians); these four **also work on reals** (a real is a complex with zero imaginary part): `im(2) = 0`, `arg(-1) = π`, `arg(0)` errors "0 没有辐角" |
| Unsupported | non-integer powers (`i^(1/2)` errors), `floor`/`ceil`/`round`/`frac`/`sign`/`mod`/`idiv`/`gcd`/`nCr`/inverse-trig and other real-only functions (error `复数不支持该函数: xxx`), equations with a complex unknown |

**Display format**: `a + bi` / `a - bi`; real part omitted when 0 (`2i`, `-i`); imaginary coefficient omitted when ±1 (`i`, `-i`); a fractional/radical imaginary part gets parentheses (`(1 / 2)i`). The `=` prefix requires both parts to be exactly representable, otherwise `≈`.

**Behavior changes vs. older versions** (upgrade notes):

- `i` is no longer a single-letter unknown ⇒ `i^2-4=0` (an equation with unknown `i`) no longer works (now reports "方程中没有变量"); use another letter (`x`, `y`, …);
- `sqrt(-4)`, `sqr(-2)`, `ln(-1)` changed from error to a complex result (undefined over reals, defined over complexes);
- odd roots are unaffected: `cbrt(-8) = -2` stays in the reals.

### Working Precision & Display Format (`/mode prec|digits|sci|group`)

| Command | Action | Range |
|---|---|---|
| `/mode prec N` | working precision (internal digits), default 80 | 20 ~ 2000 |
| `/mode digits N` | displayed significant digits, default 20 | 1 ~ current precision |
| `/mode sci on\|off` | scientific notation for big numbers (default `on`) | —— |
| `/mode group on\|off` | thousands separators in the integer part (default `off`) | —— |

```
> /mode prec 40
已设置工作精度为 40 位小数（显示 20 位有效数字）
提示: 显示位数（20）少于工作精度，可用 /mode digits 提高
> /mode digits 30
已设置显示 30 位有效数字
> pi
≈ 3.14159265358979323846264338328
> /mode group on
已开启千分位分隔
> 1234567*1000
= 1,234,567,000
> /mode sci off
已关闭科学计数法显示（大数完整写出）
> 10^25/3
≈ 3,333,333,333,333,333,333,333,333.33333
```

- All four settings **persist immediately** (`prec=`/`digits=`/`sci=`/`group=` in the state file) and survive restarts;
- Changing working precision does **not** recompute existing variables or `ans` (`Exact` is precision-independent, `Approx` keeps its own precision); constant caches (π/e/ln2/small-integer ln) are bucketed by target precision and recompute per new precision;
- Higher precision is slower (`/mode prec 2000` with big numbers can be noticeably slow); `/mode prec` only validates the range, it doesn't warn about time;
- Non-default precision/display settings append to the startup banner's mode string (e.g. `LineIO/Degree/Fast/简体中文/40/30/sci-off/group`); with defaults the banner stays the four-part `LineIO/Radian/Fast/简体中文`;
- Thousands separators apply only to **pure decimal** results (fractions `1 / 2`, radicals `sqrt(2)`, scientific notation, and π-containing symbolic strings are not comma-grouped).

### Binary Functions (modulo / integer division / root / combinatorics / number theory)

| Function | Description |
|---|---|
| `nroot(x, n)` | n-th root of `x` (`n` an integer from 2 to 1000000); perfect n-th powers are **exact** (`nroot(16,4)` → `= 2`), else numeric; negatives only when `n` is odd |
| `mod(a, b)` | **Euclidean modulo**: result always satisfies `0 ≤ r < |b|` (`mod(-7,3)` → `= 2`, `mod(7,-3)` → `= 1`) |
| `idiv(a, b)` | **Euclidean division** (matching Rust's `div_euclid`): `a = b·idiv + mod` (`idiv(-7,3)` → `= -3`, `idiv(7,-3)` → `= -2`) |
| `nCr(n, r)` / `nPr(n, r)` | combination / permutation; arguments must be non-negative integers; `r > n` gives `0`; Fast mode `n ≤ 10000` (Deep lifts it) |
| `gcd(a, b)` / `lcm(a, b)` | greatest common divisor / least common multiple; arguments must be integers (`gcd` always non-negative) |
| `isprime(n)` | primality test, returns `1`/`0` (small-prime trial division + 12-base Miller-Rabin; deterministic for `n < 3.3×10^24`) |
| `nextprime(n)` | smallest prime greater than `n` (`nextprime(0)` → `2`) |

- `mod`/`idiv` require **exact values** (`mod(pi,2)` errors), and division by zero errors;
- **Don't confuse the three divisions**: `mod`/`idiv` are Euclidean (non-negative remainder), `bigint_ext::div` is truncated division (remainder sign follows the dividend), plain `/` is exact rational division;
- Primality Fast limit is `10^24` (12-base Miller-Rabin is deterministic below `3.3×10^24`); Deep mode removes the limit.

### Inverse Hyperbolic Functions

All three are built from `ln`/`sqrt` (reusing the high-precision numeric layer), so they are **numeric-only, no exact form** — except that zeros (`arcsinh(0)`, `arccosh(1)`, `arctanh(0)`) still give the exact `0`:

- `arcsinh(x) = sign(x)·ln(|x| + √(x²+1))` —— uses `|x|` inside the log, negates for negatives, to avoid `ln(negative)`;
- `arccosh(x) = ln(x + √(x²-1))`, `x < 1` → domain error;
- `arctanh(x) = ½·ln((1+x)/(1-x))`, `|x| ≥ 1` → domain error; for `|x|` extremely close to 1 the result is very large and significant digits shrink noticeably at 80 digits (the input's precision is amplified).

### Interactive Enhancements (Tab Completion / Inline Hints / Session Scripts)

**Tab completion** (rustyline):

- Line-start commands: `/mo` + Tab → `/mode`;
- Function/constant/stored-variable names: `sin` + Tab → `sin` / `sinh`, `ta` + Tab → `tau`, `PI` + Tab → `PI_VAR`;
- **Single-letter tokens get no candidates**: `x`, `y`, `xy` (implicit-multiplication factors) are never completed into function names, so completion can't break the `xy = x·y` semantics;
- An empty token (line start / after a space) doesn't pop the full candidate list.

**Inline hints** (ghost text shown when the cursor is at line end):

- Full command name → usage, e.g. `log` hints `(base, x)`, `/mode` hints the sub-commands;
- Only shown when the cursor is at line end and the text just left of it is a full function/command name — never interferes with existing text.

**Session scripts**:

| Command | Description |
|---|---|
| `/save <path>` | write the current session as a **replayable script**: modes, precision, display switches, `/set` colors, all `/let` variables |
| `/load <path>` | replay the script line by line (per-command confirmations are muted during replay; one summary line at the end) |
| `/reset` | clear all variables and `ans` (modes, colors, precision unchanged) |
| `/reset all` | restore default modes, colors, precision and timing (language stays) |

- `/save` variable form: exact values use MathIO symbolic form (`1 / 2`, `(1 / 2)*sqrt(2)`, `2*pi`; still exact after replay); approximate values write the **full decimal string** (no precision loss; becomes an exact decimal on replay);
- `/save` vs. the state file (`~/.hipercalc_state`): the former is an **explicit, nameable, partial-state** script, the latter is the auto-persisted session snapshot (with language & colors); delete it to restore defaults.

### UI Language `/lang` `/language`

Supports **简体中文 / 繁體中文 / English**:

- No argument: prints the three-line list, **↑/↓ to move, Enter to confirm** (Esc cancels); the current one is marked with `❯`; when piped/redirected it degrades to "type an index or language code then Enter" (empty line = confirm current);
- With an argument switches directly: `/lang en`, `/lang zh-TW`, `/lang 2`, `/lang English`;
- The language lives in the user config (`lang=` in `~/.hipercalc_state`) and persists;
- **First launch** detects the system language: Chinese UI → corresponding simplified/traditional, everything else → English;
- **Switching only affects subsequent output**; already-printed content is not rewritten.

```
> /lang
语言 / Language:
  ❯ 简体中文
    繁體中文
    English
↑/↓ 选择，回车确认（也可直接输入序号或 /lang <代码>）
> 2
已切换语言：繁體中文（仅对后续输出生效）
```

### `/set` Color Settings

- Categories (English name + Chinese annotation): `functions(函数)` `operators(运算符)` `commands(指令)` `brackets(括号)`
  `constants(常量)` `numbers(数字)` `prompt(提示符)` `result(结果)` `error(错误)`;
- Colors: `black(黑)` `red(红)` `green(绿)` `yellow(黄)` `blue(蓝)` `magenta(洋红)` `cyan(青)` `white(白)` plus the `bright_*` series (`bright_black(亮黑)` … `bright_white(亮白)`;
- **Bare `/set`**: prints usage (categories and colors with Chinese annotations) then lists each category's current color (each color name shown in its own color);
- Examples: `/set functions green`, `/set operators yellow`;
- Settings **write immediately** to `~/.hipercalc_state` and persist; invalid category/color reports and doesn't write.

The REPL highlights keywords **live** (functions green, operators yellow, commands cyan, brackets magenta, constants blue, …); `/help` body, command usage hints and result output share the same palette (`colorize_text`): the same keyword is the same color on the input line, in the help list, and in the hints. Commands are recognized only at the **line start** (whitespace allowed); a `/` mid-line is division (`1/2`, `1/x` are not commands).

## 12. Special-Angle Exact Values

| Meaning | Description |
|---|---|
| Angles | `0° 15° 30° 45° 60° 75° 90°` and their induced-form generalizations |
| Radians | `0, π/12, π/6, π/4, π/3, 5π/12, π/2` etc., i.e. rational multiples of `pi` |
| Inverse trig | `arcsin(1/2)=pi/6`, `arccos(1/2)=pi/3`, `arctan(1)=pi/4` |

Non-special angles go through 80-digit numeric computation.

---

# Part 2: Code Walkthrough

## 13. Module Overview

```
src/
├── bigfloat.rs       arbitrary-precision float (the numeric core: +−×÷/root/power/exp/ln/trig series + scale guards)
├── bigint_ext.rs     big-integer helpers (own long division + integer sqrt, bypassing num-bigint's BZ defect)
├── calc_mode.rs      calc mode switch (Fast has scale guards / Deep brute-forces)
├── i18n.rs           UI language (zh-Hans/zh-Hant/English table + runtime whole-line translation + system language detection)
├── number.rs         dual representation: symbolic exact + numeric approximate
├── parser.rs         recursive-descent parser + evaluator + top-level entry + function whitelist constants
├── trig.rs           special-angle exact value tables (degrees/radians/inverse trig)
├── display.rs        MathIO symbolic output / LineIO decimal (with big-number scientific notation)
├── equation.rs       equation info extraction, polynomial/linear detection
├── solver_linear.rs  linear systems (Gaussian elimination)
├── solver_nonlinear.rs  nonlinear systems (multi-dimensional Newton)
├── solver_poly.rs    polynomial root-finding (exact/numeric/complex)
├── solver_factor.rs  polynomial factorization (uni/multi-variate, with candidate budgets)
├── solver_fit.rs     polynomial fit (points/vertex → general form + vertex form; templates & under-determined relationships)
├── solver_triangle.rs  triangle solver (sides/angles/heights → sides, angles, heights, area, perimeter, two radii)
├── solve_aux.rs      result prefix, periodic general form, root collection
├── state.rs          session persistence (modes + /let variables + /set colors, ~/.hipercalc_state)
└── main.rs           REPL, commands, /help, color config (/set persistence), history, highlighting, timing
```

## 14. Core Data Structures

### BigFloat (bigfloat.rs)

Arbitrary-precision float, essentially a **BigInt mantissa and a negative power of 10**:

```rust
pub struct BigFloat {
    pub value: BigInt,      // mantissa (scaled to 10^precision)
    pub precision: usize,   // number of decimal digits
}
```

i.e. `value = mantissa / 10^precision`. Key constants:

- `PRECISION = 80`: internal computation precision (decimal digits)
- `MARGIN = 20`: extra headroom kept during operations, to avoid truncating intermediate results
- `DISPLAY_DIGITS = 20`: significant digits in LineIO output
- `EXP_ARG_LIMIT_LOG10 = 6.0`: `exp` argument magnitude limit (|x| ≤ 10^6)
- `MAX_RESULT_DIGITS = 1e6`: decimal-digit limit for power results

Every operation returns a `BigFloat` and finishes with `round_to` at the target precision. **The four arithmetic ops operate directly on BigInt**; `sqrt` seeds with an integer square root then Newton-iterates; `ln` first rescales the argument to `x = s·2^m` (`s∈[1,2)`) then converges with an `atanh` series, with `ln(2)` computed explicitly as `2·atanh(1/3)` — this is what keeps 20 correct digits even for large arguments.

**Constant caches** (`pi` / `e` / `ln2` / small-integer `ln`):

- All three are iterative/series computations (π is Gauss-Legendre; e and ln2 are infinite series), each far costlier than one arithmetic op;
- They're called extremely often: `pow` goes through `exp(b·ln a)` and needs ln2 every time, trig reduction needs π every time, `ExactExpr::to_bigfloat` recomputes on every `Pi`/`E` term — **Newton equation solving hits them every iteration**;
- So all three are cached **by target precision** (`HashMap<usize, BigFloat>` + `OnceLock`/`Mutex`), with `ln2` pulled out as `ln2_cached`;
- **Small-integer `ln` cache** (`HashMap<(BigInt, usize), BigFloat>`, cleared when full at 64 entries): `pow(a, b)` needs `ln(a)` every round, and in an exponential equation (`3^x=27`) the base is fixed but `ln(3)` gets recomputed repeatedly (atanh series ≈ 100 terms); only integer bases 2..=10^6 are cached (arbitrary arguments never enter, to avoid pollution). Measured: `3^x=27` 5s→3s, `5^x=125` 2s→1s;
- Caches only affect speed: a hit returns the same value at the same precision, a miss computes as usual, a poisoned lock silently skips the cache. Measured ~4× faster on system solving after caching — a key reason equations like `2^x=8` went from tens of seconds to a few.

**Numerical stability & scale guards** (against "silent wrong results / hangs"):

- `exp(x)`: first halves the argument to `|x| ≤ 1` (`exp(x) = exp(x/2^k)^(2^k)`) then expands the series, with a **relative** convergence test `|term| ≤ |result|·10^-work_prec`; arguments beyond `EXP_ARG_LIMIT_LOG10` error out directly.
  (The old code used an absolute test with no argument limit — for large |x| the series never converged and intermediate integer digit counts exploded ⇒ hang + wrong result.)
- `pow(a, b)`: integer exponents use fast power with a result-digit guard; the general case first checks `|b·ln a|`, erroring if over limit instead of silently returning a truncated series sum.
- `sqrt`/`ln`/`atan`: arguments are first scaled into the convergence domain before expanding the series; the convergence domains must not be changed casually.

### Big-Number Display Rules

`to_significant_string(max_digits)`: prints the whole string when decimal digits are within the limit; **switches to scientific notation** when the integer part exceeds `max_digits` (`d.ddd×10^n`, keeping the magnitude). The old code truncated the string, dropping the integer part's low digits along with the magnitude (`10^25/3` once showed as `3.3e19`).

### Big-Integer Division & Root (bigint_ext.rs)

Big-integer ops dispatch by size: small → num-bigint's native impl (fast), large → own Knuth-D textbook long division and Newton integer square root. The reason: num-bigint 0.4.7's Burnikel-Ziegler division triggers `debug_assert!(ah < b)` in the "dividend ≈ divisor²" shape, panicking the process — measured on `sqr(10^100000)`, `(10^200000+pi)/(10^100000+pi)`. `bigint_ext.rs` also carries `#[cfg(test)]` unit tests (division identity `a = q·b + r`, sqrt `s² ≤ n < (s+1)²`), runnable by `cargo test`.

### calc_mode (calc_mode.rs)

Process-level calc-mode switch (`AtomicBool`): `Fast` (default) / `Deep`.

- Why a global: scale guards live in low-level numeric functions in `bigfloat.rs` / `number.rs` / `parser.rs` / `solver_factor.rs`, which have no `Evaluator` context; the REPL executes computation single-threaded, so an atomic bool suffices.
- Every guard checks `crate::calc_mode::is_deep()` to decide whether to lift:

| Location | Fast | Deep |
|---|---|---|
| `bigfloat::exp` / `pow` | error over `EXP_ARG_LIMIT_LOG10` / `MAX_RESULT_DIGITS` | no guard |
| `number::check_pow_scale` | error over 10^6 result digits | no guard |
| `parser::check_exp_range` / `check_trig_range` | error over argument limit | no guard |
| `parser` factorial limit 10000, scientific-notation exponent limit 10^5 | error | no guard |
| `solver_factor` factor enumeration / sqrt simplification / quadratic budget | stop at limit, note "possibly incomplete" | no limit (possibly very slow) |
| `solver_poly::durand_kerner` degree limit 200 | high-degree factor (> 200) → "use /mode deep" | no limit (possibly very slow) |
| `bigfloat::to_significant_string` | 20 significant digits / scientific notation | full precision, no scientific notation |
| `newton_solve` / `solver_nonlinear` divergence guard | **kept** | **kept** (otherwise `2^x=8` hangs) |

### Number (number.rs)

Dual representation: exact when possible, approximate otherwise:

```rust
pub enum Number {
    Exact(ExactExpr),   // symbolic exact: rationals + sqrt/pi/e terms
    Approx(BigFloat),   // high-precision approximation
}

pub struct ExactExpr {
    pub terms: Vec<ExactTerm>,   // Rational(p/q) | Sqrt(coeff, radicand) | Pi(coeff) | E(coeff)
    pub denominator: BigInt,     // denominator (positive integer)
}
```

The pipeline (order must not change): **drop zero terms → `normalize_terms` (normalize `Sqrt(x,1)` into a rational) → `simplify_expr` (merge, reduce, rationalize denominator)**. `Exact + Exact` only falls back to `Approx` when merging fails. In division, `div_exact` first handles "divisor denominator ≠ 1" (like `sqr(2)/2`), then the single-sqrt / conjugate-rationalization branches — this is what makes denominator rationalization correct.

**Gotchas (fixed, don't revert)**:

- `div_exact`'s "divisor is a single sqrt term" branch: `self/(c·√r) = self·√r·(1/(c·r))`; `1/(c·r)` must be multiplied into every coefficient as a **rational factor** (`scale_term_rational`), not just `c.numer()*r` (which drops the coefficient denominator, e.g. `1/(sqr(2)/2)` would miss a factor of 2).
- `Number::int_pow` expanding `(a√b)^n` must use `a`'s **rational** power: `n=2m` even → `a^(2m)·b^m`; `n=2m+1` odd → `a^(2m+1)·b^m·√b`. (The old code truncated with `numer/denom` integers, turning `1/2` into 0; and even powers missed one factor of `a`: `(2√3)^2` once gave 6.)
- `Number::sqrt` fully simplifies rational `p/q`: `sqrt(p/q) = sqrt(p·q)/q`, exact radical (`sqr(1/2)` → MathIO `(1 / 2)*sqrt(2)`).

### Expr & Evaluator (parser.rs)

AST:

```rust
pub enum Expr {
    Number(Number),
    Variable(String),          // single lowercase unknown / ans / uppercase stored variable
    Binary(Box<Expr>, BinOp, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Function(String, Vec<Expr>),  // argument vector: mostly one arg, log takes two
    Sd(Box<Expr>),
    Factor(Box<Expr>),
    Equation(Box<Expr>, Box<Expr>),
    System(Vec<Expr>),
}
```

`Evaluator` holds runtime state: `display_mode`, `angle_mode`, the previous result `ans`, and the `/let` variable table `vars: BTreeMap<String, Number>`. Variable resolution precedence: **explicit substitution (equation unknown) → `ans` → stored variable → undefined error**.

### Parser Structure (recursive descent)

Entry chain: `parse_system()` (comma-separated system) → `parse_equation()` (single `=`) → expression.

```
parse_expression → parse_term → parse_unary → parse_power → parse_atom
      + -            */ + implicit      unary ±        power        parens/abs/number/identifier
```

- **Implicit multiplication**: `parse_term` auto-multiplies on digits, letters, `(`, `|`, e.g. `3x`, `2(x+1)`;
- **Absolute value `| |`**: parsed at the `parse_atom` level (same as parens), using `abs_depth` to tell open/close bars apart so a closing `|` isn't swallowed by implicit multiplication;
- **Postfix factorial `!`**: handled in `parse_power`, binds to its operand, `2^3! = 2^(3!)`; generates the internal function name `"fact"` (not in the whitelist, so users can't type `fact(...)`);
- **Multi-arg functions**: `(` supports a comma-separated argument list; currently only `log(b, x)` uses two;
- **Function whitelist**: `parser::FUNCTIONS` is the single source (parsing validation, multi-letter splitting, `main.rs` highlighting); adding a function only needs it and `Evaluator::eval_function`;
- **Identifier classification**: checks `pi`/`e`/`ans` first, then single-letter variables, then uppercase stored variables (`X`, `AB`, `PI_VAR`), then splits the rest into a sequence of single-letter variables (`xy^2 = x*(y^2)`); a variable string starting with `e` is treated as "constant e + the remaining variables" (`2ex` → `2*e*x`, `ee` → `e^2`); multi-letter splitting consumes only the first character and backs the position up for implicit multiplication.

### Argument Validation in the Evaluator (parser.rs::eval_function)

- `log(b, x)`: base must be > 0 and **exactly not 1** (`base.as_rational().is_one()`, or deviation from 1 above 1e-40). The old code used `base.rounded(0) == 1`, misjudging bases like 0.5/0.9/1.2 as 1;
- `exp`/`sinh`/`cosh`: `check_exp_range` rejects |x| > 10^6;
- `tanh`: for |x| > 100 returns ±1 directly — at 80 digits `1 - tanh(x) = 2e^{-2x}/(1+e^{-2x}) < 1e-80` is already saturated; this is the same thing as "avoiding `exp(2x)` argument explosion" (the old threshold 10^10 would first hit the exp argument limit and misreport);
- Trig `sin/cos/tan/cot/sec/csc`: `check_trig_range` rejects radian arguments |x| ≥ 10^78 (degree mode first converts by `×π/180` to get the radian magnitude).

### Top-Level Entry parse_and_eval

```rust
pub enum EvalResult {
    Value(Number),
    SdValue(Number),
    Factor(Expr),
    Equation(Expr, Expr),
    System(Vec<(Expr, Expr)>),
}
```

First tries the two top-level function forms `sd(...)`, `fac/factor(...)`, then parses a system/equation, and finally falls back to evaluating an ordinary expression (including identity detection and "both sides simplify to equal").

## 15. Solver Logic

### trig.rs — Special-Angle Exact Values

In degree mode, the angle is first normalized to `[0, 360)` then mapped to a `[0, 90]` reference angle and looked up in a table (`15°`-step integer fractions, rational multiples of `pi`), building `(a + b√c)/d` or double-radical exact values via `make_surd` / `make_double_surd`; other angles use `BigFloat` high-precision trig, and inverse trig back-fills angles for special values like `0, ±1/2, ±1, ±√3/2...`.

### solver_linear.rs — Linear Systems

`gaussian_elimination` pivots, eliminates and back-substitutes on the augmented matrix, returning unique / no-solution / infinite-solution info (`LinearSolution`). `format_linear_solution(sol, mode)` prints per `DisplayMode`, prefixing each entry with `=`/`≈` (consistent with single-equation output).

### solver_poly.rs — Polynomial Root-Finding

- `extract_polynomial` (+ `collect_terms`) extracts ascending-order coefficients from the AST; `collect_terms` can expand `/let` variables, `ans`, and `(x+1)^n` (n≤64) into constant/polynomial terms;
- `solve_poly_full`: solves factored factors **exactly for linear/quadratic**, and uses **Durand-Kerner** (Weierstrass) iteration for `deg ≥ 3` to get **all complex roots** (own `Cx { re, im }` complex arithmetic, imaginary part near 0 → real root). High-degree factors get four targeted treatments:
  - **Initial values**: n starting points spread evenly on a circle of radius `R = min(Cauchy bound, max(|a₀/aₙ|^(1/n), 1))` (angle offset by half a step) — the Cauchy bound is too wide for sparse polynomials (`x^96+…+1` has bound 2 but all roots on the unit circle), the geometric estimate fits the true root modulus better; if the first round doesn't converge, retry once at the Cauchy-bound radius;
  - **Two-phase precision**: first 40-digit coarse convergence (each round O(n²) complex ops, BigInt digits halved, per-round cost ≈ 1/2~1/3 of full precision), then 80-digit refinement (Newton-type, reaches the target in ~ten rounds);
  - **Report non-convergence honestly**: after exhausting the step limit it returns an error, **never outputting intermediate values as roots** (the old code printed unconverged iterates; `x^100=1` once gave "roots" with modulus ≈1e21);
  - **Fast degree guard**: DK is O(n²) per round; measured n≈200 ≈ 25s, n≥300 > 1min, so Fast errors on factors > 200 with a `/mode deep` hint (Deep has no limit);
- `newton_solve`: single-root Newton, `max_iter` configurable; every step has a **divergence guard** (`NEWTON_ABS_LIMIT = 10^6`, correction or |x| out of range → drop the guess), and a residual check on exit eliminates spurious roots;
- `format_solution(sol, mode)` prints roots per `DisplayMode`: MathIO exact roots symbolically (`1 / 2`, `sqrt(2)`), LineIO decimals; the `=`/`≈` prefix is decided by `main.rs::solution_prefix` (LineIO's decimal expansion of `sqrt(2)` is `≈`).

### solver_nonlinear.rs — Nonlinear Systems

Two-phase Newton:

1. **Coarse scan**: the Cartesian product grid of all initial guesses (with a 0.5-step fine grid), `max_iter=16`, tolerance `1e-12`, **forward-difference** Jacobian, deduplicating candidates;
2. **Fine convergence**: per candidate `max_iter=80`, tolerance `1e-45`, **central-difference** Jacobian, deduplicated before returning.

`same_solution` judges duplicates at `1e-12`. Any component above `NEWTON_ABS_LIMIT` is divergence; a "step too small" early return must still pass a residual check (`loose_tol = 1e-25`) to avoid flat-region spurious roots.

### solver_factor.rs — Factorization

Internally represents polynomials as `Poly` (monomials `Mono` (variable-exponent vector) + `BigRational` coefficients, merged in a BTreeMap):

1. `expr_to_poly` / `collect_expr_terms(expr, vars, evaluator, terms)` expand the AST into a rational-coefficient polynomial (with `^` power folding, polynomial base 2..=64), where `ans` and `/let` variables substitute into the constant term;
2. **Univariate** `factor_univariate`: pull out constant factor → common factor → **rational root theorem** (`int_divisors` enumerates ±p/q, u64 trial division for speed, size limit 1e13) → **synthetic division** to reduce degree → the remaining quadratic uses the discriminant to split radicals (MathIO) or mark irreducible (LineIO); reaching a quadratic residual means rational-root enumeration failed ⇒ in LineIO a retained quadratic is **definitely irreducible over the rationals**, appended with `（有理数域内不可再分解）`; radical pairs (`SqrtPair`) join their two factors with ` * ` (`(x - sqrt(2)) * (x + sqrt(2))`); fractional coefficients before a radical get parentheses (`(1 / 2)*sqrt(2)`, integer coefficients don't — matching `display.rs`);
3. **Multivariate** `factor_multivariate`: project along coordinate axes to find linear factors (`find_linear_factor`) and repeatedly long-divide; `is_homogeneous` + `lift_univariate` handle homogeneous polynomials; equal exponent vectors merge (`push_factor_rep`);
4. `factor_expr(evaluator, expr, mode)` decides the domain per display mode and outputs the product string;
5. **Results use `FactorOutcome { factors, has_irreducible, truncated }` to distinguish two kinds of "not fully done"**: `has_irreducible` (irreducible in this domain) → `有理数域内不可再分解`; `truncated` (rational-root candidates or quadratic enumeration over budget) → `候选枚举超出规模上限，结果可能不完整`. Budget constants: `QUADRATIC_TRY_BUDGET = 5000` long divisions, `QUADRATIC_BMAX_LIMIT = 200000`.

### solver_fit.rs — Polynomial Fit

Given some `(x, y)` points (`P(x, y)` additionally marks the vertex), find the polynomial through them / solve unknown parameters from a template. Core data:

```rust
pub struct FitPoint  { pub is_vertex: bool, pub x: Expr, pub y: Expr }
pub struct FitTemplate { pub rhs: Expr, pub var: char }
pub struct FitInput  { pub points: Vec<FitPoint>, pub template: Option<FitTemplate> }
pub struct LinForm   { k: Number, terms: BTreeMap<String, Number> }  // k + Σ coeff·free parameter
pub struct FitSolution {
    pub params: Vec<(String, LinForm)>,  // each unknown = linear form in free parameters
    pub free: Vec<String>,               // free-parameter names
    pub coeffs: Vec<LinForm>,            // simplified ascending coefficients (general form)
    pub var: char,
}
```

Two solving paths:

1. **No template** (`template == None`): an ordinary point gives 1 equation, a vertex point adds one **analytic derivative row** (`[0, 1, 2a, 3a², …]` = 0 at the vertex x, i.e. `f'(m)=0`), forming a Vandermonde-type linear system handled by `solver_linear::gaussian_elimination`; `unique` → the one polynomial, else report insufficient/contradictory constraints.
2. **With template**: treat each unknown parameter as an unknown, build basis functions `φ_j = extract_polynomial(template term j, var = 1) − g` (`g` is the template's constant part with no unknown parameters, **which must be subtracted** or constant-offset templates shift entirely); each constraint gives one row `Σ φ_j(x_i)·p_j = y_i − g(x_i)`; own `rref()` gives the reduced row-echelon form, pivot-column parameters are written as `LinForm`, free-column parameters become free parameters → "parameter relationships". Also double-checked by `check_param_linearity` (parameters must appear linearly) and `verify_solution` (back-substitution residual tolerance `10^(3·precision/4)`).

Output rendering:

- `format_fit_solution` **always starts with `y = …`** (the left side is decorative, never `f(x)`); coefficients get parentheses when fractional or containing spaces/division (`(1 / 2)*x^2`);
- `format_vertex_form`: only when "actually quadratic (leading coeff ≠ 0) and all coefficients constant" appends the vertex form `y = a*(x - m)^2 + k`, where `m = -b/(2a)`, `k = c - b²/(4a)`; returns `None` (no output) when `m == 0` or coefficients contain free parameters;
- `format_fit_params`: prints `参数 = 关系式` lines then a `自由参数: …` line;
- Scale limit `FIT_MAX_SIZE_FAST = 16` (constraints/parameters), above → `/mode deep`, Deep lifts it.

### solver_triangle.rs — Triangle Solver

Core data:

```rust
pub enum TriPart { Side(u8), Angle(u8), Height(u8) }   // index 0/1/2 → a-A / b-B / c-C
pub struct TriangleInput  { pub parts: Vec<(TriPart, Expr)> }   // not evaluated at parse time
pub struct TriangleSolution { /* three sides, three angles (radians), three heights, area, perimeter, R, r */ }
```

`solve_triangle` has four steps:

1. **Evaluate + normalize**: `evaluate_with_vars` per item; reject complex; sides/heights must be positive, angles must be in `(0, π)` (input angles are converted to **internal radians** per `Evaluator::angle_mode`); duplicate assignments are tolerated only when the values match.
2. **Reduce** (`complete_angles` + `reduce_heights`): two angles complete the third; heights convert into area or sides via `S = hA·a/2`, `hA = b·sinC = c·sinB`, `a·hA = b·hB = 2S`. After reduction, if there is **no length at all**, report "insufficient information" (three heights or three angles only fix the shape; three heights additionally print the reduced integer ratio `a : b : c`).
3. **Analytic first, numeric fallback** (`solve_analytic` → `solve_numeric`):
   - SSS uses Heron's formula for area (`sqrt` stays exact) and **each angle via the law of cosines** (not `π−A−B` accumulation, to preserve exact `90°`/`60°`); SAS uses the law of cosines for the third side; AAS/ASA use the law of sines for the ratio; SSA gives 0/1/2 solutions by comparing `h = b·sinA` with `a`; also "two sides + area" and "one side + its opposite angle + area" reduction cases.
   - Everything else uses the `(A, B, s=2R)` parameterization (`C = π−A−B`, `a = s·sinA`, `hA = s·sinB·sinC`) with multi-start Gauss-Newton (normal equations `JᵀJ Δ = −Jᵀ r`, angle seeds as integer multiples of `π/8`), deduplicating after solving.
4. **Verify + write back**: every known quantity is back-substituted and compared (over-determination contradictions are caught here); after passing, known values are written back verbatim, then `R = abc/4S`, `r = S/(p/2)`, `hA = 2S/a` derive the rest (all via `Number` exact arithmetic).

`format_triangle_solution` renders a fixed 5 lines (sides / angles / heights / area+perimeter / two radii); each value's `=`/`≈` is decided by `solve_aux::result_prefix`; angles are converted with `parser::radians_to_degrees` (exact `Pi(coeff)` → exact degrees) before output. Multiple solutions are sectioned with `解 N:` by `handle_triangle`.

## 16. Main Equation Handling (main.rs)

`handle_input_result` dispatches on `EvalResult`:

- `Value` / `SdValue` → `fmt_value` formats per mode with the `=` / `≈` prefix (`solve_aux::result_prefix`). `SdValue`'s display form is **swapped** with the mode (MathIO→decimal, LineIO→symbolic), and the prefix is decided by the swapped form (`sd(1/2)`: LineIO `= 1 / 2`, MathIO `= 0.5`);
- `Factor` → `solver_factor::factor_expr(&evaluator, expr, mode)`;
- `Equation` → `handle_equation`: collect variables and **filter out stored uppercase variables** → single unknown goes through the `newton_with_guesses` / `solve_poly_full` hybrid path → `solve_aux::collect_all_roots` merges multiple solutions, then `format_periodic_roots` tries the **periodic general form**;
- `System` → `handle_system`: first check all-linear (Gaussian elimination), else multi-dimensional Newton;
- `Fit` → `handle_fit`: `solver_fit::solve_polynomial_fit` solves params/coefficients, outputs `y = …` (quadratics append the vertex form). Recognition entry is `parser::parse_and_eval` → `looks_like_polynomial_fit` (line-start `(` / `P(` / `p(` + top-level comma + trailing-character whitelist) then `Parser::parse_fit`, before the `sd(…)` check to avoid clashing with function calls.
- `Triangle` → `handle_triangle`: `solver_triangle::solve_triangle`, outputs 5 lines (`解 N:` sections when multiple). Recognition entry is `parser::parse_triangle_call`, handling the `triangle(… )` function form: the parentheses are split by **top-level commas or whitespace** (`split_triangle_items` is depth-aware, so the comma in `log(2,8)` is not a separator) and each item is `<notation>=<expression>`. Like `sd`/`fac` it **must be the outermost function of the whole expression**: if `triangle` appears anywhere else (`2*triangle(...)`) or anything follows the closing `)`, it errors with "must be the outermost function of the whole expression" and does **not** fall back to the normal grammar (which would give a misleading message).

### Session Persistence (state.rs)

`~/.hipercalc_state` (same directory as `.hipercalc_history`) stores display/angle/calc modes, all `/let` variables and `/set` colors:

| Line format | Meaning |
|---|---|
| `display=mathio\|lineio`, `angle=deg\|rad`, `calc=fast\|deep` | the three modes |
| `var:NAME=E:<MathIO expression>` | exact value: stored symbolically (`1 / 3`, `(1 / 2)*sqrt(2)`, `pi`), re-parsed on load, **lossless** |
| `var:NAME=A:<value>:<precision>` | approximate value: BigFloat's internal decimal form (mantissa + digit count), restored directly, **lossless** |
| `color:<category>=<color name>` | `/set` highlight colors (e.g. `color:functions=red`); category and name validated on load, invalid entries skipped |

- Exact values aren't serialized directly because `ExactExpr` is a composite "terms + denominator" structure that's awkward to serialize and unstable across changes, while MathIO's symbolic output is already a parseable expression — re-parsing it (with a temporary `Evaluator`, not polluting the session's `ans`/`vars`) restores exactly;
- `persist_state()` runs after every change (`/let`, `/del`, `/mode`, `/set`) and once more on exit;
- On load, variable names and values are validated, invalid lines are skipped silently; a missing file means defaults.

### Color Highlighting (main.rs)

- `ColorConfig` holds the colors of 9 categories (functions/operators/commands/brackets/constants/numbers/prompt/result/error), with `set_category`/`get_category`/`to_pairs` shared by `/set` and the state file; category and color names live in two constant tables `COLOR_CATEGORIES` / `COLOR_OPTIONS` (English name + Chinese annotation + color value), used for both parsing and reverse lookup;
- `colorize_text(line, colors, command_anywhere)` is the single colorizer: the REPL input line (`command_anywhere=false`, only a line-start `/xxx` is a command), the `/help` body and usage hints (`true`) all share it, so the same keyword is the same color everywhere;
- `/help` is a compact no-border version (the `HELP_TEXT` constant): `【…】` section titles use the result color bolded, the body goes through `colorize_text`;
- Bare `/set` prints usage (categories and colors with Chinese annotations) then the current colors (each name in its own color).

### UI Language Implementation (i18n.rs)

- The entry table is keyed by the **Simplified Chinese source text**: `(简体, 繁體, English)`, and the runtime pattern-matches **whole output lines** to translate (`{0}`/`{1}` mark interpolation spots; the interpolated parts are extracted first and filled into the target-language template, so word order can vary per language);
- Lines may contain ANSI color codes: before translating, split into segments on escape sequences, translate only the pure-text segments, then reassemble — so concatenations like `"错误".red() + ": " + message` translate correctly; when a segment has multiple translatable spans, loop the replacement (up to 8 rounds);
- Single-character entries (color names) only match a whole segment exactly, to avoid replacing everywhere; Simplified Chinese is the default and returns verbatim (zero cost);
- Unmatched strings are kept as-is, **never erroring** — to add new text, add the simplified source into `TABLE`;
- All console output in `main.rs` goes through the `lprint!` / `leprint!` / `lprint_inline!` macros (which call `i18n::t` internally); the `/help` body switches wholesale per language (`help_text()`).

### Timing (main.rs)

`handle_input_result` wraps the whole "parse + result generation" in `Timing` (the formatting stage — general-form recognition, root collection, factorization — is often costlier than `parse_and_eval` itself):

- `Timing::begin()`: when stdout is a terminal, starts a refresh thread (`mpsc::channel` + `recv_timeout(100ms)`; closing the channel lets the thread exit immediately, no waiting), refreshing `\r用时：N秒` every whole second;
- The handlers (`handle_equation`/`handle_system`/`handle_factor`) only **produce the result string**; after all computation, `Timing::stop_dynamic()` stops the thread, prints `\r` + 24 spaces + `\r` to clear the line, then a single `println!` prints the result exactly **covering** that line;
- Finally `timing.elapsed_text()` prints `用时：N秒` below the result (sub-second → `用时：<1秒`), also via `lprint!` — writing it with `println!` would bypass i18n and leave it in Chinese under en/zh-TW.

When piped/redirected, stdout is not a terminal (`std::io::IsTerminal`), so the dynamic refresh is skipped to avoid writing `\r` into the output.

### solve_aux.rs — Helpers

- `result_prefix`: returns `"="` / `"≈"` per display mode and exactness (LineIO requires the rational to be **fully writable**: an integer, or a finite decimal with ≤ 20 significant digits, see `finite_decimal_fits`);
- `float_to_exact_rational`: back-fills an exact rational `(p, q)` when an approximate value is close enough to a simple fraction (returns numerator/denominator, caller prints `1 / 2` or `0.5` per mode), tolerance scales with magnitude (`tol = 1e-12·min(|x|,1)`), tiny roots never back-filled to `0`;
- `format_periodic_roots(roots, var, angle_mode)`: detects a single arithmetic sequence or two alternating families, derives the common period `d`, expresses `d` and each residue as `(p/q)·π` via **rational approximation with denominator ≤ 12**, then renders per `angle_mode` as `x = π/6 + k·2π  或  5π/6 + k·2π，k 为整数` or `x = 30 + k·360  或  150 + k·360，k 为整数`. Degree-valued input is first `×π/180`-converted to radians, so **both modes use identical detection** (`pi_frac_str` / `deg_str` only handle labels). Only roots within `±3π` are considered (removes far-root pollution); residue dedup uses **relative + absolute** tolerance (`tol·max(|a|,|b|) + 1e-40`) and dedups again by **recognition label** (rational multiple of π), to avoid repeating the same family;
- `best_rational` / `pi_ratio`: pick the closest fraction with denominator ≤ 12 (replacing the old fixed candidate tables, so table-missing angles like `5π/3` are covered); `deg_str` renders `(p,q)` as exact degrees (`(1,6)→30`, `(1,8)→22.5`);
- `collect_all_roots`: multiple initial guesses + two-phase Newton (coarse 1e-8 → fine 1e-12) collect and dedup; when the equation contains a trig function of the unknown and the mode is degrees, guesses scale by `×180/π` (see `equation::has_trig_of_var`).

## 17. Data-Flow Overview

```
User input
  │
  ▼
main.rs: Timing::begin()（start the \r dynamic timing thread on a terminal）
  │
  ▼
parse_and_eval (parser.rs)
  ├── looks_like_polynomial_fit → parse_fit (points + optional template)
  ├── triangle(...) → parse_triangle_call (outermost function; whitespace/comma-separated assignments)
  ├── sd(...) / fac(...) top-level recognition
  ├── parse_system → parse_equation → expression parsing
  │      └── scale guards query calc_mode::is_deep(): Fast errors over limit / Deep allows
  │
  ▼
EvalResult dispatch (main.rs)
  ├── Value ──► evaluate (Evaluator exact-first) ────► display + prefix output
  ├── Factor ─► solver_factor::factor_expr
  ├── Equation ─► filter stored vars → polynomial/Newton root-finding → general-form recognition → output
  ├── Fit ────► solver_fit::solve_polynomial_fit (general form + vertex form)
  ├── Triangle ► solver_triangle::solve_triangle (analytic / numeric fallback)
  └── System ──► linear ? solver_linear (Gaussian elimination)
                    : solver_nonlinear (multi-dimensional Newton)
  │
  ▼
Timing::stop_dynamic()（clear the dynamic timer）→ result covers that line → print 用时：N秒 below
```
