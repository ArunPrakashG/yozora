//! The launcher's search engine, without a toolkit. Apps, windows, files, the clipboard, emoji,
//! scripts, SSH hosts, projects, browser pages, math, units, currency and time zones, ranked by
//! use. A UI creates an [`Engine`], searches on every keystroke and draws the [`Section`]s it
//! gets back; choosing a row goes through [`Engine::activate`].

mod browser;
mod calc;
mod clock;
mod desktop;
mod engine;
mod glyph;
mod matcher;
mod money;
mod projects;
mod scripts;
mod search;
mod sources;
mod spawn;
mod ssh;
mod usage;

pub use desktop::{Action as AppAction, App};
pub use engine::{Act, ActionItem, Engine, Modifier, Outcome, encode, index, load_settings, web_row};
pub use glyph::Glyph;
pub use scripts::{Arg as ScriptArg, Script};
pub use search::{Entry, Mode, PREFIXES, Row, Section, Target, Visual, committed_prefix, empty_message, parse_mode};
pub use sources::Win;
pub use usage::Usage;
