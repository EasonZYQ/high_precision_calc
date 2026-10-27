//! 结果区（历史列表 / 搜索 / 复制）。
//!
//! 从 `app.rs` 拆出来的视图层：**只读 `State`，不发消息以外的事**。
//! 业务逻辑仍在 `mod.rs` 的 `update` 里 —— 拆分的目的是让 4600 行的单文件
//! 变成"按区域可定位"的几个文件，而不是把逻辑打散。

use iced::widget::{column, row};

use super::*;

impl State {
    pub(super) fn results(&self) -> Element<'_, Message> {
        let math = self.modes.display == DisplayMode::MathIO;

        // **搜索过滤**（用户 2026-10-27）：子串、大小写不敏感，原式与结果都匹配。
        // 保留**真实下标**（`shown` 存的是 history 的下标）：插入/复制都按它取条目，
        // 否则过滤后点"插入"会插到别的式子上。
        let q = self.search.trim().to_lowercase();
        let shown: Vec<usize> = self
            .history
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                q.is_empty()
                    || e.input.to_lowercase().contains(&q)
                    || e.output.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();

        let mut col = Column::new().spacing(14).width(Length::Fill);
        if self.history.is_empty() {
            col = col.push(
                container(
                    // ⚠️ 占位提示**不能说**"按 Enter 求值" —— Enter 现在是换行。
                    // 之前那句是错的，用户按 Enter 只会换行、然后以为程序坏了。
                    theme::label(t("输入表达式，按 = 或键盘右下角的 ↵ 求值"))
                        .size(15)
                        .style(theme::hint),
                )
                .padding([26, 8])
                .center_x(Length::Fill),
            );
        } else if shown.is_empty() {
            // 有历史但没匹配上：说清楚"是过滤掉了"，否则用户会以为历史丢了
            col = col.push(
                container(
                    theme::label(t("没有匹配的记录"))
                        .size(14)
                        .style(theme::hint),
                )
                .padding([26, 8])
                .center_x(Length::Fill),
            );
        } else {
            for i in shown {
                col = col.push(self.history_entry(i, &self.history[i], math));
            }
        }

        // 「计算中…」提示（用户要求："让程序始终响应"）。
        //
        // 它不只是装饰：大运算要跑几十秒，用户需要**看得见**"还在算"，否则会以为
        // 程序又卡死了 —— 而那正是本次要修掉的观感。放在历史最下面（也就是
        // 视线落点，因为 `anchor_bottom` 会把视图钉在底部）。
        if self.busy {
            // 用户要求（2026-10-24）：「计算中要显示原式」——
            // 大算跑几十秒时，"我在算哪一条"比"还在算"更重要（尤其连按几次之后）。
            if !self.last_input.is_empty() {
                col = col.push(
                    container(
                        Row::new()
                            .align_y(Alignment::Center)
                            .spacing(6)
                            .push(theme::label(t("正在计算：")).size(13).style(theme::hint))
                            .push(theme::label(self.last_input.clone()).size(15)),
                    )
                    .width(Length::Fill)
                    .padding([4, 9]),
                );
            }
            // 用户要求（2026-10-23）：「不要只写一个计算中，要计时，以秒为单位；
            // 计算栏旁边要有一个取消按钮」。
            let secs = self
                .busy_since
                .map(|t| t.elapsed().as_secs_f32())
                .unwrap_or(0.0);
            let cancel = button(text(t("取消")).size(12).font(crate::fonts::font_cjk()))
                .padding([3, 10])
                .style(|t, s| theme::key(t, KeyKind::Control, s))
                .on_press(Message::Cancel);
            col = col.push(
                container(
                    Row::new()
                        .align_y(Alignment::Center)
                        .spacing(8)
                        .push(
                            theme::label(t_fmt("计算中 {0} 秒", &[&format!("{secs:.1}")]))
                                .size(13)
                                .style(theme::hint),
                        )
                        .push(cancel),
                )
                .width(Length::Fill)
                .padding([4, 9]),
            );
        }

