//! Forms: the text field every page shares, and the page drawn from an app's settings schema.

use rune::tok::TextStyle;
use rune::{Color, El, el, Key, KeyEvent, Tx, col, edit_access, row, text_input};
use yozora_config::schema::{Field, FieldKind, Schema, display_value};
use yozora_style::Tokens;

use crate::settings_apps::AppEntry;
use crate::settings_doc::{string_array, to_edit_value};
use crate::settings_state::Target;
use crate::settings_ui::{self as ui, card, group, mono, notice, segmented, pills, setting, toggle, tool};
use crate::kit::{fig, glyph_el, label};
use crate::stage::Stage;

/// How a text field looks and where it writes.
pub struct Input<'a> {
    pub id: String,
    pub target: Target,
    /// What is saved now, or empty.
    pub current: String,
    /// Shown in grey while the field is empty.
    pub placeholder: String,
    pub width: f32,
    pub secret: bool,
    /// Draw the text in the file's mono face, for commands and paths.
    pub mono: bool,
    pub t: &'a Tokens,
}

/// A text field. Pressed, it opens for typing with a caret: Enter saves it, Esc drops what was
/// typed, and pasting works. Losing focus saves too.
pub fn text_field(s: &Stage, i: Input) -> El {
    let t = i.t;
    let active = s.settings.is_active(&i.id);
    let family = if i.mono { t.fonts.fig } else { t.fonts.body };
    let style = TextStyle::new(13.0, 500.0).family(family);
    let shell = row().w(i.width).h(36.0).px(12.0).items_center().radius(t.radii.ctl).fill(t.fill)
        .border(if active { 2.0 } else { 1.0 }, if active { t.acc } else { t.rule });
    if active {
        let input = text_input(&s.settings.edit, style).single_line().color(t.ink).caret(t.acc, Color { a: 0.3, ..t.acc })
            .placeholder(i.placeholder.clone(), t.ink3).grow(1.0).key(format!("in-{}", i.id))
            .on_edit(edit_access(|s: &mut Stage| &mut s.settings.edit))
            .on_submit(|s: &mut Stage, tx: &mut Tx| {
                if !s.settings.submit() {
                    tx.blur();
                }
            })
            .on_input_key(|s: &mut Stage, e: &KeyEvent, tx: &mut Tx| {
                if e.key == Key::Escape {
                    s.settings.cancel();
                    tx.blur();
                    return true;
                }
                false
            });
        return shell.child(input);
    }
    let shown = if i.secret && !i.current.is_empty() { "\u{2022}".repeat(10) } else { i.current.clone() };
    let (text, ink) = if shown.is_empty() { (i.placeholder.clone(), t.ink3) } else { (shown, t.ink) };
    let body = if i.mono { fig(t, 13.0, 500.0, text, ink) } else { label(t, 13.0, 500.0, text, ink) };
    let (id, target, current, secret) = (i.id.clone(), i.target.clone(), i.current.clone(), i.secret);
    shell.key(format!("fld-{}", i.id)).role("textbox").label(i.id.as_str()).hit().hover(t.raised)
        .child(body.truncate().grow(1.0))
        .on_press(move |s: &mut Stage| s.settings.activate(&id, target.clone(), &current, secret))
}

fn number(v: &toml::Value) -> Option<f64> {
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
}

/// A number with a step either side, and a field in the middle for typing one in.
fn stepper(s: &Stage, t: &Tokens, file: &str, f: &Field, value: Option<toml::Value>) -> El {
    let float = f.kind == FieldKind::Float;
    let n = value.as_ref().and_then(number).unwrap_or(0.0);
    let step = f.step.unwrap_or(if float { 0.1 } else { 1.0 });
    let (min, max) = (f.min, f.max);
    let set = s.settings.is_set(file, &f.key);
    let text = if !set {
        String::new()
    } else if float {
        format!("{}", (n * 1000.0).round() / 1000.0)
    } else {
        format!("{}", n as i64)
    };
    let placeholder = f.default.as_ref().map(display_value).unwrap_or_else(|| "0".into());
    let button = |glyph: &'static str, delta: f64| {
        let (file, key) = (file.to_owned(), f.key.clone());
        tool(t, format!("{file}-{key}-{glyph}"), glyph, if delta < 0.0 { "Less" } else { "More" }, true, move |s: &mut Stage| {
            s.settings.commit();
            let mut next = n + delta;
            if let Some(m) = min { next = next.max(m); }
            if let Some(m) = max { next = next.min(m); }
            if float {
                s.settings.set(&file, &key, (next * 1000.0).round() / 1000.0);
            } else {
                s.settings.set(&file, &key, next.round() as i64);
            }
        })
    };
    row().gap(6.0).items_center()
        .child(button("minus", -step))
        .child(text_field(s, Input {
            id: format!("{file}:{}", f.key),
            target: Target::Num { file: file.to_owned(), key: f.key.clone(), float, min, max },
            current: text,
            placeholder,
            width: 84.0,
            secret: false,
            mono: true,
            t,
        }))
        .child(button("plus", step))
}

fn choices(s: &Stage, t: &Tokens, file: &str, f: &Field, value: Option<&toml::Value>) -> El {
    let selected = value.and_then(|v| f.options.iter().position(|o| o == v));
    let labels: Vec<String> = (0..f.options.len()).map(|i| f.option_label(i)).collect();
    let (file, key, options) = (file.to_owned(), f.key.clone(), f.options.clone());
    let _ = s;
    let write = move |s: &mut Stage, i: usize| {
        s.settings.commit();
        if let Some(v) = to_edit_value(&options[i]) {
            s.settings.set(&file, &key, v);
        }
    };
    if labels.len() <= 4 && labels.iter().all(|l| l.chars().count() <= 12) {
        segmented(t, &format!("{}-seg", f.key), &labels, selected, (labels.len() as f32 * 84.0).clamp(168.0, 340.0), write)
    } else {
        pills(t, &format!("{}-opt", f.key), &labels, selected, 360.0, write)
    }
}

