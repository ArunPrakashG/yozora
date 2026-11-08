use rune::El;
use yozora_style::Tokens;

use crate::kit::{header, panel};
use crate::stage::Stage;

pub const W: f32 = 260.0;

pub fn view(_s: &Stage, t: &Tokens) -> El {
    panel(t, W).child(header(t, "Tray", None))
}
