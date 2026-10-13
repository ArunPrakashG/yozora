//! The clock and its calendar popover, with ISO week numbers.
//!
//! There is no calendar backend, so the popover shows dates only, not events.

use chrono::{Datelike, Duration, Local, NaiveDate};
use gpui::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, div, prelude::*, px,
};
use yozora_ui::{Icon, Theme, controls, icon_sized};

use crate::{app::Config, bar::bi, popover::Popover};

pub fn item(_theme: &Theme, cx: &App) -> Div {
    let format = &Config::get(cx).settings.bar.clock.format;
    let text = format_time(format);
    // Two spaces in the format separate groups, like the date and the time.
    bi().gap(px(8.)).children(text.split("  ").map(|s| SharedString::from(s.to_string())).collect::<Vec<_>>())
}

/// Formats now with `format`, falling back to the default if it is invalid.
pub fn format_time(format: &str) -> String {
    use std::fmt::Write;
    let now = Local::now();
    let mut out = String::new();
    if write!(out, "{}", now.format(format)).is_err() {
        out.clear();
        let _ = write!(out, "{}", now.format("%a %-d %b  %H:%M"));
    }
    out
}

/// Whether `format` is valid strftime, for `check`.
pub fn valid_format(format: &str) -> bool {
    use std::fmt::Write;
    let mut s = String::new();
    write!(s, "{}", Local::now().format(format)).is_ok()
}

/// The first day of the month `offset` months from `today`'s.
fn month_start(today: NaiveDate, offset: i32) -> NaiveDate {
    let months = today.year() * 12 + today.month0() as i32 + offset;
    NaiveDate::from_ymd_opt(months.div_euclid(12), months.rem_euclid(12) as u32 + 1, 1).expect("valid month")
}

/// Six weeks of dates starting on the Monday on or before the 1st.
fn weeks(first: NaiveDate) -> Vec<[NaiveDate; 7]> {
    let start = first - Duration::days(first.weekday().num_days_from_monday() as i64);
    (0..6)
        .map(|w| std::array::from_fn(|d| start + Duration::days(w * 7 + d as i64)))
        .collect()
}

pub fn popover(p: &mut Popover, theme: &Theme, cx: &mut Context<Popover>) -> Vec<AnyElement> {
    let today = Local::now().date_naive();
    let week_numbers = Config::get(cx).settings.bar.clock.week_numbers;
    let first = month_start(today, p.ui.month_offset);

    let top = div()
        .flex()
        .items_end()
        .justify_between()
        .px(px(6.))
        .pt(px(4.))
        .pb(px(12.))
        .child(
            div()
                .flex()
                .flex_col()
                .child(eyebrow(today.format("%A").to_string(), theme))
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .mt(px(3.))
                        .child(today.format("%-d %B").to_string()),
                ),
        )
        .when(week_numbers, |el| {
            el.child(controls::muted(format!("Week {}", today.iso_week().week()), theme).pb(px(2.)))
        });

    let nav_button = |id: &'static str, glyph: Icon, delta: i32, cx: &mut Context<Popover>| {
        div()
            .id(id)
            .size(px(28.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.ink_2)
            .cursor_pointer()
            .hover(|s| s.bg(theme.fill_2).text_color(theme.ink))
            .child(icon_sized(glyph, 15.))
            .on_click(cx.listener(move |p, _, _, cx| {
                p.ui.month_offset += delta;
                cx.notify();
            }))
    };
    let label = if first.year() == today.year() {
        first.format("%B").to_string()
    } else {
        first.format("%B %Y").to_string()
    };
    let nav = div()
        .flex()
        .items_center()
        .justify_between()
        .pl(px(6.))
        .pr(px(2.))
        .pb(px(6.))
        .child(
            div()
                .id("month-label")
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .child(label)
                // Clicking the month returns to today.
                .on_click(cx.listener(|p, _, _, cx| {
                    p.ui.month_offset = 0;
                    cx.notify();
                })),
        )
        .child(
            div()
                .flex()
                .gap(px(2.))
                .child(nav_button("prev-month", Icon::ChevronLeft, -1, cx))
                .child(nav_button("next-month", Icon::ChevronRight, 1, cx)),
        );

    let cell = |w: f32| div().flex().items_center().justify_center().when(w > 0., |el| el.w(px(w))).when(w == 0., |el| el.flex_1());
    let header = div()
        .flex()
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_3)
        .pt(px(4.))
        .pb(px(6.))
        .when(week_numbers, |el| el.child(cell(24.).child("W")))
        .children(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|d| cell(0.).child(d)));
    let grid = weeks(first).into_iter().map(|week| {
        div()
            .flex()
            .when(week_numbers, |el| {
                el.child(cell(24.).text_size(px(10.)).text_color(theme.ink_3).child(week[0].iso_week().week().to_string()))
            })
            .children(week.map(|d| {
                let is_today = d == today;
                let outside = d.month() != first.month();
                let weekend = d.weekday().num_days_from_monday() >= 5;
                cell(0.).h(px(32.)).text_size(px(12.5)).child(
                    div()
                        .size(px(28.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(outside, |el| el.text_color(theme.ink_3).opacity(0.7))
                        .when(!outside && weekend, |el| el.text_color(theme.ink_2))
                        .when(is_today, |el| {
                            el.bg(theme.accent)
                                .text_color(theme.accent_ink)
                                .font_weight(FontWeight::BOLD)
                                .shadow(theme.accent_glow())
                        })
                        .child(d.day().to_string()),
                )
            }))
            .into_any_element()
    });
    let mut rows = vec![top.into_any_element(), nav.into_any_element(), header.into_any_element()];
    rows.push(div().flex().flex_col().children(grid).into_any_element());
    rows
}

pub fn eyebrow(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.accent)
        .child(text.into().to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_and_weeks() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        assert_eq!(month_start(d, -1), NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
        assert_eq!(month_start(d, 13), NaiveDate::from_ymd_opt(2027, 2, 1).unwrap());
        let w = weeks(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap());
        // 1 Oct 2026 is a Thursday; the grid starts Monday 28 Sep.
        assert_eq!(w[0][0], NaiveDate::from_ymd_opt(2026, 9, 28).unwrap());
        assert_eq!(w[0][0].iso_week().week(), 40);
        assert!(valid_format("%H:%M") && !valid_format("%Q"));
    }
}
