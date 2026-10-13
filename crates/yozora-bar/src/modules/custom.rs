//! User-defined modules: a command's output as text, with an optional click action.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, IntoElement, ParentElement, StatefulInteractiveElement, Styled, canvas, div, prelude::*, px,
};
use yozora_ui::Theme;

use crate::{
    app::Config,
    bar::bi,
    services::{Services, run_shell},
    tooltip,
};

pub fn render(name: &str, theme: &Theme, cx: &App) -> Option<AnyElement> {
    let custom = Services::get(cx).custom.as_ref()?.read(cx);
    let output = custom.state.outputs.get(name)?;
    if output.text.is_empty() && output.error.is_none() {
        return None;
    }
    let on_click = Config::get(cx).settings.bar.custom.get(name).and_then(|c| c.on_click.clone());
    let (text, color) = match &output.error {
        Some(_) if output.text.is_empty() => ("⚠".to_string(), theme.danger),
        _ => (output.text.clone(), theme.ink),
    };
    // The tooltip, or the error when there's no text to show it next to.
    let tip = match &output.error {
        Some(e) if output.tooltip.is_empty() => e.clone(),
        _ => output.tooltip.clone(),
    };
    let fill_2 = theme.fill_2;
    let bounds = Rc::new(Cell::new(gpui::Bounds::default()));
    let track = bounds.clone();
    Some(
        bi().id(gpui::ElementId::Name(format!("custom/{name}").into()))
            .relative()
            .text_color(color)
            .text_size(px(12.5))
            .when(on_click.is_some(), |el| el.cursor_pointer().hover(move |s| s.bg(fill_2)))
            .child(canvas(move |b, _, _| track.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .child(div().max_w(px(260.)).truncate().child(text))
            .when(!tip.is_empty(), |el| {
                el.on_hover(move |hovered, window, cx| {
                    tooltip::hover(*hovered, tip.clone(), bounds.clone(), window, cx)
                })
            })
            .when_some(on_click, |el, command| {
                el.on_click(move |_, _, cx| {
                    tooltip::hide(cx);
                    run_shell(&command)
                })
            })
            .into_any_element(),
    )
}
