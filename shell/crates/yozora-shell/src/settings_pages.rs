//! The settings pages other than Appearance.

use rune::El;
use yozora_style::Tokens;

use crate::kit::label;
use crate::stage::Stage;

pub fn view(_s: &Stage, t: &Tokens, page: &str) -> El {
    label(t, 22.0, 600.0, page, t.ink)
}
