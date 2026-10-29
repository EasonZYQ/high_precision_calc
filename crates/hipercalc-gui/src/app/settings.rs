//! 设置面板与函数弹窗。
//!
//! 从 `app.rs` 拆出来的视图层：**只读 `State`，不发消息以外的事**。
//! 业务逻辑仍在 `mod.rs` 的 `update` 里 —— 拆分的目的是让 4600 行的单文件
//! 变成"按区域可定位"的几个文件，而不是把逻辑打散。

use iced::widget::{column, row};

use super::*;

/// 一行的**排版预算**（像素）：窗口 940 − 外层 padding 44 − 卡片 padding 24 ≈ 872，
/// 留一点余量取 860。超了就把后面的小件换到下一行。
///
/// ⚠️ iced **没有 flex-wrap**（`Row` 不会自动折行），所以只能自己按估算宽度装行。
/// 估算用的是"字数 × 12.5 + 余量"—— 对中文 12–13px 的标签足够准，
/// 宁可早换一行，也不要横向溢出被裁掉。
const SETTINGS_ROW_BUDGET: f32 = 860.0;

/// 设置面板的**最大高度**：窗口高 900 − 上下各 56 的留白（见 `view.rs` 的
/// `.padding([56.0, 22.0])`）再留一点余量，超出就由内部 `scrollable` 滚动。
const SETTINGS_MAX_H: f32 = 900.0 - 2.0 * 56.0 - 8.0;

/// 估算一段文字占多宽（12–13px 字号）
fn w_text(s: &str) -> f32 {
    s.chars().count() as f32 * 12.5 + 8.0
}

