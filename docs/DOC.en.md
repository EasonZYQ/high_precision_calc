# HiPerCalc — a high-precision command-line calculator

A calculator for the terminal: **80-digit internal precision**, **exact symbolic answers whenever they exist**, a Chinese
interface, and three UI languages. This document is the **feature manual** — what it can compute, how to type it, and
what you get back. No implementation details.

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

---

## 1. Starting and quitting

Run the program for an interactive REPL: type an expression, press Enter, read the result. Commands start with `/`;
quit with `/exit`.

Non-interactive use:

| Usage | Meaning |
|---|---|
| `hipercalc -e "expr"` | Evaluate one expression, print only the result |
| `hipercalc -f script` | Evaluate a file line by line |
| `hipercalc --stdin` | Read from standard input |
| `hipercalc --lang zh-TW` | Force the UI language (Simplified / Traditional Chinese, English) |
| `hipercalc -q` | Suppress the timing line |

Exit codes: 0 normal, 1 evaluation error, 2 bad arguments.

## 2. Input rules

- **Operators**: `+` `-` `*` `/` `^` (power), `!` (factorial), `|x|` (absolute value).
- **Implicit multiplication**: `2x`, `3(4+5)`, `xy` all mean multiplication (`xy` is x times y).
- **Functions**: `sin(x)`, `sqrt(2)`, `log(2, 8)` (the first argument is the base).
- **Case**: function names are case-insensitive; **variables** are single lowercase letters.
- Full-width characters are converted automatically; trailing Chinese text is ignored.

## 3. Exact vs. approximate results

This is the point of the program:

- **`=`** marks an **exact** result: `1/3 + 1/6` = `= 1 / 2`, `sqrt(8)` = `= 2*sqrt(2)`, `sin(pi/6)` = `= 1 / 2`.
- **`≈`** marks an **approximate** result (`sin(1)`, `ln(2)`), shown with the configured significant digits.

If an exact form exists, it is never replaced by a decimal.

## 4. Basic operations

Beyond the four operations, powers and roots:

- **Integer division and modulo**: `idiv(17, 5)` = 3, `mod(17, 5)` = 2.
- **Combinatorics and number theory**: `nCr(5,2)`, `nPr(5,2)`, `gcd(12,18)`, `lcm(4,6)`, `isprime(97)`, `nextprime(100)`, `prevprime(100)`.
- **Rounding and sign**: `abs(-3)`, `sign(-5)`, `floor(2.7)`, `ceil(2.1)`, `round(2.5)`.

Large numbers, high powers and large factorials are supported, with **size guards**: in normal mode an oversized input
gives a clear message instead of hanging. Use `/mode deep` when you really want it to grind through.

## 5. Constants and precision

Built-in constants: `pi`, `e`, `tau` (2π), `phi` (golden ratio), `i` (imaginary unit), `ans` (last result).

- **Internal precision**: 80 digits by default; raise it with `/mode prec 200`.
- **Display digits**: 20 significant digits by default; `/mode digits 30`.
- Scientific notation is written `2.5e-8`; toggle it with `/mode sci on|off`.
- Thousands separators: `/mode group on|off` (off by default).

## 6. Functions

- **Trigonometry**: `sin` `cos` `tan` `cot` `sec` `csc`; inverse: `arcsin` `arccos` `arctan` `arccot` … (`asin`/`acos`/`atan` also work)
- **Angle unit**: `/mode deg` or `/mode rad`.
- **Exact values at special angles**: `sin(pi/6)`, `cos(60°)`, `tan(pi/4)` return exact results, not decimals.
- **Logs and exponentials**: `ln`, `log(base, x)`, `log2`, `log10`, `exp`, `sqrt`, `cbrt`, `nroot(x, n)`.
- **Hyperbolic / inverse hyperbolic**: `sinh` `cosh` `tanh`, `asinh` `acosh` `atanh` …

## 7. Special functions

| Function | Notes |
|---|---|
| `gamma(x)` | Gamma function. Exact factorial for positive integers; **exact √π form for positive half-integers** (`gamma(1/2)` = `sqrt(pi)`) |
| `erf(x)` / `erfc(x)` | Error function and complementary error function |
| `zeta(s)` | Riemann zeta. Exact at integer points (e.g. `zeta(2)` contains π²), high-precision approximation elsewhere |
| `cbrt(x)` / `nroot(x, n)` | Cube root / n-th root |

## 8. Number bases and bitwise ops

- **Literals**: `0b1010`, `0o17`, `0xFF`.
- **Bitwise**: `and(a,b)` `or(a,b)` `xor(a,b)` `not(a)` `shl(a,n)` `shr(a,n)`.
- **Display in a base**: `/base hex` (or `dec` / `oct` / `bin`) — affects integer output only.
- **Conversions**: `bin(10)`, `hex(255)`.

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

