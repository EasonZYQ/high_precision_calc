# HiPerCalc demo (English) — recorded with vhs:
#     vhs docs/demo.en.tape
#     gifsicle -O3 --colors 128 --lossy=80 docs/demo.en.gif -o docs/demo.en.gif
#
# Before recording, move ~/.hipercalc_state away (otherwise the demo inherits this machine's
# persisted settings and is not reproducible); restore it afterwards.
Output docs/demo.en.gif

Set Shell "bash"
Set FontSize 17
Set FontFamily "Consolas"
Set Width 1100
Set Height 600
Set Padding 0
Set Margin 0
Set Theme "Dracula"
Set TypingSpeed 55ms
Set CursorBlink false

# Switch to the directory holding the hipercalc binary and simplify the prompt to "> ".
# All of this is hidden: sleep first so the shell is ready (vhs drops the first few keystrokes
# otherwise), and clear the screen with raw ANSI (this machine's terminfo has no
# xterm-256color, so `clear` fails with "terminals database is inaccessible").
Hide
Sleep 800ms
Type "export PS1='> '"
Enter
Type "cd /e/setup/Java/Rust/high_precision_calc/target-t/demo"
Enter
Type "printf '\033[H\033[2J'"
Enter
Sleep 600ms
Show

Type "./hipercalc --lang en"
Enter
Sleep 900ms

Type "1/3+1/6"
Enter
Sleep 700ms

# Tab completion + the in-bracket argument hint
Type "sin"
Sleep 400ms
Tab
Sleep 900ms
Type "pi/6)"
Enter
Sleep 700ms

Type "(x-2)(x+3)+2x=12"
Enter
Sleep 700ms

# Equation: all roots, real and complex
Type "x^6-1=0"
Enter
Sleep 900ms

# Trigonometric equation: general (periodic) solution
Type "tan(x)=1"
Enter
Sleep 700ms

# Dimensioned unit arithmetic
Type "3 km / 2 second"
Enter
Sleep 700ms

# Inequality
Type "x^4-5x^2+4>0"
Enter
Sleep 700ms

# Closed-form summation
Type "sum(k,k,1,1000000)"
Enter
Sleep 700ms

# Triangle (any letters)
Type "triangle(x=3 y=4 z=5)"
Enter
Sleep 1200ms
