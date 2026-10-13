//! Motion: reduced motion and the popover spring, with its response curve and a live preview.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, PathBuilder,
    StatefulInteractiveElement, Styled, Window, canvas, div, point, prelude::FluentBuilder, px,
};
use yozora_config::theme::ThemeConfig;
use yozora_ui::{
    Icon, Theme,
    controls::{segmented, switch},
    icon_sized,
    spring::{SpringConfig, settle_time, step_response},
};

use crate::{
    view::{SettingsView, THEME_FILE, with_alpha},
    widgets::{card, group_label, setting_row},
};

pub const SEARCH: &[(&str, &str)] = &[
    ("Reduce motion", "animation accessibility fade springs"),
    ("Popover feel", "spring stiffness damping crisp balanced soft"),
];

/// Crisp, Balanced, Soft: stiffness and damping.
const FEELS: [(f32, f32); 3] = [(560., 40.), (380., 28.), (220., 20.)];

const GRAPH_W: f32 = 280.;
const GRAPH_H: f32 = 150.;
/// The graph spans 800 ms.
const GRAPH_MS: f32 = 800.;

pub struct Curve {
    pub t95_ms: u32,
    pub overshoot: f32,
    pub rest_ms: u32,
}

pub fn curve(config: SpringConfig) -> Curve {
    let t95_ms = (0..2000).find(|ms| step_response(config, *ms as f32 / 1000.) >= 0.95).unwrap_or(0);
    let w0 = config.stiffness.sqrt();
    let zeta = config.damping / (2. * w0);
    let overshoot =
        if zeta < 1. { (-zeta * std::f32::consts::PI / (1. - zeta * zeta).sqrt()).exp() * 100. } else { 0. };
    Curve { t95_ms, overshoot, rest_ms: settle_time(config).as_millis() as u32 }
}

