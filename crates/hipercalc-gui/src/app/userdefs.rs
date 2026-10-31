//! **「自定义函数与变量」页**（用户 2026-10-31 要求）。
//!
//! 为什么单开一页：自定义函数（`f(x) = x^2 + 1`）与存储变量（`A = 3`）都是"用户自己
//! 造出来的东西"，在历史列表里只能靠翻记录找。这一页把它们**列成表**，可直接删除，
//! 也能**在页面内直接定义**（不必切回主输入栏）。
//!
//! 与 `/fn`、`/del` 的关系：底层是同一份状态（`Session` 的 `user_fns` / `vars`），
//! 页面只是它的一个视图 + 两个动作（定义 / 删除），不另存一份。
//!
//! ⚠️ **页面内的定义走与主输入栏完全相同的通道**（`Message::UdfSubmit` → `evaluate`），
//! 所以语法只有一套；这里用普通 `text_input` 而不是结构化编辑器，与三角求解页一致。

use super::*;

impl State {
    pub(super) fn userdefs_page(&self) -> Element<'_, Message> {
        let mut body = Column::new().spacing(10).width(Length::Fill);

        // ── 页面内定义 ────────────────────────────────────────────────────
        body = body.push(
            theme::label(t("输入 f(x)=x^2+1 定义函数，或 A=3 定义变量"))
                .size(13)
                .style(theme::hint),
        );
        body = body.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(
                    text_input("f(x)=x^2+1", &self.udf_input)
                        .on_input(Message::UdfChanged)
                        .on_submit(Message::UdfSubmit)
                        .width(Length::Fill)
                        .size(14)
                        .padding([7, 10])
                        .font(theme::font_math())
                        .style(theme::field),
                )
                .push(
                    button(theme::label(t("定义")).size(13))
                        .padding([6, 16])
                        .style(|t, s| theme::key(t, KeyKind::Equals, s))
                        .on_press(Message::UdfSubmit),
                ),
        );
        if let Some(h) = &self.udf_hint {
            body = body.push(theme::label(h.as_str()).size(13).style(theme::danger_text));
        }

        // ── 函数表 ────────────────────────────────────────────────────────
        body = body.push(theme::label(t("自定义函数")).size(13).style(theme::hint));
        if self.user_fns.is_empty() {
            body = body.push(
                theme::label(t("还没有自定义函数"))
                    .size(13)
                    .style(theme::hint),
            );
        } else {
            let mut col = Column::new().spacing(4);
            for (name, params, body_text) in &self.user_fns {
                let expr = format!("{name}({}) = {body_text}", params.join(", "));
                col = col.push(
                    Row::new()
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .push(
                            text(expr)
                                .size(theme::text_size(15.0))
                                .font(theme::font_math())
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
        if self.user_vars.is_empty() {
            body = body.push(theme::label(t("还没有变量")).size(13).style(theme::hint));
        } else {
            let mut col = Column::new().spacing(4);
            for (name, value) in &self.user_vars {
                col = col.push(
                    Row::new()
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .push(
                            text(format!("{name} = {value}"))
                                .size(theme::text_size(15.0))
                                .font(theme::font_math())
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
