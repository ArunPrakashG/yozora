use chrono::{Datelike, Duration, NaiveDate};
use rune::{El, col, el, row};
use yozora_style::Tokens;

use crate::kit::{cell, display, fig, glyph_el, icon_button, label, panel};
use crate::stage::Stage;

pub const W: f32 = 332.0;

pub fn view(s: &Stage, t: &Tokens) -> El {
    let (y, m) = s.month;
    let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    let today = chrono::Local::now().date_naive();
    let start = first - Duration::days(first.weekday().num_days_from_monday() as i64);
    let months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

    let title = row().grow(1.0).center_y().gap(6.0)
        .child(label(t, 22.0, 600.0, months[(m - 1) as usize], t.ink))
        .child(display(t, 20.0, y.to_string(), t.ink2));
    let head = row().w_full().h(48.0).center_y().px(8.0)
        .child(title)
        .child(icon_button(t, "prev", "chevl", |s| s.step_month(-1)))
        .child(icon_button(t, "next", "chev", |s| s.step_month(1)));

    let mut dow = row().w_full().h(24.0).center_y().child(cell(32.0, 24.0, label(t, 11.0, 500.0, "Wk", t.ink3)));
    for d in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
        dow = dow.child(cell(36.0, 24.0, label(t, 11.0, 500.0, d, t.ink2)));
    }
    let mut grid = col().w_full().gap(4.0).child(dow);
    for week in 0..6 {
        let monday = start + Duration::days(week * 7);
        let mut r = row().w_full().h(36.0).child(cell(32.0, 36.0, fig(t, 11.0, 400.0, monday.iso_week().week().to_string(), t.ink3)));
        for d in 0..7 {
            let day = monday + Duration::days(d);
            let inside = day.month() == m;
            let is_today = day == today;
            let txt = if is_today {
                fig(t, 14.0, 600.0, day.day().to_string(), t.on_acc)
            } else {
                fig(t, 14.0, if inside { 500.0 } else { 400.0 }, day.day().to_string(), if inside { t.ink } else { t.ink3 })
            };
            let mut c = cell(36.0, 36.0, txt).radius(t.radii.pill);
            if is_today {
                c = c.fill(t.acc);
            }
            r = r.child(c);
        }
        grid = grid.child(r);
    }
    let _ = (el, glyph_el);
    panel(t, W).child(head).child(grid)
}
