//! **「自定义函数与变量」页**（用户 2026-10-31 要求）。
//!
//! 为什么单开一页：自定义函数（`f(x) = x^2 + 1`）与存储变量（`A = 3`）都是"用户自己
//! 造出来的东西"，在历史列表里只能靠翻记录找。这一页把它们**列成表**，可直接删除，
//! 也能**在页面内直接定义**（不必切回主输入栏）。
//!
//! 交互（用户 2026-10-31 指定）：
//! 1. 先点 `函数` / `变量` 切模式；
//! 2. 按模式填字段（函数：名称 + 参数 + 表达式；变量：名称 + 值）—— **参数个数不限**；
//! 3. 点 **`保存`** 完成定义；
//! 4. 列表上方有**搜索框**，规则与计算栏历史搜索一致（大小写不敏感的子串匹配），
//!    命中片段用主题强调色标出。
//!
//! 与 `/fn`、`/del` 的关系：底层是同一份状态（`Session` 的 `user_fns` / `vars`），
//! 页面只是它的一个视图 + 两个动作（定义 / 删除），不另存一份。

use super::*;

/// 把 `s` 里第一处匹配 `q` 的片段用**强调色**标出来（`q` 为空则原样显示）。
///
/// 实现说明：三段并排的 `text` 拼成一个 `Row`（间距 0），观感与单个 `text` 一致。
/// 没用富文本 API 是刻意的 —— 这里只要"一段变色"，三段拼接更直观、也不受排版限制。
fn highlighted(s: String, q: &str, size: f32, font: Font) -> Element<'static, Message> {
    let plain = |t: String| text(t).size(size).font(font);
    let q_lower = q.trim().to_lowercase();
    if q_lower.is_empty() {
        return plain(s).into();
    }
    let Some(pos) = s.to_lowercase().find(&q_lower) else {
        return plain(s).into();
    };
    // 按**字节**切：q_lower 与 s 的匹配长度在小写后可能变（本项目的输入基本是
    // ASCII 算式 + 中文名，`to_lowercase` 对中文不改变长度），这里仍按原始长度夹一下防越界。
    let end = (pos + q_lower.len()).min(s.len());
    let before = s[..pos].to_string();
    let mid = s[pos..end].to_string();
    let after = s[end..].to_string();
    row![
        plain(before),
        text(mid)
            .size(size)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..font
            })
            .style(theme::accent_text),
        plain(after),
    ]
    .spacing(0)
    .into()
}

impl State {
    pub(super) fn userdefs_page(&self) -> Element<'_, Message> {
        let mut body = Column::new().spacing(10).width(Length::Fill);

