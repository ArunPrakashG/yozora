mod bar;

use bar::{Bar, view};
use rune::AppSpec;
use yozora_style::{FONTS, Look};

fn init() -> Bar {
    Bar::demo(std::env::var("YZ_LOOK").ok().and_then(|s| Look::from_slug(&s)).unwrap_or_default())
}

fn main() {
    rune::run(AppSpec::<Bar> { fonts: FONTS, ..AppSpec::new("Yozora bar", (1440.0, 44.0), init, view) });
}