Available units: metres (`m`/`meter`), km, cm, mm, µm, nm, inch, foot, yard, mile, nautical mile; seconds (`s`/`second`),
ms, µs, ns, minute, hour, day, week; gram, kg, mg, tonne, pound, ounce; litre, ml, hectare, acre;
`kph`, `mph`, knot; calorie, eV; bar, atm. **Matching is case-insensitive** (`KM` = `Km` = `km`).

Units are only recognised **after a number**, so `3 m` is 3 metres while a bare `m` stays a variable.

## 12. Complex numbers

Use `i` directly: `(2+3i)*(1-i)` = `5 + i`, `i^2` = `-1`, `sqrt(-4)` = `2i`.

- **Component access**: `re(...)`, `im(...)`, `conj(...)`, `arg(...)`, `abs(...)`.
- **Any power works** (principal value): `i^i` ≈ 0.2078795764, `2^i`, `i^(1/2)` = (1+i)/√2.
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
area, perimeter, circumradius and inradius.

**Notation**: a lowercase letter is a side, an uppercase letter is an angle, and `h` plus the uppercase letter is the
altitude on that side (`hA` is the altitude on side `a`). `a` and `A` denote the same vertex, so `triangle(a=3 b=4 c=5)`,
`triangle(A=30 b=5 C=60)` and `triangle(hA=4 a=3 b=4)` are all valid.

**The letters are not limited to a/b/c**: `triangle(x=3 y=4 z=5)` is exactly equivalent to `triangle(a=3 b=4 c=5)`, and
the output labels follow your letters (`X ≈ 36.87…`, `hZ = 12/5`). Greek letters work too, e.g. `triangle(α=3 β=4 γ=5)`.

- Vertices are numbered in order of first appearance, so the output order matches your input order.
- If only two letters appear, the third vertex is filled in: `triangle(x=1 y=2)` uses `z`, `triangle(p=1 q=2)` uses `r`.
- At most three distinct letters; a fourth is an error.

SSS / SAS / ASA / AAS / SSA are supported, analytically where possible; SSA may have two solutions and both are
reported. The angle unit follows the current `/mode deg|rad`.

## 20. Calculus

`diff(f, x)` (higher orders: `diff(f, x, 2)`), `lim(f, x, a)`, `int(f, x)` / `int(f, x, a, b)`,
`taylor(f, x, a, n)`, `sum(f, k, a, b)`, `prod(f, k, a, b)`.

These expand into ordinary expressions, so they compose freely (`diff(x^2,x)+1`); symbolic results are displayed as-is.

## 21. Variables

`/let A = 3.5` stores a variable (**names must be uppercase**); variables **persist across restarts** and may hold
numbers, complex values, matrices or dimensioned quantities. `ans` holds the previous result.

## 22. Command reference

Commands start with `/`; `/help` lists them all. Highlights:

| Command | Effect |
|---|---|
| `/mode fast` / `/mode deep` | Normal / grind-through mode |
| `/mode prec N` / `/mode digits N` | Internal precision / display digits |
| `/mode sci on\|off` / `group on\|off` | Scientific notation / thousands separators |
| `/mode deg` / `rad` | Angle unit |
| `/mode mathio` / `lineio` / `latex` | Symbolic / decimal / LaTeX output |
| `/base dec\|hex\|oct\|bin` | Integer output base |
| `/unit km` / `/unit off` | Display dimensionless results in a unit |
| `/lang zh-CN\|zh-TW\|en` | Interface language |
| `/set …` | Colours |
| `/let NAME = value` | Store a variable |

## 23. Display formats

MathIO (symbolic, default), LineIO (decimals), LaTeX (`/mode latex`), integer base (`/base`), unit display (`/unit`,
dimensionless results only — dimensioned values carry their own unit). All of these change **presentation only**.

## 24. Language and colours

Interface in Simplified Chinese, Traditional Chinese and English (auto-detected on first run, switchable with `/lang`).
Colours for results, errors, hints and variables are configurable with `/set` and are remembered.

## 25. Input experience

- **Tab completion** for functions, variables and `/` commands; completing a function inserts the parentheses and puts
  the cursor inside.
- **Bracket auto-pairing**: typing `(` inserts `)` and places the cursor inside; typing `)` later **steps over** the
  existing one.
- **Bracket matching highlight**, unpaired brackets flagged in red.
- **Argument hints** while the cursor is inside a function call.
- **Keys**: full-width input auto-converted; `Ctrl+L` clears the screen, `Ctrl+R` searches history;
  `Ctrl+C` quits when idle and **interrupts the running computation** while computing.

## 26. Known limits

- **Dimensions**: results are expressed in SI base units; composite units such as km/h are not synthesised, and
  prefixes are not split (`km` and `mm` work, `k` + `m` does not).
- **Inequalities**: polynomial only.
- **Matrices**: eigenvalues for 2×2 and 3×3; no matrix division (use `inv(A)*B`).
- **Statistics**: data points must be evaluable at parse time (`/let` variables are fine).
- **Special functions**: `gamma` supports positive integers and half-integers.
- **Interruption**: supported on Windows, Linux and macOS, only during a computation.

---

*This manual covers features and usage only; for implementation details, read the source and its comments.*