        // ── 模式切换（点按钮决定下面填哪些字段）──────────────────────────
        let chip = |label: &'static str, on: bool, to_fn: bool| {
            button(theme::label(t(label)).size(13))
                .padding([5, 14])
                .style(move |th, st| theme::mode_chip(th, st, on))
                .on_press(Message::UdfMode(to_fn))
        };
        body = body.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(chip("函数", self.udf_is_fn, true))
                .push(chip("变量", !self.udf_is_fn, false)),
        );

        // ── 字段 ──────────────────────────────────────────────────────────
        let field = |placeholder: &'static str, value: &str, on: fn(String) -> Message, w: f32| {
            text_input(placeholder, value)
                .on_input(on)
                .on_submit(Message::UdfSubmit)
                .width(Length::Fixed(w))
                .size(14)
                .padding([6, 9])
                .font(theme::font_math())
                .style(theme::field)
        };
        let mut fields = Row::new().spacing(8).align_y(Alignment::Center);
        if self.udf_is_fn {
            fields = fields
                .push(theme::label(t("名称")).size(13).style(theme::hint))
                .push(field("f", &self.udf_name, Message::UdfName, 58.0))
                .push(theme::label(t("参数")).size(13).style(theme::hint))
                .push(field("x, y", &self.udf_params, Message::UdfParams, 110.0))
                .push(theme::label(t("表达式")).size(13).style(theme::hint))
                .push(field("x^2 + 1", &self.udf_expr, Message::UdfExpr, 220.0));
        } else {
            fields = fields
                .push(theme::label(t("名称")).size(13).style(theme::hint))
                .push(field("A", &self.udf_name, Message::UdfName, 88.0))
                .push(theme::label(t("值")).size(13).style(theme::hint))
                .push(field("3", &self.udf_expr, Message::UdfExpr, 300.0));
        }
        fields = fields.push(
            button(theme::label(t("保存")).size(13))
                .padding([6, 16])
                .style(|t, s| theme::key(t, KeyKind::Equals, s))
                .on_press(Message::UdfSubmit),
        );
        body = body.push(fields);
        if let Some(h) = &self.udf_hint {
            body = body.push(theme::label(h.as_str()).size(13).style(theme::danger_text));
        }

        // ── 搜索（与计算栏历史搜索同一套规则）────────────────────────────
        body = body.push(
            text_input(t("搜索名称或内容").as_str(), &self.udf_search)
                .on_input(Message::UdfSearch)
                .width(Length::Fill)
                .size(13)
                .padding([6, 9])
                .style(theme::field),
        );
        let q = self.udf_search.trim().to_lowercase();
        let hit = |s: &str| q.is_empty() || s.to_lowercase().contains(&q);

        // ── 函数表 ────────────────────────────────────────────────────────
        body = body.push(theme::label(t("自定义函数")).size(13).style(theme::hint));
        let fns: Vec<_> = self
            .user_fns
            .iter()
            .filter(|(name, params, body_text)| {
                hit(name) || hit(&params.join(", ")) || hit(body_text)
            })
            .collect();
        if fns.is_empty() {
            body = body.push(
                theme::label(if q.is_empty() {
                    t("还没有自定义函数")
                } else {
                    t("没有匹配的函数")
                })
                .size(13)
                .style(theme::hint),
            );
        } else {
            let mut col = Column::new().spacing(4);
            for (name, params, body_text) in fns {
                let expr = format!("{name}({}) = {body_text}", params.join(", "));
                col = col.push(
                    Row::new()
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .push(
                            container(highlighted(
                                expr.clone(),
                                &self.udf_search,
                                theme::text_size(15.0),
                                theme::font_math(),
                            ))
                            .width(Length::Fill),
                        )
                        .push(
                            button(theme::label(t("删除")).size(11))
                                .padding([3, 10])
                                .style(|t, s| theme::mode_chip(t, s, false))
                                .on_press(Message::DelUserDef {
                                    is_fn: true,
                                    name: name.clone(),
                                }),
                        ),
                );
            }
            body = body.push(
                container(col)
                    .width(Length::Fill)
                    .padding([8, 10])
                    .style(theme::panel_card),
            );
        }

        // ── 变量表 ────────────────────────────────────────────────────────
        body = body.push(theme::label(t("存储变量")).size(13).style(theme::hint));
        let vars: Vec<_> = self
            .user_vars
            .iter()
            .filter(|(name, value)| hit(name) || hit(value))
            .collect();
        if vars.is_empty() {
            body = body.push(
                theme::label(if q.is_empty() {
                    t("还没有变量")
                } else {
                    t("没有匹配的变量")
                })
                .size(13)
                .style(theme::hint),
            );
        } else {
            let mut col = Column::new().spacing(4);
            for (name, value) in vars {
                let line = format!("{name} = {value}");
                col = col.push(
                    Row::new()
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .push(
                            container(highlighted(
                                line.clone(),
                                &self.udf_search,
                                theme::text_size(15.0),
                                theme::font_math(),
                            ))
                            .width(Length::Fill),
                        )
                        .push(
                            button(theme::label(t("删除")).size(11))
                                .padding([3, 10])
                                .style(|t, s| theme::mode_chip(t, s, false))
                                .on_press(Message::DelUserDef {
                                    is_fn: false,
                                    name: name.clone(),
                                }),
                        ),
                );
            }
            body = body.push(
                container(col)
                    .width(Length::Fill)
                    .padding([8, 10])
                    .style(theme::panel_card),
            );
        }

        let inner = scrollable(container(body).padding([12, 12]))
            .height(Length::Fill)
            .width(Length::Fill);

        Self::glass_under(
            container(inner)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(6)
                .style(theme::panel_card)
                .into(),
            theme::panel_glass(),
        )
    }
}