pub fn render(view: &mut SettingsView, window: &mut Window, cx: &mut Context<SettingsView>) -> AnyElement {
    let theme = Theme::global(cx).clone();
    let config: ThemeConfig = view.doc(THEME_FILE).parse();
    let motion = config.motion.clone();
    let spring = SpringConfig { stiffness: motion.popover.stiffness, damping: motion.popover.damping };
    let feel = FEELS.iter().position(|&(k, c)| (k - spring.stiffness).abs() < 0.5 && (c - spring.damping).abs() < 0.5);
    let stats = curve(spring);
    let weak = cx.entity().downgrade();

    // The preview popover follows the spring from the last replay.
    let elapsed = view.replay.map(|t| t.elapsed()).unwrap_or(Duration::from_secs(10));
    let progress = if motion.reduced {
        (elapsed.as_secs_f32() / 0.12).min(1.)
    } else {
        step_response(spring, elapsed.as_secs_f32())
    };
    let settled = elapsed > settle_time(spring).max(Duration::from_millis(120));
    if !settled {
        window.request_animation_frame();
    }

    let (accent, hair, ink_3) = (theme.accent, theme.hair, theme.ink_3);
    let graph = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let o = bounds.origin;
            let at = |ms: f32, v: f32| point(o.x + px(ms / GRAPH_MS * GRAPH_W), o.y + px(GRAPH_H - 120. * v));
            // Baselines at 0 and 1.
            for v in [0., 1.] {
                let mut line = PathBuilder::stroke(px(1.));
                line.move_to(at(0., v));
                line.line_to(at(GRAPH_MS, v));
                if let Ok(path) = line.build() {
                    window.paint_path(path, hair);
                }
            }
            // A 200 ms ease-out, for comparison.
            let mut ghost = PathBuilder::stroke(px(1.2));
            ghost.move_to(at(0., 0.));
            for ms in (8..=200).step_by(8) {
                let t = ms as f32 / 200.;
                ghost.line_to(at(ms as f32, 1. - (1. - t).powi(3)));
            }
            ghost.line_to(at(GRAPH_MS, 1.));
            if let Ok(path) = ghost.build() {
                window.paint_path(path, with_alpha(ink_3, 0xA0));
            }
            // The spring, as reduced motion would play it when that's on.
            let mut line = PathBuilder::stroke(px(2.2));
            line.move_to(at(0., 0.));
            let mut ms = 0.;
            while ms <= GRAPH_MS {
                let v = if motion.reduced { (ms / 120.).min(1.) } else { step_response(spring, ms / 1000.) };
                line.line_to(at(ms, v));
                ms += 4.;
            }
            if let Ok(path) = line.build() {
                window.paint_path(path, accent);
            }
        },
    )
    .w(px(GRAPH_W))
    .h(px(GRAPH_H));

    let stat = |value: String, label: &str| {
        div()
            .flex()
            .flex_col()
            .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(value))
            .child(div().mt(px(2.)).text_size(px(11.5)).text_color(theme.ink_2).child(label.to_string()))
    };

    let lines = |el: gpui::Div, n: usize| {
        el.children((0..n).map(|i| {
            div().h(px(5.)).rounded(px(3.)).bg(theme.fill_3).when(i % 2 == 1, |el| el.w(gpui::relative(0.58)))
        }))
    };
    let opacity = progress.clamp(0., 1.);
    let shift = if motion.reduced { 0. } else { -8. * (1. - progress) };

    div()
        .flex()
        .flex_col()
        .child(
            card(&theme)
                .child(setting_row(
                    "Reduce motion",
                    Some("Springs become a 120 ms fade. Nothing slides or scales.".into()),
                    switch("reduced", motion.reduced, &theme).on_click(cx.listener(move |this, _, _, cx| {
                        this.edit(THEME_FILE, cx, |d| d.set("motion.reduced", !motion.reduced));
                    })),
                    true,
                    &theme,
                ))
                .child(setting_row(
                    "Popover feel",
                    Some(format!("stiffness {} · damping {}", spring.stiffness, spring.damping).into()),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .when(feel.is_none(), |el| el.child(div().text_size(px(12.)).text_color(theme.ink_3).child("Custom")))
                        .child(div().w(px(270.)).child(segmented("feel", &["Crisp", "Balanced", "Soft"], feel.unwrap_or(1), &theme, move |i, _, cx| {
                            let (k, c) = FEELS[i];
                            weak.update(cx, |this, cx| {
                                this.edit(THEME_FILE, cx, |d| {
                                    d.edit(|f| {
                                        f.set("motion.popover.stiffness", k as f64);
                                        f.set("motion.popover.damping", c as f64);
                                    })
                                });
                                this.replay = Some(Instant::now());
                            })
                            .ok();
                        }))),
                    false,
                    &theme,
                )),
        )
        .child(
            card(&theme)
                .mt(px(16.))
                .p(px(18.))
                .flex_row()
                .gap(px(20.))
                .child(
                    div()
                        .flex_none()
                        .w(px(GRAPH_W))
                        .child(graph)
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .mt(px(6.))
                                .font_family(theme.mono.clone())
                                .text_size(px(10.5))
                                .text_color(theme.ink_3)
                                .children(["0", "200", "400", "600", "800 ms"].map(|s| div().child(s))),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(18.))
                                .mt(px(12.))
                                .child(stat(format!("{} ms", stats.t95_ms), "to 95%"))
                                .child(stat(format!("{:.1}%", stats.overshoot), "overshoot"))
                                .child(stat(format!("{} ms", stats.rest_ms), "at rest")),
                        ),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w(px(150.))
                        .h(px(200.))
                        .rounded(px(12.))
                        .overflow_hidden()
                        .bg(crate::widgets::sky(!theme.dark))
                        .border_1()
                        .border_color(theme.hair)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(5.))
                                .px(px(7.))
                                .bg(theme.glass_bar)
                                .border_b_1()
                                .border_color(theme.hair)
                                .child(div().w(px(12.)).h(px(5.)).rounded(px(3.)).bg(theme.fill_3))
                                .child(div().w(px(12.)).h(px(5.)).rounded(px(3.)).bg(theme.fill_3))
                                .child(div().size(px(10.)).rounded(px(3.)).bg(theme.accent)),
                        )
                        .child(lines(
                            div()
                                .absolute()
                                .right(px(7.))
                                .top(px(26. + shift))
                                .w(px(132.))
                                .p(px(9.))
                                .flex()
                                .flex_col()
                                .gap(px(7.))
                                .rounded(px(10.))
                                .bg(theme.glass_pop)
                                .border_1()
                                .border_color(theme.hair_2)
                                .opacity(opacity),
                            5,
                        ))
                        .child(
                            div()
                                .id("replay")
                                .absolute()
                                .left(px(8.))
                                .bottom(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .h(px(28.))
                                .pl(px(9.))
                                .pr(px(11.))
                                .rounded(px(14.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .bg(theme.glass_pop)
                                .border_1()
                                .border_color(theme.hair_2)
                                .cursor_pointer()
                                .child(icon_sized(Icon::Play, 13.))
                                .child("Replay")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.replay = Some(Instant::now());
                                    cx.notify();
                                })),
                        ),
                ),
        )
        .child(
            div()
                .mt(px(14.))
                .ml(px(2.))
                .text_size(px(12.5))
                .text_color(theme.ink_2)
                .child(format!(
                    "Dashed: a 200 ms ease-out, for comparison. Workspaces, switches and highlights use the indicator spring, {} / {}.",
                    motion.indicator.stiffness, motion.indicator.damping
                )),
        )
        .child(group_label("", false, &theme))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_matches_the_design() {
        let c = curve(SpringConfig { stiffness: 380., damping: 28. });
        assert!((3.0..5.0).contains(&c.overshoot), "{}", c.overshoot);
        assert!((150..260).contains(&c.t95_ms), "{}", c.t95_ms);
    }
}