        // `anchor_bottom`：历史是"越往下越新"，滚到底部才符合计算器的直觉。
        // 内容超出可视区时首条会被滚出视野 —— 这是**正常行为**（不是裁切 bug），
        // 加内边距只是为了首条不与面板边框贴死。
        //
        // 滚动方向跟着"长结果"设置走（用户要求："增加左右拖动或者换行，供用户选择"）：
        // - 自动换行：只开垂直滚动。这一档要求内容折行，而折行由 `mathrender`
        //   的 `Wrapping::Glyph` 负责 —— 光开垂直滚动是不够的（`Wrapping::Word`
        //   不会在 100 位整数中间断行，照样横向溢出）。
        // - 横向滚动：两个方向都开。内容的宽度约束变成"无限"，于是不会折行，
        //   超出的部分靠拖动看。
        let direction = if self.wrap {
            scrollable::Direction::Vertical(scrollable::Scrollbar::default())
        } else {
            scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            }
        };
        let inner = scrollable(col.padding([10, 10]))
            .id(HISTORY_ID)
            .anchor_bottom()
            .height(Length::Fill)
            .direction(direction)
            // 记住"用户是不是在底部"（见 `State::at_bottom`）
            .on_scroll(Message::HistoryScrolled);

        // 搜索入口（用户 2026-10-27 定：默认收起，点按钮或 Ctrl+F 展开）
        // **搜索栏常驻一行**（用户 2026-10-27："反正也没多大"）。原来要 Ctrl+F 展开，
        // 实际用起来多一步；现在直接给输入框，右边一个"×"清空。
        //
        // ⚠️ `text_input` 的占位符要求 `&'a str`（与 Element 同生命周期），而 `t()`
        // 返回 `String` ⇒ 临时值活不到渲染，所以用**独立标签**代替占位符。
        let search_row: Row<'_, Message> = row![
            theme::label(t("搜索")).size(12).style(theme::hint),
            text_input("", &self.search)
                .on_input(Message::SearchChanged)
                .width(Length::Fill)
                .size(12)
                .padding([4, 8])
                .font(crate::fonts::font_cjk())
                .style(theme::field),
            button(theme::label(t("清空")).size(11))
                .padding([2, 8])
                .style(|t, s| theme::mode_chip(t, s, false))
                .on_press(Message::SearchChanged(String::new())),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        Self::glass_under(
            container(column![search_row, inner].spacing(6))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(6)
                .style(theme::panel_card)
                .into(),
            theme::panel_glass(),
        )
    }

    /// 一条历史：上面是表达式回显，下面是（可排版的）结果
    pub(super) fn history_entry<'a>(
        &'a self,
        idx: usize,
        e: &'a Entry,
        math: bool,
    ) -> Element<'a, Message> {
        // 回显也走 `mathrender`：用户敲的 `sqrt(2)/3` 在回显里同样应该显示成
        // 根号 + 分数，而不是一行原始 ASCII —— 与输入框里看到的样子保持一致。
        //
        // ⚠️ **回显永远按数学排版，与显示模式无关**（用户要求："根号应当始终渲染"）。
        // 曾经的 bug：这里传的是 `math`，于是切到 LineIO 后回显也退化成纯文本，
        // `sqrt(2)/3` 原样显示成 ASCII —— 但**回显是用户自己敲的表达式**，
        // 它天然就是数学式，跟"结果用十进制还是分数显示"是两码事。
        // 显示模式只该影响**结果**的呈现（MathIO 排版 / LineIO 小数）。
        //
        // ⚠️ **错误行也要排版**：回显的是**用户敲的表达式**，它天然是数学式。
        // 以前错误行退回纯文本，于是 `∑` 结构的输入在历史里显示成
        // `sum(k^2,k,1,)`（用户实测："错误提示还是输出的 sum 函数，没有渲染结构"）。
        // 报错文案本身仍然是散文（见下面 `result` 那一段的说明），两者互不影响。
        let echo_text: Element<'a, Message> = mathrender::render(
            &e.input,
            true,
            mathrender::Style2 {
                size: 15.0,
                color: Some(theme::colors().text_dim),
                wrap: self.wrap,
                // 回显与输入栏是同一串式子，函数名的斜体规则也保持一致
                italic_names: true,
            },
            // 历史是只读的：占位框不可点
            None,
        );

        // 插入键（用户 2026-10-24）：把这一条的**原式**送回输入栏光标前。
        // 放在回显行最右侧、用低调的 chip 样式 —— 它是"顺手再来一次"的辅助动作，
        // 不该跟结果抢注意力。
        let insert = button(theme::label(t("插入")).size(11))
            .padding([2, 8])
            .style(|t, s| theme::mode_chip(t, s, false))
            .on_press(Message::InsertHistory(idx));

        let mut echo = row![
            text("›")
                .size(15)
                .style(theme::accent_text)
                .font(theme::font_math()),
            echo_text,
            Space::new().width(Length::Fill).height(Length::Shrink),
            insert,
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // **复制键常驻**（用户 2026-10-27 改）：之前是"悬停才出现"，但那样鼠标一进
        // 就会**挤出一个按钮**，右边的"插入"跟着跳位 —— 用户原话"鼠标一放上去就又
        // 弹出一个复制"。常驻两个 chip、位置固定，不再抖。
        //
        // 文案叫「复制结果」而不是「复制」：旁边还有个「插入」，光写"复制"
        // 分不清复制的是原式还是结果。
        echo = echo.push(
            button(theme::label(t("复制结果")).size(11))
                .padding([2, 8])
                .style(|t, s| theme::mode_chip(t, s, false))
                .on_press(Message::CopyEntry(idx)),
        );

        // 结果文本带 core 给的 `= ` / `≈ ` 前缀。**刻意保留**：
        // 它是区分"精确值"与"近似值"的唯一信号，比只看颜色可靠（色盲用户也能分辨）。
        // 长结果换行时前缀会被留在首行，观感也正常。
        let precise_for_style = e.precise && !e.error;
        // ⚠️ **错误信息不能走 `mathrender`**。
        //
        // 错误信息是**散文**（`错误: 位置 2 处意外的字符: ' '`），不是数学式。
        // `mathrender` 的 tokenizer 会**把空格整个吃掉**（见其 `' ' => i += 1`），
        // 于是上面那句会被排成 `错误:位置2处意外的字符:''` —— 用户在原快照里
        // 看到的 `错误:位置2处意外的字符:"` 就是这么来的（末尾那个引号是两个
        // 单引号中间的空格被吃掉后挤在一起的观感）。错误信息必须原样显示。
        //
        // 所以：**报错时退回纯文本**（`math = false`），与 LineIO 走同一条路。
        let result: Element<'a, Message> = mathrender::render(
            &e.output,
            math && !e.error && !e.notice,
            mathrender::Style2 {
                // 提示行（「计算已中断」）用小字号 + hint 色：它是**用户的动作回执**，
                // 不是失败，别用错误的红与大字号去刺激人（用户 2026-10-24 要求）。
                size: if e.error {
                    18.0
                } else if e.notice {
                    15.0
                } else {
                    24.0
                },
                color: Some(if e.error {
                    theme::colors().danger
                } else if e.notice {
                    theme::colors().text_dim
                } else if precise_for_style {
                    theme::colors().success
                } else {
                    theme::colors().text
                }),
                // ⚠️ **统一用数学字体**（用户要求：LineIO 与 MathIO 字体要一致）。
                // 之前 LineIO 走等宽字体（`!math`），观感上与 MathIO 判若两套；
                // 等宽的好处只是"小数位对齐"，代价是整块结果区看起来像另一款软件。
                wrap: self.wrap,
                // 结果是**程序产出**（`= 3`、`sin(x) = 0.5`），函数名按排版惯例直立；
                // 斜体只用于输入栏/回显/函数面板（用户 2026-10-24 的要求范围）。
                italic_names: false,
            },
            // 历史是只读的：结果里的占位框不可点
            None,
        );

        // ── 耗时贴结果行**最右侧**（用户要求："计时放在运算结果的最右侧"）──
        //
        // 用 `Space(Length::Fill)` 把结果推到左边、耗时压到右边 ——
        // 这是 iced 里唯一的"两端对齐"写法（没有 `justify_content: space-between`）。
        //
        // ⚠️ 结果元素必须能伸能缩：`mathrender` 的根是 `Row`，宽度默认
        // `Length::Shrink`，所以它只占自己需要的宽度，`Space` 吃掉剩下的全部余量，
        // 耗时自然落到最右端。若结果被撑成 `Fill`，`Space` 就没有余量可分，
        // 耗时会紧贴在结果后面而不是靠右。
        let timing = elapsed_text(e.elapsed);
        let result_line = row![
            result,
            Space::new().width(Length::Fill).height(Length::Shrink),
            theme::label(timing).size(12).style(theme::hint),
        ]
        .align_y(Alignment::Center);

        // 复制键常驻后不再需要悬停跟踪（`mouse_area` 那层已去掉）：
        // 少一次"进/出各重绘一帧"，也少一个"按钮突然冒出来"的抖动源。
        container(column![echo, result_line].spacing(5))
            .width(Length::Fill)
            .padding([7, 9])
            .style(move |t| theme::history_row(t, e.error))
            .into()
    }
}