fn list(s: &Stage, t: &Tokens, file: &str, f: &Field, value: Option<&toml::Value>) -> El {
    let items: Vec<String> = match value {
        Some(toml::Value::Array(a)) => a.iter().map(display_value).collect(),
        _ => Vec::new(),
    };
    let mut chips = row().max_w(320.0).flex_wrap().gap(6.0).justify_end();
    for (i, item) in items.iter().enumerate() {
        let (file, key, items) = (file.to_owned(), f.key.clone(), items.clone());
        chips = chips.child(
            row().h(28.0).pl(10.0).pr(4.0).gap(4.0).items_center().radius(t.radii.chip.min(10.0)).fill(t.raised)
                .child(fig(t, 12.0, 500.0, item.as_str(), t.ink))
                .child(tool(t, format!("{}-rm{i}", f.key), "x", "Remove", true, move |s: &mut Stage| {
                    s.settings.commit();
                    let mut items = items.clone();
                    items.remove(i);
                    s.settings.set(&file, &key, string_array(&items));
                })),
        );
    }
    let add = text_field(s, Input {
        id: format!("{file}:{}:add", f.key),
        target: Target::Append { file: file.to_owned(), key: f.key.clone() },
        current: String::new(),
        placeholder: "Add and press Enter".into(),
        width: 280.0,
        secret: false,
        mono: true,
        t,
    });
    let c = col().gap(8.0).items_end();
    if items.is_empty() { c.child(add) } else { c.child(chips).child(add) }
}

fn control(s: &Stage, t: &Tokens, file: &str, f: &Field, value: Option<toml::Value>) -> El {
    match f.kind {
        FieldKind::Bool => {
            let on = value.as_ref().and_then(|v| v.as_bool()).unwrap_or(false);
            let (file, key) = (file.to_owned(), f.key.clone());
            toggle(t, format!("{}-sw", f.key), &f.label, on, move |s: &mut Stage| {
                s.settings.commit();
                s.settings.set(&file, &key, !on);
            })
        }
        FieldKind::Enum => choices(s, t, file, f, value.as_ref()),
        FieldKind::Int | FieldKind::Float => stepper(s, t, file, f, value),
        FieldKind::String => {
            let set = s.settings.is_set(file, &f.key);
            let current = if set { value.as_ref().map(display_value).unwrap_or_default() } else { String::new() };
            text_field(s, Input {
                id: format!("{file}:{}", f.key),
                target: Target::Str { file: file.to_owned(), key: f.key.clone(), secret: f.secret },
                current,
                placeholder: f.default.as_ref().map(display_value).unwrap_or_default(),
                width: 300.0,
                secret: f.secret,
                mono: !f.secret,
                t,
            })
        }
        FieldKind::List => list(s, t, file, f, value.as_ref()),
    }
}

/// One schema field as a row: its name, its help and key, a reset link when the file overrides
/// the default, and the control for its kind.
pub(crate) fn field_row(s: &Stage, t: &Tokens, file: &str, f: &Field, first: bool) -> El {
    let set = s.settings.is_set(file, &f.key);
    let value = s.settings.value(file, &f.key).or_else(|| f.default.clone());
    let mut help = f.help.clone().unwrap_or_default();
    if let (Some(min), Some(max)) = (f.min, f.max) {
        help = format!("{help} From {min} to {max}.").trim().to_owned();
    }
    let mut tag = row().gap(10.0).items_center().child(mono(t, f.key.as_str(), t.ink3));
    if set {
        let (file, key) = (file.to_owned(), f.key.clone());
        tag = tag.child(
            label(t, 12.0, 600.0, "Reset", t.acc).key(format!("{}-reset", f.key)).role("button").label("Reset to default").hit()
                .on_press(move |s: &mut Stage| {
                    s.settings.commit();
                    s.settings.remove(&file, &[key.as_str()]);
                }),
        );
    }
    setting(t, first, &f.label, Some(help), Some(tag), control(s, t, file, f, value))
}

/// The keys "Restore defaults" removes: every field the schema names.
pub fn schema_keys(schema: &Schema) -> Vec<String> {
    schema.section.iter().flat_map(|s| &s.field).map(|f| f.key.clone()).collect()
}

/// An app's options page, drawn from its schema, so an app Settings has never heard of gets one.
pub fn schema_page(s: &Stage, t: &Tokens, app: &AppEntry) -> Vec<El> {
    if let Some(e) = &app.schema_error {
        return vec![notice(t, "This app's settings schema has an error", e.clone(), true)];
    }
    let Some(schema) = &app.schema else { return Vec::new() };
    let file = &app.manifest.config;
    let mut out = Vec::new();
    for section in &schema.section {
        out.push(group(t, &section.title, section.description.as_deref()));
        let mut rows = card(t);
        for (i, f) in section.field.iter().enumerate() {
            rows = rows.child(field_row(s, t, file, f, i == 0));
        }
        out.push(rows);
    }
    out
}

/// An icon beside a line, for the small notes under a page.
pub fn footnote(t: &Tokens, glyph: &str, text: impl Into<String>) -> El {
    row().w_full().gap(8.0).items_center().child(glyph_el(t, glyph, 14.0, t.ink3)).child(el().grow(1.0).min_w(0.0).child(label(t, 12.0, 400.0, text, t.ink3).wrap()))
}

pub use ui::status_pill;
