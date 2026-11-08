//! The calendar popover: a month of weeks that starts on Monday, with ISO week numbers when
//! `clock.week_numbers` is on in bar.toml, weekends a shade quieter, and today marked in the
//! look's own way. The month name takes you back to today.

use chrono::{Datelike, Duration, NaiveDate};
use rune::tok::TextStyle;
use rune::{Color, El, col, row, text};
use yozora_style::{Look, Tokens};

use crate::kit::{cell, display, fig, icon_button, label, panel};
use crate::stage::Stage;

pub const W: f32 = 332.0;

/// Today's cell: a disc in the accent, a ring, an inverted block or a glowing disc, per look.
fn today_cell(t: &Tokens, day: u32) -> El {
    let n = day.to_string();
    match t.look {
        Look::Folio => {
            let txt = text(n).font(TextStyle::new(14.0, 600.0).family(t.fonts.fig).italic().features("'tnum' 1")).color(t.acc);
            cell(36.0, 36.0, txt).radius(t.radii.pill).border(1.5, t.acc)
        }
        Look::Phosphor => cell(36.0, 36.0, fig(t, 14.0, 600.0, n, t.on_ink)).fill(t.ink),
        Look::Prism => cell(36.0, 36.0, fig(t, 14.0, 600.0, n, t.on_acc)).radius(t.radii.pill).fill(t.acc).shadow(6.0, 14.0, Color { a: 0.5, ..t.acc }),
        _ => cell(36.0, 36.0, fig(t, 14.0, 600.0, n, t.on_acc)).radius(t.radii.pill).fill(t.acc),
    }
}

pub fn view(s: &Stage, t: &Tokens) -> El {
    let (y, m) = s.month;
    let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    let today = chrono::Local::now().date_naive();
    let week_numbers = s.sh.bar_conf().bar.clock.week_numbers;
    let start = first - Duration::days(first.weekday().num_days_from_monday() as i64);
    let months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

    let title = row().grow(1.0).center_y().gap(6.0)
        .child(label(t, 22.0, 600.0, months[(m - 1) as usize], t.ink))
        .child(display(t, 20.0, y.to_string(), t.ink2))
        .key("month").role("button").label("Back to this month").hit()
        .on_press(|s: &mut Stage| {
            let today = chrono::Local::now().date_naive();
            s.month = (today.year(), today.month());
        });
    let head = row().w_full().h(48.0).center_y().px(8.0)
        .child(title)
        .child(icon_button(t, "prev", "chevl", |s| s.step_month(-1)))
        .child(icon_button(t, "next", "chev", |s| s.step_month(1)));

    let mut dow = row().w_full().h(24.0).center_y();
    if week_numbers {
        dow = dow.child(cell(32.0, 24.0, label(t, 11.0, 500.0, "Wk", t.ink3)));
    }
    dow = dow.justify_center();
    for d in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
        dow = dow.child(cell(36.0, 24.0, label(t, 11.0, 500.0, d, t.ink2)));
    }
    let square = t.look == Look::Phosphor;
    let mut grid = col().w_full().gap(4.0).child(dow);
    for week in 0..6 {
        let monday = start + Duration::days(week * 7);
        let mut r = row().w_full().h(36.0).justify_center();
        if week_numbers {
            r = r.child(cell(32.0, 36.0, fig(t, 11.0, 400.0, monday.iso_week().week().to_string(), t.ink3)));
        }
        for d in 0..7 {
            let day = monday + Duration::days(d);
            let inside = day.month() == m;
            let weekend = d >= 5;
            if day == today {
                r = r.child(today_cell(t, day.day()));
                continue;
            }
            let color = if !inside { t.ink3 } else if weekend { t.ink2 } else { t.ink };
            let c = cell(36.0, 36.0, fig(t, 14.0, if inside { 500.0 } else { 400.0 }, day.day().to_string(), color));
            r = r.child(if square { c } else { c.radius(t.radii.pill) });
        }
        grid = grid.child(r);
    }
    panel(t, W).child(head).child(grid)
}
