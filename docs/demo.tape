# HiPerCalc 演示（简体中文）—— 用 vhs 录制：
#     vhs docs/demo.tape
#     gifsicle -O3 --colors 128 --lossy=80 docs/demo.gif -o docs/demo.gif
#
# 注意：录制前把 ~/.hipercalc_state 移走（否则会继承本机持久化状态 ⇒ 结果不可复现），录完还原。
Output docs/demo.gif

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

# 把工作目录切到放好 hipercalc 的地方，并把提示符简化成 "> "。
# 这些准备动作整段隐藏：先 Sleep 让 shell 就绪（否则 vhs 会吃掉开头几个字符），
# 收尾用 printf 打 ANSI 清屏（本机 terminfo 里没有 xterm-256color，`clear` 会报
# "terminals database is inaccessible"）。
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

Type "./hipercalc --lang zh-CN"
Enter
Sleep 900ms

Type "1/3+1/6"
Enter
Sleep 700ms

# Tab 补全 + 括号内的参数提示
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

# 方程：全部根（含复根）
Type "x^6-1=0"
Enter
Sleep 900ms

# 三角方程给周期通式
Type "tan(x)=1"
Enter
Sleep 700ms

# 带量纲的单位运算
Type "3 km / 2 second"
Enter
Sleep 700ms

# 不等式
Type "x^4-5x^2+4>0"
Enter
Sleep 700ms

# 闭式求和
Type "sum(k,k,1,1000000)"
Enter
Sleep 700ms

# 三角形（字母任意）
Type "triangle(x=3 y=4 z=5)"
Enter
Sleep 1200ms
