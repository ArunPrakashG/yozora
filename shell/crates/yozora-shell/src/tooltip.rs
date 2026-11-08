//! Tooltips for bar items. The bar's own surface ends at the bar, so the text is drawn here, on
//! the stage, just below the item it names. The first line is a title; the rest is detail.

use std::time::Duration;

use rune::{Color, El, col};
use yozora_style::{Fx, Look, Tokens};

use crate::bar;
use crate::kit::{enter, label};
use crate::stage::Stage;

/// How long the pointer rests on an item before its tooltip shows.
pub const DELAY: Duration = Duration::from_millis(450);
const MAX_W: f32 = 360.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Tip {
    /// The bar item it belongs to, so leaving that item (and only that item) clears it.
    pub item: String,
    pub text: String,
    /// The item's centre, in screen points.
    pub x: f32,
}

/// Roughly how wide the text sets, to centre it under its item before it has been laid out.
fn guess_width(text: &str) -> f32 {
    let longest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
    (longest * 6.8 + 28.0).clamp(60.0, MAX_W)
}

/// The tooltip as an element, pinned under the bar.
pub fn view(s: &Stage, t: &Tokens, fx: Fx, top: f32) -> Option<El> {
    let tip = s.bar.tip.as_ref()?;
    if tip.text.trim().is_empty() {
        return None;
    }
    let w = guess_width(&tip.text);
    let left = (tip.x - w / 2.0).clamp(bar::SIDE, (s.screen_w - w - bar::SIDE).max(bar::SIDE));
    let mut lines = col().gap(2.0).max_w(MAX_W);
    for (i, line) in tip.text.lines().enumerate() {
        lines = lines.child(if i == 0 { label(t, 12.5, 600.0, line, t.ink) } else { label(t, 12.0, 400.0, line, t.ink2) });
    }
    let boxed = lines.px(12.0).py(8.0).radius(t.radii.ctl).fill(t.surf);
    let boxed = match t.look {
        Look::Folio | Look::Phosphor => boxed.border(1.0, t.ink),
        _ => boxed.border(1.0, t.rule).shadow(8.0, 24.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.3 }),
    };
    Some(boxed.key(format!("tip-{}", tip.item)).pin(Some(left), Some(top), None, None).transition(enter(t, fx, tip.x - left, w)))
}

/// Shows `text` under `item`, or takes it down.
pub fn set(s: &mut Stage, item: &str, text: String, x: f32) {
    s.bar.tip = Some(Tip { item: item.to_owned(), text, x });
}

/// Takes the tooltip down if it is `item`'s.
pub fn clear(s: &mut Stage, item: &str) {
    if s.bar.tip.as_ref().is_some_and(|t| t.item == item) {
        s.bar.tip = None;
    }
}
