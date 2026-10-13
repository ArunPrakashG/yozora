//! A page rendered from an app's settings schema, so a new app gets settings without Settings
//! knowing about it.

use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use yozora_config::schema::{Field, FieldKind, display_value};
use yozora_ui::{
    Icon, Theme,
    controls::{segmented, switch},
    icon_sized,
};

use crate::{
    apps::AppEntry,
    doc::{string_array, to_edit_value},
    view::{InputTarget, SettingsView},
    widgets::{card, group_label, notice, text_block},
};

pub fn render(
    view: &mut SettingsView,
    app: &AppEntry,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let theme = Theme::global(cx).clone();
    if let Some(error) = &app.schema_error {
        return notice(Icon::Warning, "This app's settings schema has an error", error.clone(), true, &theme)
            .into_any_element();
    }
    let Some(schema) = app.schema.clone() else {
        return div().into_any_element();
    };
    let file = app.manifest.config.clone();
    let mut page = div().flex().flex_col();
    for (si, section) in schema.section.iter().enumerate() {
        page = page.child(group_label(section.title.clone(), si == 0, &theme));
        if let Some(description) = &section.description {
            page = page.child(
                div()
                    .mt(px(-4.))
                    .mb(px(10.))
                    .ml(px(2.))
                    .text_size(px(12.5))
                    .text_color(theme.ink_2)
                    .child(description.clone()),
            );
        }
        let mut rows = card(&theme);
        for (fi, field) in section.field.iter().enumerate() {
            rows = rows.child(field_row(view, &file, field, fi == 0, &theme, window, cx));
        }
        page = page.child(rows);
    }
    page.into_any_element()
}

