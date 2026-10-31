# HiPerCalc — a high-precision calculator (CLI + GUI)

A calculator with two front ends (CLI `hipercalc` + GUI `hipercalc-gui`): **80-digit internal precision**,
**exact symbolic answers whenever they exist**, and a UI in Simplified Chinese, Traditional Chinese or English.

This document is the **feature manual** — what it can compute, how to type it, and what you get back.
No implementation details.

---

### How to read this

| Your goal | Go to |
|---|---|
| New here, want the basic rules | **Sections 1 – 6** (a 20-minute read) |
| Look up one function | **Sections 7 – 21** (grouped by topic, use as a reference) |
| Change settings / recall commands | **Sections 22 – 25** |
| "Why won't this evaluate?" | **Section 26 · Known limits** |
| The graphical interface | **Section 27** |

> Conventions: `>` marks input, `=` an exact result, `≈` an approximation, `#` starts a comment.

---

## Contents

**Getting started**
1. [Starting and quitting](#1-starting-and-quitting)
2. [Input rules](#2-input-rules)
3. [Exact vs. approximate results](#3-exact-vs-approximate-results)

**Computing**
4. [Basic operations](#4-basic-operations)
5. [Constants and precision](#5-constants-and-precision)
6. [Functions](#6-functions)
7. [Special functions](#7-special-functions)
8. [Number bases and bitwise ops](#8-number-bases-and-bitwise-ops)
9. [Statistics](#9-statistics)
10. [Physical constants](#10-physical-constants)
11. [Units and dimensions](#11-units-and-dimensions)
12. [Complex numbers](#12-complex-numbers)
13. [Matrices and linear algebra](#13-matrices-and-linear-algebra)

**Solving**
14. [Equations](#14-equations)
15. [Systems of equations](#15-systems-of-equations)
16. [Inequalities](#16-inequalities)
17. [Factoring](#17-factoring)
18. [Polynomial fitting](#18-polynomial-fitting)
19. [Triangle solving](#19-triangle-solving)
20. [Calculus](#20-calculus)

**Using and customising**
21. [Variables](#21-variables)
22. [Command reference](#22-command-reference)
23. [Display formats](#23-display-formats)
24. [Language and colours](#24-language-and-colours)
25. [Input experience](#25-input-experience)
26. [Known limits](#26-known-limits)

**GUI**

27. [GUI (hipercalc-gui)](#27-gui-hipercalc-gui)

---

## 1. Starting and quitting

Run `hipercalc` and you are in **interactive mode**.

```text
> 1/3+1/6
= 0.5
```

- Type an expression, press Enter, get the result;
- Lines starting with `/` are **commands** that change settings (full list in section 22);
- `/exit` quits — so does `Ctrl+C` while idle.

### Non-interactive use

For scripts and batch checking:

| Command | What it does |
|---|---|
| `hipercalc -e "1/3+1/6"` | evaluate one expression, print only the result |
| `hipercalc -f script.txt` | run a file line by line |
| `hipercalc --stdin` | read expressions from stdin |
| `hipercalc --lang en` | force the UI language (`zh-CN` / `zh-TW` / `en`) |
| `hipercalc -q` | hide the "time" line |
| `hipercalc -V` | Print the version (`hipercalc 2.1.0`) and exit; does not touch the config file |

> **Timing**: shown to **0.1 s**, truncated. For example `用时：1.9秒`
> (below 0.1 s it prints `用时：<0.1秒`).

**Exit codes**: `0` ok · `1` evaluation error · `2` bad arguments.

---

## 2. Input rules

### Operators

| Form | Meaning |
|---|---|
| `+` `-` `*` `/` | arithmetic |
| `^` | power, e.g. `2^10` |
| `!` | factorial, e.g. `5!` |
| `\|x\|` | absolute value |

### Rules that bite

- **Multiplication may be implied**: `2x`, `3(4+5)`, `xy` all mean multiplication.
  ⚠️ So `xy` is x times y — **not** a variable named `xy`.
- **Variables** are single lowercase letters; **function names** are case-insensitive.
- **No need to leave your IME**: full-width characters (`。` `（` `，` `＝`, full-width digits)
  are converted to half-width automatically.
- **Parsing is from the start**: trailing characters it cannot read produce
  "位置 N 处意外的字符" instead of being silently ignored — so do not append prose to an expression.

---

## 3. Exact vs. approximate results (the one thing to understand)

The symbol in front of the result is the whole story:

| Symbol | Meaning |
|---|---|
| `=` | the result is **exact** (a fraction, a surd, a multiple of π …) |
| `≈` | only an **approximation** is available (shown to the current significant digits) |

Internally everything is kept in exact form and only *rendered* as decimals or symbols:

```text
> 1/3+1/6        (MathIO mode)
= 1 / 2          (LineIO would show 0.5)

> sqrt(8)        (MathIO mode)
= 2*sqrt(2)

> sin(pi/6)
= 0.5
```

> When exactness is possible it never degrades to a decimal; approximations appear only
> when there is no exact form (`sin(1)`, `ln(2)`).

---

## 4. Basic operations

**Integer division and remainder**

```text
> idiv(17, 5)
= 3
> mod(17, 5)
= 2
```

**Combinatorics and number theory**

```text
> nCr(5,2)        # combinations
> nPr(5,2)        # permutations
> gcd(12,18)      # greatest common divisor
> lcm(4,6)        # least common multiple
> isprime(97)     # primality
> nextprime(100)  # next prime
> prevprime(100)  # previous prime (n must be > 2)
```

**Rounding and sign**: `abs(-3)` · `sign(-5)` · `floor(2.7)` · `ceil(2.1)` · `round(2.5)`

> **Size guard**: big numbers, high powers and large factorials are supported, but in the
> default mode an over-large input is **reported** instead of hanging. Use `/mode deep`
> when you really want the brute-force answer (section 22).

---

## 5. Constants and precision

**Built-in constants**: `pi` · `e` · `tau` (2π) · `phi` (golden ratio) · `i` · `ans` (last result)

| Setting | Default | How to change |
|---|---|---|
| Internal precision | 80 digits | `/mode prec 200` |
| Significant digits shown | 20 | `/mode digits 30` |
| Scientific notation | allowed | `/mode sci on` / `off` |
| Thousands separators | off | `/mode group on` / `off` |

- Scientific notation input looks like `2.5e-8`.
- Raising the internal precision makes computation slower — only raise it when you need it.

---

## 6. Functions

**Trigonometry**: `sin` `cos` `tan` `cot` `sec` `csc`
**Inverse trig**: `arcsin` `arccos` `arctan` `arccot`… (also spelled `asin` `acos` `atan` — same thing)

```text
> sin(pi/6)
= 0.5

> /mode deg
> cos(60)
= 0.5
```

- **Radians by default**; `/mode deg` switches to degrees.
- Special angles (`sin(pi/6)`, `tan(pi/4)`) come back **exact**, not as decimals.
- In degree mode write the angle **without `°`**: `cos(60)` works, `cos(60°)` does not.

**Logs and exponentials**: `ln` · `log(base, x)` · `log2` · `log10` · `exp` · `sqrt` · `cbrt` · `nroot(x, n)`

**Hyperbolic**: `sinh` `cosh` `tanh` … and their inverses `asinh` `acosh` `atanh` …

---

## 7. Special functions

| Function | Notes |
|---|---|
| `gamma(x)` | Gamma function. Exact factorial for positive integers; **exact √π form for positive half-integers** (MathIO shows `gamma(1/2)` as `sqrt(pi)`; LineIO shows its decimal value `1.772453850…`) |
| `erf(x)` / `erfc(x)` | Error function and complementary error function |
| `zeta(s)` | Riemann zeta. **Exact closed form at even positive integers** (`zeta(2)` = π²/6); high-precision approximation elsewhere (odd integers included); `s ≤ 1` is not supported yet |
| `cbrt(x)` / `nroot(x, n)` | Cube root / n-th root |

## 8. Number bases and bitwise ops

- **Literals**: `0b1010`, `0o17`, `0xFF`.
- **Bitwise**: `and(a,b)` `or(a,b)` `xor(a,b)` `not(a)` `shl(a,n)` `shr(a,n)`.
- **Display in a base**: `/base hex` (or `dec` / `oct` / `bin`) — affects integer output only. After `/base hex`, `255` prints as `0xff`.
  (`bin`/`hex`/`oct`/`dec` are **not callable functions**; they are values of `/base`.)

## 9. Statistics

Data can be given as a **list** or as an **expression plus a range**:

| Feature | Example |
|---|---|
| Mean | `mean([1,2,3,4])` = `5 / 2`, `mean(k, k, 1, 4)` = `5 / 2` |
| Population variance / stddev | `var(...)`, `stddev(...)` (divide by n) |
| Sample variance / stddev | `var_s(...)`, `stddev_s(...)` (divide by n−1) |
| Median | `median([3,1,2])` = 2 |
| Percentile | `percentile(25, [1,2,3,4,5])` = 2 (linear interpolation) |
| Correlation | `corr([1,2,3],[2,4,6])` = 1 |

Lists use square brackets; in the range form the second argument is the **variable**.

## 10. Physical constants

16 constants (CODATA 2022), each with a **short and a long name**: `C0`/`LIGHT_SPEED`, `G0`/`GRAV_CONST`,
`HPL`/`PLANCK`, `HBAR`/`REDUCED_PLANCK`, `KB`/`BOLTZMANN`, `NA`/`AVOGADRO`, `QE`/`ELEMENTARY_CHARGE`,
`ME`/`ELECTRON_MASS`, `MP`/`PROTON_MASS`, `MN`/`NEUTRON_MASS`, `RGAS`/`GAS_CONST`,
`SIGMA`/`STEFAN_BOLTZMANN`, `ALPHA`/`FINE_STRUCTURE`, `MU0`/`VACUUM_PERMEABILITY`,
`EPS0`/`VACUUM_PERMITTIVITY`, `GACC`/`EARTH_G`.

These names are **reserved for the constants** — `/let C0 = 5` is rejected.

## 11. Units and dimensions

A unit written after a number is **remembered and participates in arithmetic** — real dimensional analysis:

| Input | Result |
|---|---|
| `3 km` | `3000 m` |
| `3 km + 500 meter` | `3500 m` |
| `3 km / 2 second` | `1500 m/s` |
| `2 second * 3 km` | `6000 m*s` |
| `(2 km)^2` | `4000000 m^2` |
| `1 km / 1 mile` | `≈ 0.621371` |
| `90 kph + 10 kph` | `≈ 27.78 m/s` |

**The exponent belongs to the unit**: `3 km^2` means "3 square kilometres" = 3 000 000 m².

**Mismatched dimensions are rejected**: `3 km + 2 second`, `3 km + 2` and `sin(3 km)` all give clear errors.

Available units: metres (`m`/`meter`/`metre`), km, cm, mm, µm (`um`/`µm`/`μm`), nm, inch, foot (`foot`/`feet`), yard, mile, nautical mile; seconds (`s`/`second`/`sec`),
ms, µs (`us`/`µs`/`μs`), ns, minute (`minute`/`min`), hour (`hour`/`h`), day, week; gram, kg, mg, tonne (`tonne`/`t`), pound, ounce; litre (`liter`/`litre`/`l`), ml, hectare, acre;
`kph`, `mph`, knot; calorie, eV; bar, atm; plus the single-letter SI derived units `a` (ampere), `k` (kelvin), `n` (newton), `j` (joule),
`w` (watt), `c` (coulomb), `v` (volt), `pa` (pascal), `hz` (hertz). **Matching is case-insensitive** (`KM` = `Km` = `km`; both `µ` and `μ` are accepted).

Units are only recognised **after a number**, so `3 m` is 3 metres while a bare `m` stays a variable.

## 12. Complex numbers

Use `i` directly: `(2+3i)*(1-i)` = `5 + i`, `i^2` = `-1`, `sqrt(-4)` = `2i`.

- **Component access**: `re(...)`, `im(...)`, `conj(...)`, `arg(...)`, `abs(...)`.
- **Any power works** (principal value): `i^i` ≈ 0.2078795764, `2^i`, `i^(1/2)` ≈ 0.7071067812 + 0.7071067812i (i.e. (1+i)/√2).
- Square roots and logs of negative reals move into the complex domain automatically.
- Integer powers stay **exact** (`(1+i)^2` = `2i`).

## 13. Matrices and linear algebra

Matrices use square brackets and behave as **first-class values** — they can be stored in variables and in `ans`.

`[[1,2],[3,4]]` (construct), `det(...)`, `trace(...)`, `rank(...)`, `transpose(...)`, `inv(...)`,
`A*B` (matrix product), `A*2` (scaling), `A+B` (same shape), `A^2` (integer power, `A^0` = identity),
`linsolve(A, b)` (solve `Ax=b`), `eigen(...)` (eigenvalues, 2×2 and 3×3, complex included).

Elements may be exact or complex, so integer matrices give exact determinants.

## 14. Equations

Type the equation and it is solved: `x^2-4=0` → `x = 2, x = -2`.

- **Exact first**; approximate roots otherwise.
- **All roots** are given for higher-degree equations (including complex ones).
- **Rational equations** are cleared of denominators first, so `(x-3)/(x-5)=x` gives **both** roots.
- Infinitely many solutions are shown in general form; contradictory ones report "no solution".

## 15. Systems of equations

Separate equations with commas: `x+y=5, 2x-y=1`. Linear systems are solved by elimination, non-linear ones numerically.

## 16. Inequalities

Inequality signs: `<` `>` `<=` `>=` `=<` `=>` `!=` (also `≤` `≥` `≠`).

| Input | Result |
|---|---|
| `x^2>4` | `x ∈ (-∞, -2) ∪ (2, ∞)` |
| `x^2<=4` | `x ∈ [-2, 2]` |
| `x^2!=4` | `x ∈ (-∞, -2) ∪ (-2, 2) ∪ (2, ∞)` |
| `x>=1, x=<3` | `x ∈ [1, 3]` (system, intersected) |
| `x>1, x<3, x!=2` | `x ∈ (1, 2) ∪ (2, 3)` |
| `x^2+1>0` | `x ∈ (-∞, ∞)` |

Solutions are printed as intervals; systems use commas and yield the intersection. **Polynomial** inequalities only —
anything else reports an error rather than a wrong answer.

## 17. Factoring

- **Polynomial factoring**: `factor(x^2-1)` (alias `fac`).
- **Prime factorisation**: `primefac(360)` → `360 = 2^3 * 3^2 * 5`; negatives too.

## 18. Polynomial fitting

Give coordinates (two points → a line, three → a parabola, …) and the interpolating polynomial is returned.
A point marked with `P` is treated as a **vertex**; templates with unknown parameters are supported and reveal the
parameter relations when under-determined.

## 19. Triangle solving

`triangle(a=3 b=4 c=5)` (or comma separated) returns everything else: three sides, three angles, three altitudes,
**three medians**, **three angle bisectors**, area, perimeter, circumradius and inradius.

**Notation**: a lowercase letter is a side, an uppercase letter is an angle, `h` plus the letter is the altitude on
that side (`hA` = altitude on side `a`), `m` plus the letter is the median from that vertex (`mA`), and `t` plus the
letter is the internal angle bisector from that vertex (`tA`). `a` and `A` denote the same vertex, so `triangle(a=3 b=4 c=5)`,
`triangle(A=pi/6 b=5 C=pi/3)` and `triangle(hA=4 a=3 b=4)` are all valid. Angles follow the current `/mode deg|rad`:
radians by default, so write radian values — switch with `/mode deg` if you want to write `30` for 30° (`triangle(A=30 b=5 C=60)` reports "angle must be in (0, 180°)" under the default radian mode).

**The letters are not limited to a/b/c**: `triangle(x=3 y=4 z=5)` is exactly equivalent to `triangle(a=3 b=4 c=5)`, and
the output labels follow your letters (`X ≈ 36.87…`, `hZ = 12/5`). Greek letters work too, e.g. `triangle(α=3 β=4 γ=5)`.

- Vertices are numbered in order of first appearance, so the output order matches your input order.
- If only two letters appear, the third vertex is filled in: `triangle(x=1 y=2)` uses `z`, `triangle(p=1 q=2)` uses `r`
  (the fill-in only decides the **label**; two sides are still not enough, so you get "not enough information" until you add a third condition).
- At most three distinct letters; a fourth is an error.

SSS / SAS / ASA / AAS / SSA are supported, analytically where possible; SSA may have two solutions and both are
reported. The angle unit follows the current `/mode deg|rad`.

## 20. Calculus

`diff(f, x)` (higher orders: `diff(f, x, 2)` — the third argument is the order, `0` gives the expression back), `lim(f, x, a)`, `int(f, x)` / `int(f, x, a, b)`,
`taylor(f, x, a, n)`, `sum(f, k, a, b)`, `prod(f, k, a, b)`.

These expand into ordinary expressions, so they compose freely (`diff(x^2,x)+1`); symbolic results are displayed as-is.

## 21. Variables and custom functions

**Custom functions** — `f(x) = x^2 + 1` defines a function; afterwards `f(3)` is `10`.
The number of parameters is **unlimited**: `g(x, y, z) = x + y*z`.

Three rules (no hidden cases):

1. **The name is a single lowercase letter** (`f`, `g`, `h`…). Every built-in is two letters or
   longer (`sin`/`abs`/`gcd`), so a single letter can never collide with one.
2. **Parameters must be bare variables** — `f(x)` or `g(x, y)`; `f(2) = …` is not a definition.
3. **It only counts as a definition while the name is still undefined.** Once defined,
   `f(x) = 0` goes back to **solving an equation** — "first time defines, later solves".

| Command | Effect |
|---|---|
| `/fn` | list custom functions |
| `/del f` | delete function `f` (lowercase; uppercase still deletes a variable) |
| `/reset` | clear settings, variables and functions |

Recursion is allowed but capped at **64 levels** (`g(x) = g(x) + 1` reports "too deep").
Definitions are persisted as `fn:f(x) = x^2 + 1` and replayed on startup.

### Variable storage

`/let A = 3.5` stores a variable (**names must be uppercase**); variables **persist across restarts** and may hold
numbers, complex values, matrices or dimensioned quantities. `ans` holds the previous result.

## 22. Command reference

Every command starts with `/`; type `/help` for the full list. The three groups you will actually use:

### Calculation mode

| Command | Effect |
|---|---|
| `/mode fast` | **default** — over-large input is reported, not brute-forced |
| `/mode deep` | brute force: no size limit, full precision output |
| `/mode prec N` | internal precision (default 80 digits) |
| `/mode digits N` | significant digits shown (default 20) |
| `/mode deg` / `/mode rad` | degrees / radians |
| `/mode mathio` / `/mode lineio` | symbolic / decimal display |
| `/mode latex` | emit LaTeX source as the result |
| `/mode sci on\|off` | allow scientific notation |
| `/mode group on\|off` | thousands separators in integers |

### Display

| Command | Effect |
|---|---|
| `/base dec\|hex\|oct\|bin` | show integer results in that base |
| `/unit km` · `/unit off` | render **dimensionless** results in a given unit |
| `/set category colour` | recolour output categories |

### Session

| Command | Effect |
|---|---|
| `/lang zh-CN\|zh-TW\|en` | switch the UI language |
| `/let NAME = value` | store a variable (`NAME` must be uppercase) |
| `/var` · `/del NAME` | list / delete variables |

---

## 23. Display formats

| Mode | Looks like |
|---|---|
| **MathIO** | symbolic: `1 / 2`, `(1 / 2)*sqrt(2)`, `pi` |
| **LineIO** (default) | always decimal, to the configured digits |
| **LaTeX** | source you can paste into a paper |

- `/base` affects only how **integers are displayed**, not their value.
- `/unit` affects only **dimensionless** results; quantities with dimensions carry their own unit.

> The `= 1 / 2` and `= 2*sqrt(2)` examples in section 3 require `/mode mathio`.
> Under the default LineIO they are decimals (`1/3 + 1/6` shows `= 0.5`).

These switches change **how results look**, never what is computed.

---

## 24. Language and colours

- **Three languages**: Simplified Chinese · Traditional Chinese · English. The first launch
  follows the system language; `/lang` switches afterwards.
- **Colours**: `/set` recolours results, errors, hints, variables — the choice is remembered.
- Settings live in `~/.hipercalc_state`, **shared by the CLI and the GUI**.

---

## 25. Input experience

| Feature | Behaviour |
|---|---|
| **Tab completion** | completes functions, variables and `/` commands; completing a function inserts its parentheses and puts the caret inside |
| **Bracket pairing** | typing `(` inserts `)`; typing `)` **skips over** an existing one instead of doubling it |
| **Bracket matching** | the bracket next to the caret is highlighted with its partner; unmatched ones turn red |
| **Argument hints** | with the caret inside a function's parentheses, the hint area shows its parameter list |
| **Full-width to half-width** | Chinese punctuation and full-width digits are converted automatically |

**Keys**

- `Ctrl+L` clears the screen, `Ctrl+R` searches history backwards;
- `Ctrl+C` **quits while idle** and **interrupts the running computation** while busy.

---

## 26. Known limits

Honest list of what is not supported yet:

| Area | Limit |
|---|---|
| Dimensions | results use SI base units; compound forms like "km²" or "km/h" are not built automatically, and prefixes cannot be split (`km`, `mm` work; `k` + `m` does not) |
| Inequalities | polynomial only |
| Matrices | eigenvalues for 2×2 and 3×3; no matrix division (use `inv(A)*B`) |
| Statistics | data points must be evaluable at parse time (`/let` variables are fine) |
| Special functions | `gamma` currently accepts positive integers and positive half-integers |
| Interruption | supported on Windows, Linux and macOS, only during a computation |

---

## 27. GUI (hipercalc-gui)

The CLI syntax works **unchanged** — the GUI is a shell around the same evaluation path
(`Session::eval`), so every function, command and display switch above applies there too.

### 27.1 Layout

| Area | Contents |
|---|---|
| **Title bar (custom-drawn)** | app name + subtitle; minimize / maximize-restore / close on the right. Drag anywhere on it to move the window, **double-click** to toggle maximize |
| Toolbar | mode (`MathIO` / `LineIO`), angle (`DEG` / `RAD`), calc mode (fast / deep), `>_` (open the CLI), `⚙` settings, clear history |
| Input | a **structured** editor (fractions / roots / powers / Σ are real layout nodes) plus `C` and `=` |
| Results | real math typesetting: stacked fractions, root overlines, superscripts, Σ limits |
| Keypad | Desmos-style panels: main / functions / units / constants |
| Status bar | current keypad panel and evaluation time |

The window has **no system title bar** (`decorations: false`) — a Win32 grey strip does not
belong in this glass UI, so the title bar is drawn by the app
(`crates/hipercalc-gui/src/chrome.rs`):

- **Move**: drag anywhere on the title bar; snapping to screen edges still works (the OS move loop);
- **Resize**: move the pointer to any edge or corner (the cursor turns into ↔ / ↖ …) and drag;
- **Double-click** the title bar to maximize / restore; resize handles are hidden while maximized;
- Maximizing via `Win+↑` / `Win+←` or double-click updates the button icon automatically.

### 27.2 Keys

| Key | Effect |
|---|---|
| `Enter` | evaluate |
| `C` | clear the input |
| `←` `→` | move the caret in **linear reading order** (every slot of a fraction / root / Σ is a legal position) |
| `Backspace` | delete the item before the caret; at the start of an empty slot it does **nothing** |
| `(` `)` | `(` inserts a group, `)` leaves the current group |

### 27.3 Filling in big operators

- Inserting `∑` / `∏` pre-fills the lower-left **variable** box with `n`, and the caret lands in the
  **lower-right box (the lower bound)**;
- `←` `→` follow the **visual order**:
  - Σ / ∏: **lower-left (variable) → lower-right (lower bound) → upper (upper bound) → body**
  - `∫`: **lower bound → upper bound → body → the variable after `d`**
- With the caret before the whole symbol, pressing `→` walks through it in that order and ends after it.

### 27.4 Settings

| Setting | Meaning |
|---|---|
| Theme | dark / light |
| Texture | **acrylic** (system composited) / **glass** (our own liquid glass) / **off** (no capture, no blur) |
| Desktop blur | blur radius in glass mode (0 = pixel-sharp, continuous slider) |
| Panel shadow | shadow depth of glass panels |
| Precision | the same setting as the CLI `/mode` |
| Math font | one of five bundled math fonts |
| Language | Simplified Chinese / Traditional Chinese / English |
| Render scale | 100% / 90% / 80% / 70% |
| Allow screenshots | allow: captures work **and** the backdrop still never shows this window (that rect is inpainted from its four edges); disallow: clean backdrop but captures miss the window |
| Long results | wrap / horizontal scroll |

### 27.4.1 The CLI inside the GUI

- The **`>_`** button opens a **separate command-line window** (a real process, not a thread).
  **Closing the GUI does not close it** — they are independent, and clicking again opens another.
  The window is hosted by a shell, so it stays open after the program exits.
- It is the same `hipercalc::run()` — the GUI executable simply acts as the CLI, **no extra file needed**.
- The reverse also holds: **with any argument the GUI executable runs as the CLI** and shows no window:

  ```bash
  hipercalc-gui.exe -e "1/3+1/6"     # evaluate and exit
  hipercalc-gui.exe --stdin < in.txt # batch
  hipercalc-gui.exe -f script.txt    # run a script
  ```

  On Windows it attaches to or opens a console; with `> out.txt` redirection your standard
  output is left untouched.

### 27.5 While computing

- The results area shows **"calculating 12.3 s"**, ticking every 0.1 s;
- A **cancel** button sits next to it, and the input bar's `=` turns into **cancel** while busy;
- The keypad's `↵` is **greyed out** while busy;
- Cancelling is a *request*: the solver exits at its next checkpoint and leaves a
  "computation interrupted" line. A few loops without checkpoints may take a moment longer.

### 27.6 Relation to the CLI

- Both share `~/.hipercalc_state`: language, precision and display mode are common.
- Evaluation runs on a **background thread**, so heavy work never freezes the UI.
- The glass texture needs screen capture: DXGI desktop duplication on Windows; other platforms fall back to "off".

---

*This manual covers features and usage only; for implementation details, read the source and its comments.*