impl State {
    /// 把一串"宽度已知的小件"按预算装行：装不下就换行。
    ///
    /// 返回 `Column`（可能有多行），可以直接塞进设置面板的纵向序列里。
    fn pack_rows<'a>(
        items: Vec<(f32, Element<'a, Message>)>,
        budget: f32,
        spacing: f32,
    ) -> Column<'a, Message> {
        let mut col = Column::new().spacing(4).width(Length::Fill);
        let mut cur = Row::new().spacing(spacing).align_y(Alignment::Center);
        let mut used = 0.0f32;
        for (w, el) in items {
            if used > 0.0 && used + w > budget {
                col = col.push(cur);
                cur = Row::new().spacing(spacing).align_y(Alignment::Center);
                used = 0.0;
            }
            cur = cur.push(el);
            used += w + spacing;
        }
        col.push(cur)
    }

    pub(super) fn settings_panel(&self) -> Element<'_, Message> {
        // ① 工作精度
        let precision_row = row![
            self.setting_label(t("工作精度"), SETTING_LABEL_W),
            text_input("20 – 2000", &self.precision_text)
                .on_input(Message::PrecisionChanged)
                .on_submit(Message::ApplyPrecision)
                .width(Pixels(96.0))
                .size(13)
                .font(theme::font_math())
                .style(theme::field),
            button(theme::label(t("应用")).size(13))
                .padding([4, 10])
                .style(|t, s| theme::mode_chip(t, s, false))
                .on_press(Message::ApplyPrecision),
            theme::label(t_fmt("当前 {0} 位", &[&self.modes.precision.to_string()]))
                .size(13)
                .style(theme::hint),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // ② 语言（用户要求："设置里添加语言选择"）。
        // 语言名用**各语言自己的写法**（"简体中文" / "繁體中文" / "English"），
        // 而不是翻译成当前语言 —— 语言选单里这样才是惯例：看不懂当前语言的人
        // 也能找到自己的那一项。
        let cur_lang = self.modes.lang;
        let mut lang_row = Row::new().spacing(6).align_y(Alignment::Center);
        lang_row = lang_row.push(self.setting_label(t("语言 / Language"), SETTING_LABEL_W));
        for l in ALL_LANGS.iter() {
            let selected = *l == cur_lang;
            lang_row = lang_row.push(
                button(text(Session::lang_native_name(*l)).size(13))
                    .padding([5, 12])
                    .style(move |t, s| theme::mode_chip(t, s, selected))
                    .on_press(Message::SetLang(*l)),
            );
        }

        // ③ 数学字体（用户要求："在设置里添加这些全部字体，供用户选择"）。
        // 每个按钮**用它自己的字体渲染自己的名字** —— 这样选之前就能看到效果，
        // 比单看名字直观得多（这也是字体选单的常见做法）。
        let cur_font = theme::math_font();
        let mut font_items: Vec<(f32, Element<'_, Message>)> = vec![(
            SETTING_LABEL_W,
            self.setting_label(t("数学字体"), SETTING_LABEL_W),
        )];
        for (i, f) in MathFont::ALL.iter().enumerate() {
            let selected = *f == cur_font;
            font_items.push((
                w_text(f.display_name()) + 24.0,
                button(text(f.display_name()).size(13).font(f.font()))
                    .padding([5, 12])
                    .style(move |t, s| theme::mode_chip(t, s, selected))
                    .on_press(Message::SetMathFont(i))
                    .into(),
            ));
        }

        // ③' **字体大小**（用户 2026-10-29："添加调节字体大小的滑块，和字体选择在同一行"）。
        // 只缩放字号（`theme::text_size`），不动布局 —— 调大不会把键盘挤爆。
        let size_label = t("字体大小");
        let size_group: Element<'_, Message> = row![
            theme::label(size_label.clone()).size(12).style(theme::hint),
            slider(0.7..=1.6, self.font_scale, Message::SetFontScale)
                .step(0.05_f32)
                .width(Length::Fixed(110.0)),
            theme::label(format!("{:.0}%", self.font_scale * 100.0))
                .size(12)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into();
        font_items.push((w_text(&size_label) + 110.0 + 44.0 + 16.0, size_group));
        let font_row: Element<'_, Message> =
            Self::pack_rows(font_items, SETTINGS_ROW_BUDGET, 6.0).into();

        // ④ 主题（用户要求："深色浅色的按钮放到设置里" —— 原来在工具条上）。
        let theme_row = row![
            self.setting_label(t("主题"), SETTING_LABEL_W),
            self.chip(t("深色"), self.dark, Message::ToggleDark),
            self.chip(t("浅色"), !self.dark, Message::ToggleDark),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // ⑤ 质感（**三选一，互斥**）—— 用户规格：
        //   第一行 亚克力：背景模糊 + 半透明面板（原来那套）
        //   第二行 玻璃：iOS liquid glass（折射/高光是后续，先复用同一套背景模糊）
        //   第三行 关闭：界面完全不透明，连抓屏都停掉
        // 每个模式有**自己**的「模糊强度」与「底色浓度」，互不影响。
        let tex = self.texture;
        let pct = |v: f32| t_fmt("{0}%", &[&(v * 100.0).round().to_string()]);

        // 一行里的小件按预算装行：窗口变窄时**自动换到下一行**（iced 没有 flex-wrap）
        // "已开启 / 未启用"两种文案的宽度差很多，按实际文案估算
        let st_on = if tex == theme::Texture::Acrylic {
            t("已开启")
        } else {
            t("未启用")
        };
        let sgroup = |label: String, val: f32, msg: fn(f32) -> Message, w: f32| {
            let e: Element<'_, Message> = row![
                theme::label(label.clone()).size(12).style(theme::hint),
                slider(0.0..=1.0, val, msg)
                    .step(0.01_f32)
                    .on_release(Message::CommitGlass)
                    .width(Length::Fixed(w - 40.0)),
                theme::label(pct(val)).size(13).style(theme::hint),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into();
            (w_text(&label) + w + 24.0, e)
        };
        let tex_acrylic_row: Element<'_, Message> = Self::pack_rows(
            vec![
                (
                    SETTING_LABEL_W,
                    self.setting_label(t("亚克力质感"), SETTING_LABEL_W),
                ),
                (
                    w_text(&st_on) + 24.0,
                    self.chip(
                        st_on.clone(),
                        tex == theme::Texture::Acrylic,
                        Message::SetTexture(theme::Texture::Acrylic),
                    ),
                ),
                sgroup(
                    t("模糊强度"),
                    self.acrylic_blur,
                    Message::SetAcrylicBlur,
                    140.0,
                ),
                sgroup(
                    t("底色浓度"),
                    self.acrylic_tint,
                    Message::SetAcrylicTint,
                    140.0,
                ),
                sgroup(
                    t("设置页浓度"),
                    self.acrylic_dialog_tint,
                    Message::SetAcrylicDialogTint,
                    132.0,
                ),
            ],
            SETTINGS_ROW_BUDGET,
            10.0,
        )
        .into();

        let st_g = if tex == theme::Texture::Glass {
            t("已开启")
        } else {
            t("未启用")
        };
        let tex_glass_row: Element<'_, Message> = Self::pack_rows(
            vec![
                (
                    SETTING_LABEL_W,
                    self.setting_label(t("玻璃质感"), SETTING_LABEL_W),
                ),
                (
                    w_text(&st_g) + 24.0,
                    self.chip(
                        st_g.clone(),
                        tex == theme::Texture::Glass,
                        Message::SetTexture(theme::Texture::Glass),
                    ),
                ),
                sgroup(t("模糊强度"), self.glass_blur, Message::SetGlassBlur, 140.0),
                sgroup(
                    t("底色浓度"),
                    self.glass_tint2,
                    Message::SetGlassTint2,
                    140.0,
                ),
                sgroup(
                    t("设置页浓度"),
                    self.glass_dialog_tint,
                    Message::SetGlassDialogTint,
                    132.0,
                ),
            ],
            SETTINGS_ROW_BUDGET,
            10.0,
        )
        .into();

        let tex_off_row = row![
            self.setting_label(t("关闭"), SETTING_LABEL_W),
            self.chip(
                t("已关闭"),
                tex == theme::Texture::Off,
                Message::SetTexture(theme::Texture::Off),
            ),
            theme::label(t("界面完全不透明：不抓屏、也不模糊"))
                .size(12)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // 三行作为**一组**：内部不再插分隔线（它们是一个三选一的整体）
        let texture_block = column![tex_acrylic_row, tex_glass_row, tex_off_row].spacing(6);

        // ⑥ 面板投影（用户要求："阴影的透明调节加到设置里"）。
        // 单独占一行：玻璃那行已经有两根滑条，再塞一根就太挤了。
        let shadow_row = row![
            self.setting_label(t("面板投影"), SETTING_LABEL_W),
            slider(0.0..=1.0, self.shadow, Message::SetShadow)
                .step(0.01_f32)
                .on_release(Message::CommitGlass)
                .width(Length::Fixed(104.0)),
            theme::label(pct(self.shadow)).size(13).style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // ⑦ 桌面模糊状态（**只读**）。
        //
        // ⚠️ 结果必须显示在界面上：探测报告原本只 eprintln 到控制台，而用户是按
        // `cargo run` 跑、不会盯着控制台 —— "到底生效没有"就只能靠猜。
        // 这类"环境相关的能力"就该显示出来，而不是藏在没人看的 stderr 里。
        let acrylic_row = row![
            self.setting_label(t("桌面模糊"), SETTING_LABEL_W),
            theme::label(if theme::acrylic() {
                t("已启用")
            } else {
                t("系统未提供")
            })
            .size(13)
            .style(theme::hint),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // ⑦'' **渲染缩放**：唯一确定能压低帧时间的旋钮（像素量 ∝ k²）
        let scale_row = row![
            self.setting_label(t("渲染缩放"), SETTING_LABEL_W),
            self.chip(
                "100%".to_string(),
                (self.ui_scale - 1.0).abs() < 0.01,
                Message::SetUiScale(1.0)
            ),
            self.chip(
                "90%".to_string(),
                (self.ui_scale - 0.9).abs() < 0.01,
                Message::SetUiScale(0.9)
            ),
            self.chip(
                "80%".to_string(),
                (self.ui_scale - 0.8).abs() < 0.01,
                Message::SetUiScale(0.8)
            ),
            self.chip(
                "70%".to_string(),
                (self.ui_scale - 0.7).abs() < 0.01,
                Message::SetUiScale(0.7)
            ),
            theme::label(t("像素少则帧率高：帧时间与像素数近似成正比"))
                .size(12)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // ⑦' **允许被截图**（用户 2026-10-21 要求"必须能截图"，但这两件事在
        // Windows 上不能同时成立 —— 见 `GuiPrefs::capture` 的说明）。
        let capture_row = row![
            self.setting_label(t("允许被截图"), SETTING_LABEL_W),
            self.chip(
                t("允许截图"),
                self.allow_capture,
                Message::SetCaptureOk(true)
            ),
            self.chip(
                t("不允许截图"),
                !self.allow_capture,
                Message::SetCaptureOk(false),
            ),
            theme::label(t("关掉后背景是真实桌面，但截图/录屏拍不到窗口"))
                .size(12)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // ⑧ 长结果的处理方式（用户要求："增加左右拖动或者换行，放在设置里供用户选择"）。
        let wrap_row = row![
            self.setting_label(t("长结果"), SETTING_LABEL_W),
            self.chip(t("自动换行"), self.wrap, Message::SetWrap(true)),
            self.chip(t("横向滚动"), !self.wrap, Message::SetWrap(false)),
            theme::label(t("结果太长看不清时怎么办"))
                .size(12)
                .style(theme::hint),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        container(
            column![
                row![
                    // ⚠️ 标题用"设置面板"而不是光"设置"：词典是**子串最长优先**
                    // 匹配，而 CLI 的输出里满地都是"设置"（"已恢复默认设置…"），
                    // 一个两字短键迟早会在某句话里被误替换。
                    theme::label(t("设置面板")).size(16),
                    Space::new().width(Length::Fill),
                    // 提示"点别处就关" —— 用户明确要求了这个交互，
                    // 但没必要让每个人自己去试出来
                    theme::label(t("点面板外任意处关闭"))
                        .size(12)
                        .style(theme::hint),
                ]
                .align_y(Alignment::Center),
                rule::horizontal(1.0).style(theme::divider),
                // ⚠️ **下面这一整块要能上下拉**（用户 2026-10-29："把设置界面做成
                // 可以上下拉动"）。标题与分割线留在外面固定，只有内容滚动 ——
                // 否则滚到下面时标题一起滚走，用户会不知道自己在哪。
                scrollable(
                    // 右侧留 8px：滚动条会压在内容上（与函数面板同款处理）
                    container(
                        column![
                            precision_row,
                            rule::horizontal(1.0).style(theme::divider),
                            lang_row,
                            rule::horizontal(1.0).style(theme::divider),
                            font_row,
                            rule::horizontal(1.0).style(theme::divider),
                            theme_row,
                            rule::horizontal(1.0).style(theme::divider),
                            texture_block,
                            rule::horizontal(1.0).style(theme::divider),
                            shadow_row,
                            rule::horizontal(1.0).style(theme::divider),
                            acrylic_row,
                            rule::horizontal(1.0).style(theme::divider),
                            scale_row,
                            rule::horizontal(1.0).style(theme::divider),
                            capture_row,
                            rule::horizontal(1.0).style(theme::divider),
                            wrap_row,
                        ]
                        .spacing(8)
                    )
                    .padding(iced::Padding {
                        top: 0.0,
                        right: 8.0,
                        bottom: 0.0,
                        left: 0.0,
                    }),
                )
                .height(Length::Fill)
                .width(Length::Fill),
            ]
            .spacing(8),
        )
        .width(Length::Fill)
        // 面板最高不超过"窗口高度 − 上下留白"（56×2），超出就滚动
        .max_height(SETTINGS_MAX_H)
        .padding([12, 14])
        // ⚠️ 用 `dialog`（**接近不透明**的浮层样式）而不是 `panel`：
        // 玻璃面板的 alpha 只有 0.5 上下，下面的历史文字会透上来，
        // 与设置项混在一起 —— 用户实测反馈"设置与计算结果重叠"。
        // `dialog` 保留高光边与大投影（浮起来的感觉），但底色是实心的。
        .style(theme::dialog)
        .into()
    }

    /// 设置面板里的一个"选中态芯片"（主题、行布局这类二选一/开关用它）
    ///
    /// 收 `String`（已翻译）而不是 `&str`：`theme::mode_chip` 需要 `Move` 闭包
    /// 捕获 `selected`，而按钮文案是运行时翻译出来的。
    pub(super) fn chip(&self, label: String, selected: bool, msg: Message) -> Element<'_, Message> {
        // ⚠️ 结构与工具条的 `chip` 同一套（见 `toolbar` 里的长注释）：
        // padding 给内层 container（玻璃层要盖到 padding），玻璃用 `push_under`
        // 垫在文字**下面**且让文字当 base 层决定尺寸 —— 写成 `stack![玻璃, 文字]`
        // 会让 `Fill` 的玻璃成为 base 把 chip 撑满可用空间。
        let padded = container(theme::label(label).size(13)).padding([5, 12]);
        let body: Element<'_, Message> = if theme::liquid_on() {
            Stack::new()
                .push(padded)
                .push_under(liquid::layer(theme::chip_glass(
                    selected,
                    theme::CHIP_RADIUS_GLASS,
                )))
                .into()
        } else {
            padded.into()
        };
        button(body)
            .padding(0)
            .style(move |t, s| theme::mode_chip(t, s, selected))
            .on_press(msg)
            .into()
    }

    /// 设置面板里各行标签，统一宽度让右边的控件对齐成一列
    ///
    /// 收 `String`（已翻译的文案）而不是 `&str`：标签内容随语言变化，
    /// 传借用的话调用处得先把 `t(..)` 的结果存起来，反而啰嗦。
    pub(super) fn setting_label(&self, s: String, w: f32) -> Element<'_, Message> {
        container(text(s).size(13).style(theme::hint))
            .width(Length::Fixed(w))
            .into()
    }

    // ── ③ 输入区 ────────────────────────────────────────────────────────────

    pub(super) fn func_panel(&self) -> Element<'_, Message> {
        let mut body = Column::new().spacing(12);
        for (gi, g) in FN_GROUPS.iter().enumerate() {
            let mut items_col = Column::new().spacing(5);
            // 一行三个，手动切行。行内卡片用等分宽度（FillPortion），
            // 末行不满三个时补空位，保证卡片宽度与上面几行一致。
            for (chunk_i, chunk) in g.items.chunks(FN_COLS).enumerate() {
                let mut r = Row::new().spacing(6).align_y(Alignment::Start);
                for (j, it) in chunk.iter().enumerate() {
                    // 全局下标 = 本行起点 + 列偏移（chunks 会丢掉原始下标，得自己算）
                    let ii = chunk_i * FN_COLS + j;
                    r = r.push(Self::func_entry(gi, ii, it));
                }
                for _ in chunk.len()..FN_COLS {
                    r = r.push(Space::new().width(Length::FillPortion(1)));
                }
                items_col = items_col.push(r);
            }
            body = body.push(
                column![
                    theme::label(t(g.title)).size(12).style(theme::accent_text),
                    items_col,
                ]
                .spacing(4),
            );
        }

        let card = container(
            column![
                // 头部只留一个"关闭"：小浮层不需要大标题（分组标题本身就是导航）
                row![
                    text("ƒ")
                        .size(16)
                        .font(theme::font_math())
                        .style(theme::accent_text),
                    Space::new().width(Length::Fill),
                    button(text("×").size(14))
                        .padding([2, 8])
                        .style(|t, s| theme::mode_chip(t, s, false))
                        .on_press(Message::CloseFuncs),
                ]
                .align_y(Alignment::Center),
                rule::horizontal(1.0).style(theme::divider),
                // 右侧留 8px：滚动条会压在内容上，卡片贴着右缘时问号会被盖住一半
                scrollable(container(body).padding(iced::Padding {
                    top: 0.0,
                    right: 8.0,
                    // 底部留 10px：最后一行不要贴着卡片下沿被"切一半"
                    //（用户 2026-10-27："函数面板下面疑似被虚拟键盘遮住了"）
                    bottom: 10.0,
                    left: 0.0,
                }))
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .spacing(8),
        )
        .width(Length::Fixed(FN_PANEL_W))
        .height(Length::Fixed(FN_PANEL_H))
        .padding(12)
        // 与设置面板同理：浮层必须压得住下面的内容（见 `theme::dialog`）
        .style(theme::dialog);

        card.into()
    }

    /// 函数面板里的一张**小卡片**：结构化签名 + 简述，右上角带圈问号。
    ///
    /// 整卡是一个按钮（点了插入函数）；问号悬停出详细用法 ——
    /// 位置用 **FollowCursor**：提示框跟着鼠标走、画在指针**上方**，
    /// 永远不会盖住问号、也不会被鼠标指针挡住（用户 2026-10-24：
    /// Right 方位的提示框在最右列会翻到左边、正好压住问号）。
    pub(super) fn func_entry(
        gi: usize,
        ii: usize,
        it: &crate::keypad::FnItem,
    ) -> Element<'static, Message> {
        // ── 签名：交给 mathrender 排版 ─────────────────────────────────────
        // 颜色与输入栏同源（theme 文本色 = 浅色主题下的黑），
        // 函数名斜体也与输入栏一致（用户 2026-10-24："函数都用黑色斜体字，
        // 与输入栏字体相同"）。
        let sig_style = mathrender::Style2 {
            size: 14.0,
            color: Some(theme::colors().text),
            italic_names: true,
            ..mathrender::Style2::default()
        };
        // 微积分五函数直接画大运算符（Σ/Π/∫/lim/d/dx）——
        // 槽位填占位字母（f/k/a/b），与 tooltip 里的参数说明对应
        let sig: Element<'static, Message> = match it.bigop {
            // ⚠️ **卡片里不画上下限**（用户 2026-10-27："这个函数面板的微积分渲染是
            // 个啥玩意"）。原因是卡片只有 90px 高：`∫`/`∑` 的字形按 1.6 倍放大，
            // 而上下限按 0.62 倍缩小，两者挤在一起就变成"大字 + 几粒小字飘在旁边"。
            //
            // 卡片的作用是**让人认出这是哪个函数**，不是展示完整排版 —— 完整形状
            // 插进输入框后自然就有（`BigOp` 的排版没动）。所以这里排成**单行紧凑式**：
            // 符号 + 主体（积分再补个 `dx`），字号统一，不再有三级大小。
            Some(kind) => {
                let t = |s: &str| mathrender::Node::Text(s.to_string());
                let nodes: Vec<mathrender::Node> = match kind {
                    mathrender::BigKind::Sum => vec![t("∑"), t(" "), t("f")],
                    mathrender::BigKind::Prod => vec![t("∏"), t(" "), t("f")],
                    mathrender::BigKind::Integral => {
                        vec![t("∫"), t(" "), t("f"), t(" "), t("d"), t("x")]
                    }
                    mathrender::BigKind::Limit => vec![t("lim"), t(" "), t("f")],
                    mathrender::BigKind::Diff => vec![
                        mathrender::Node::Frac(
                            Box::new(t("d")),
                            Box::new(mathrender::Node::Seq(vec![t("d"), t("x")])),
                        ),
                        t(" "),
                        t("f"),
                    ],
                };
                mathrender::build(
                    &mathrender::Node::Seq(nodes),
                    sig_style,
                    None::<&dyn Fn(usize) -> Message>,
                )
            }
            // 普通函数：签名按数学排版（`sin(x)` → 斜体函数名 + 括号）。
            //
            // ⚠️ **含逗号的签名要退回纯文本**：mathrender 把逗号当参数分隔符、
            // 渲染成空（`Node::Comma`），直排会让 `gcd(a, b)` 丢掉逗号。
            // 走结构排版的只有那几个特判函数（sum/prod/int/lim/diff/nroot）。
            None => {
                let structural = !it.sig.contains(',')
                    || ["sum(", "prod(", "int(", "lim(", "diff(", "nroot("]
                        .iter()
                        .any(|p| it.sig.starts_with(p));
                let main = mathrender::render(it.sig, structural, sig_style, None);
                // log 的底数下标要**夹在名字与括号之间**（`log_b(x)`）：
                // 带下标的条目，sig 只写名字部分（"log"），"(x)" 由这里补排。
                // ⚠️ 下标拼在整串签名后面会得到 `log(x)_b`，顺序是错的。
                match it.sub {
                    None => main,
                    Some(sub) => row![
                        main,
                        container(
                            text(sub.to_string())
                                .size(10)
                                .font(theme::font_math_italic())
                        )
                        // 下移量 ≈ 主字号的一半，让 `b` 落在基线之下（视觉下标）
                        .padding([7.0, 1.0]),
                        mathrender::render("(x)", true, sig_style, None),
                    ]
                    .align_y(Alignment::Start)
                    .into(),
                }
            }
        };

        // ── 带圈问号：悬停出详细用法（跟随鼠标） ───────────────────────────
        let c = theme::colors();
        let help = container(text("?").size(10).font(theme::font_cjk()).style(move |_t| {
            iced::widget::text::Style {
                color: Some(c.text_dim),
            }
        }))
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .center_x(Length::Fixed(16.0))
        .center_y(Length::Fixed(16.0))
        .style(move |_t| container::Style {
            // 圆圈 = 正方形 + 全圆角；描边用面板高光色，不抢签名的注意力
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: c.stroke,
            },
            ..container::Style::default()
        });
        let help = iced::widget::tooltip(
            help,
            container(theme::label(t(it.detail)).size(12))
                .max_width(320)
                .padding(10)
                .style(theme::dialog),
            iced::widget::tooltip::Position::FollowCursor,
        );

        // ── 卡片 ───────────────────────────────────────────────────────────
        button(
            column![
                row![sig, Space::new().width(Length::Fill), help]
                    .align_y(Alignment::Start)
                    .spacing(4),
                theme::label(t(it.about)).size(10).style(theme::hint),
            ]
            .spacing(2),
        )
        .width(Length::FillPortion(1))
        .padding([6, 7])
        .style(|t, s| theme::key(t, KeyKind::Function, s))
        .on_press(Message::Function(gi, ii))
        .into()
    }
}