fn field_row(
    view: &mut SettingsView,
    file: &str,
    field: &Field,
    first: bool,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let set = view.doc(file).is_set(&field.key);
    let value = view.doc(file).value(&field.key).or_else(|| field.default.clone());
    let key = field.key.clone();
    let file_owned = file.to_string();
    let id = |what: &str| SharedString::from(format!("f-{file}-{}-{what}", field.key));

    let control: AnyElement = match field.kind {
        FieldKind::Bool => {
            let on = value.as_ref().and_then(|v| v.as_bool()).unwrap_or(false);
            switch(id("sw"), on, theme)
                .on_click(cx.listener(move |this, _, _, cx| this.edit(&file_owned, cx, |d| d.set(&key, !on))))
                .into_any_element()
        }
        FieldKind::Enum => {
            let selected = value.as_ref().and_then(|v| field.options.iter().position(|o| o == v));
            let labels: Vec<String> = (0..field.options.len()).map(|i| field.option_label(i)).collect();
            let options = field.options.clone();
            if labels.len() <= 4 && labels.iter().all(|l| l.chars().count() <= 12) {
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                let weak = cx.entity().downgrade();
                div()
                    .w(px((refs.len() as f32 * 74.).clamp(150., 320.)))
                    .child(segmented(id("seg"), &refs, selected.unwrap_or(0), theme, move |i, _, cx| {
                        let (file, key) = (file_owned.clone(), key.clone());
                        let Some(v) = to_edit_value(&options[i]) else { return };
                        weak.update(cx, |this, cx| this.edit(&file, cx, |d| d.set(&key, v))).ok();
                    }))
                    .into_any_element()
            } else {
                // Long or many choices: wrap pills.
                div()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .max_w(px(360.))
                    .gap(px(6.))
                    .children(labels.into_iter().enumerate().map(|(i, label)| {
                        let (file, key, option) = (file_owned.clone(), key.clone(), options[i].clone());
                        yozora_ui::controls::pill(id(&format!("opt{i}")), label, selected == Some(i), theme).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if let Some(v) = to_edit_value(&option) {
                                    this.edit(&file, cx, |d| d.set(&key, v));
                                }
                            }),
                        )
                    }))
                    .into_any_element()
            }
        }
        FieldKind::Int | FieldKind::Float => {
            let float = field.kind == FieldKind::Float;
            let n =
                value.as_ref().and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).unwrap_or(0.);
            let step = field.step.unwrap_or(if float { 0.1 } else { 1. });
            let (min, max) = (field.min, field.max);
            let clamp = move |v: f64| {
                let v = min.map_or(v, |m| v.max(m));
                max.map_or(v, |m| v.min(m))
            };
            let text = if float { format!("{}", (n * 1000.).round() / 1000.) } else { format!("{}", n as i64) };
            let input = view.input(
                &format!("{file}:{}", field.key),
                &display_value(field.default.as_ref().unwrap_or(&toml::Value::Integer(0))),
                true,
                InputTarget::Number { file: file.into(), key: field.key.clone(), float, min, max },
                &text,
                window,
                cx,
            );
            let stepper = |glyph: &'static str, delta: f64, cx: &mut Context<SettingsView>| {
                let (file, key) = (file_owned.clone(), key.clone());
                div()
                    .id(id(glyph))
                    .size(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(9.))
                    .bg(theme.fill_2)
                    .border_1()
                    .border_color(theme.hair)
                    .text_size(px(15.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.fill_3))
                    .child(glyph)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let next = clamp(n + delta);
                        if float {
                            this.edit(&file, cx, |d| d.set(&key, (next * 1000.).round() / 1000.));
                        } else {
                            this.edit(&file, cx, |d| d.set(&key, next.round() as i64));
                        }
                    }))
            };
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(stepper("−", -step, cx))
                .child(div().w(px(80.)).child(input))
                .child(stepper("+", step, cx))
                .into_any_element()
        }
        FieldKind::String => {
            let current = value.as_ref().map(display_value).unwrap_or_default();
            let placeholder = field.default.as_ref().map(display_value).unwrap_or_default();
            let input = view.input(
                &format!("{file}:{}", field.key),
                &placeholder,
                true,
                InputTarget::String { file: file.into(), key: field.key.clone() },
                if set { &current } else { "" },
                window,
                cx,
            );
            div().w(px(280.)).child(input).into_any_element()
        }
        FieldKind::List => {
            let items: Vec<String> = match &value {
                Some(toml::Value::Array(a)) => a.iter().map(display_value).collect(),
                _ => Vec::new(),
            };
            let input = view.input(
                &format!("{file}:{}:add", field.key),
                "Add and press Enter",
                true,
                InputTarget::Append { file: file.into(), key: field.key.clone() },
                "",
                window,
                cx,
            );
            div()
                .flex()
                .flex_col()
                .items_end()
                .gap(px(8.))
                .w(px(300.))
                .when(!items.is_empty(), |el| {
                    el.child(div().flex().flex_wrap().justify_end().gap(px(6.)).children(items.iter().enumerate().map(
                        |(i, item)| {
                            let (file, key, items) = (file_owned.clone(), key.clone(), items.clone());
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .h(px(26.))
                                .pl(px(9.))
                                .pr(px(4.))
                                .rounded(px(8.))
                                .bg(theme.fill_2)
                                .font_family(theme.mono.clone())
                                .text_size(px(11.5))
                                .child(item.clone())
                                .child(
                                    div()
                                        .id(id(&format!("rm{i}")))
                                        .size(px(18.))
                                        .rounded(px(5.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(theme.ink_3)
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme.fill_3).text_color(theme.ink))
                                        .child(icon_sized(Icon::Close, 11.))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let mut items = items.clone();
                                            items.remove(i);
                                            this.edit(&file, cx, |d| d.set(&key, string_array(&items)));
                                        })),
                                )
                        },
                    )))
                })
                .child(input)
                .into_any_element()
        }
    };

    let (reset_file, reset_key) = (file.to_string(), field.key.clone());
    let mut help = field.help.clone().unwrap_or_default();
    if let (Some(min), Some(max)) = (field.min, field.max) {
        help = format!("{help} From {min} to {max}.").trim().to_string();
    }
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .min_h(px(58.))
        .px(px(18.))
        .py(px(10.))
        .when(!first, |el| el.border_t_1().border_color(theme.hair))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(text_block(field.label.clone(), (!help.is_empty()).then(|| help.into()), theme))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .mt(px(3.))
                        .child(
                            div()
                                .font_family(theme.mono.clone())
                                .text_size(px(11.))
                                .text_color(theme.ink_3)
                                .child(field.key.clone()),
                        )
                        .when(set, |el| {
                            el.child(
                                div()
                                    .id(id("reset"))
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.accent)
                                    .cursor_pointer()
                                    .child("Reset")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.edit(&reset_file, cx, |d| d.remove(&[&reset_key]))
                                    })),
                            )
                        }),
                ),
        )
        .child(control)
        .into_any_element()
}
