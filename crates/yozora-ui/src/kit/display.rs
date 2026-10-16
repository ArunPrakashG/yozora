//! Display components: Row, Section, Title, Badge, Progress, Empty and Note.

use gpui::{
    AnyElement, Div, ElementId, FontWeight, InteractiveElement, ParentElement, SharedString, Stateful, Styled, div,
    prelude::FluentBuilder, px, relative,
};
use yozora_module::{Node, Size, Tone};

use super::controls::{row_height, tone_color, tone_soft};
use crate::{
    icons::{Icon, icon},
    theme::{RADIUS_ROW, Theme},
};

/// How a row shares width between label and value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowLayout {
    /// Whether the label column may shrink below its content.
    pub label_shrinks: bool,
    /// Whether the value cell may shrink and truncate.
    pub value_shrinks: bool,
    /// The value cell's maximum width as a percentage of the row.
    pub value_max_fraction_percent: u8,
}

/// The truncation rule. `row()` reads these constants: the label never shrinks below its content
/// while the value is capped at 40% of the row and truncates.
pub const ROW_LAYOUT: RowLayout =
    RowLayout { label_shrinks: false, value_shrinks: true, value_max_fraction_percent: 40 };

/// Everything a row shows.
#[derive(Clone, Debug)]
pub struct RowProps {
    pub icon: Option<Icon>,
    pub title: SharedString,
    pub subtitle: Option<SharedString>,
    pub value: Option<SharedString>,
    pub badge: Option<(SharedString, Tone)>,
    pub tone: Tone,
    pub size: Size,
    pub clickable: bool,
}

/// Maps a module-supplied icon name to a built-in icon. Unknown names fall back to the neutral
/// generic [`Icon::Dot`].
pub fn icon_for(name: &str) -> Icon {
    const KNOWN: &[Icon] = &[
        Icon::Logo,
        Icon::Wifi,
        Icon::WifiOff,
        Icon::Ethernet,
        Icon::NetworkOff,
        Icon::Vpn,
        Icon::VolumeHigh,
        Icon::VolumeLow,
        Icon::VolumeMute,
        Icon::Mic,
        Icon::MicOff,
        Icon::Speaker,
        Icon::Headphones,
        Icon::Display,
        Icon::Bluetooth,
        Icon::BluetoothOff,
        Icon::Battery,
        Icon::BatteryCharging,
        Icon::Bell,
        Icon::Moon,
        Icon::Appearance,
        Icon::Check,
        Icon::ChevronLeft,
        Icon::ChevronRight,
        Icon::Dot,
        Icon::Play,
        Icon::Pause,
        Icon::Previous,
        Icon::Next,
        Icon::Close,
        Icon::Settings,
        Icon::Lock,
        Icon::Power,
        Icon::Keyboard,
        Icon::Mouse,
        Icon::Gamepad,
        Icon::Phone,
        Icon::Terminal,
        Icon::Globe,
        Icon::Package,
        Icon::Sync,
        Icon::Cloud,
        Icon::ControlCenter,
        Icon::Music,
        Icon::Sun,
        Icon::Bolt,
        Icon::Leaf,
        Icon::Gauge,
        Icon::Calendar,
        Icon::App,
        Icon::Warning,
        Icon::Bar,
        Icon::Motion,
        Icon::Modules,
        Icon::Info,
        Icon::Search,
        Icon::File,
        Icon::Grip,
        Icon::Plus,
        Icon::Reset,
        Icon::External,
        Icon::Code,
        Icon::ArrowRight,
        Icon::CapsLock,
        Icon::Spinner,
        Icon::Clock,
        Icon::Alert,
        Icon::Window,
        Icon::Folder,
        Icon::Clipboard,
        Icon::Calculator,
        Icon::Smile,
        Icon::Logout,
        Icon::Star,
        Icon::Shell,
        Icon::Link,
        Icon::Image,
    ];
    KNOWN.iter().copied().find(|i| i.name() == name).unwrap_or(Icon::Dot)
}

/// The component name for a node. The `match` has no wildcard arm on purpose: adding a `Node`
/// variant fails to compile here until it is given a component.
pub fn component_name(node: &Node) -> &'static str {
    match node {
        Node::Section { .. } => "section",
        Node::Row { .. } => "row",
        Node::Toggle { .. } => "toggle",
        Node::Slider { .. } => "slider",
        Node::Progress { .. } => "progress",
        Node::Buttons { .. } => "buttons",
        Node::Empty { .. } => "empty",
        Node::Note { .. } => "note",
    }
}

/// The popover title bar: title on the left, optional caption right-aligned.
pub fn title(text: impl Into<SharedString>, caption: Option<&str>, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(8.))
        .pt(px(4.))
        .pb(px(12.))
        .child(div().min_w_0().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(text.into()))
        .when_some(caption.map(|c| SharedString::from(c.to_string())), |el, c| {
            el.child(div().flex_none().text_size(px(12.)).text_color(theme.ink_3).child(c))
        })
}

/// A section: an optional uppercase heading, then its rows.
pub fn section(heading: Option<&str>, rows: Vec<AnyElement>, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .mt(px(8.))
        .when_some(heading.map(|h| SharedString::from(h.to_uppercase())), |el, h| {
            el.child(
                div()
                    .px(px(8.))
                    .pt(px(8.))
                    .pb(px(4.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.ink_3)
                    .child(h),
            )
        })
        .children(rows)
}

/// A pill badge with a soft tone background.
pub fn badge(text: impl Into<SharedString>, tone: Tone, theme: &Theme) -> Div {
    let (bg, fg) = if tone == Tone::Neutral {
        (theme.fill_3, theme.ink_2)
    } else {
        (tone_soft(tone, theme), tone_color(tone, theme))
    };
    div().flex_none().px(px(7.)).py(px(1.)).rounded_full().bg(bg).text_color(fg).text_size(px(11.)).child(text.into())
}

/// A list row. Label shrink and value truncation follow [`ROW_LAYOUT`].
pub fn row(id: impl Into<ElementId>, props: RowProps, theme: &Theme) -> Stateful<Div> {
    let RowProps { icon: glyph, title, subtitle, value, badge: tag, tone, size, clickable } = props;
    let tile_bg = if tone == Tone::Accent { theme.accent_soft } else { theme.fill_2 };
    let tile_fg = tone_color(tone, theme);
    let label = div()
        .flex()
        .flex_col()
        .min_w_0()
        .when(!ROW_LAYOUT.label_shrinks, |el| el.flex_none())
        .when(ROW_LAYOUT.label_shrinks, |el| el.flex_1())
        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title))
        .when_some(subtitle, |el, s| el.child(div().text_size(px(12.)).text_color(theme.ink_3).child(s)));
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(12.))
        .w_full()
        .min_h(row_height(size))
        .px(px(8.))
        .py(px(8.))
        .rounded(RADIUS_ROW)
        .when(clickable, |el| el.cursor_pointer().hover(|s| s.bg(theme.fill_2)))
        .when_some(glyph, |el, g| {
            el.child(
                div()
                    .size(px(28.))
                    .flex_none()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(tile_bg)
                    .text_color(tile_fg)
                    .child(icon(g)),
            )
        })
        .child(label)
        // Spacer so the value sits at the end; it is the only thing that gives way.
        .child(div().flex_1().min_w_0())
        .when_some(value, |el, v| {
            el.child(
                div()
                    .when(ROW_LAYOUT.value_shrinks, |el| el.min_w_0())
                    .max_w(relative(f32::from(ROW_LAYOUT.value_max_fraction_percent) / 100.))
                    .truncate()
                    .text_size(px(12.))
                    .text_color(theme.ink_2)
                    .child(v),
            )
        })
        .when_some(tag, |el, (text, tone)| el.child(badge(text, tone, theme)))
}

/// A progress row: title, optional subtitle and a 4 px track filled to `value` (0..=1).
pub fn progress(title: impl Into<SharedString>, subtitle: Option<SharedString>, value: f32, theme: &Theme) -> Div {
    let value = if value.is_finite() { value.clamp(0., 1.) } else { 0. };
    div()
        .flex()
        .flex_col()
        .w_full()
        .px(px(8.))
        .py(px(6.))
        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).truncate().child(title.into()))
        .when_some(subtitle, |el, s| el.child(div().text_size(px(12.)).text_color(theme.ink_3).truncate().child(s)))
        .child(
            div()
                .mt(px(6.))
                .h(px(4.))
                .w_full()
                .rounded(px(2.))
                .bg(theme.fill_2)
                .overflow_hidden()
                .child(div().h_full().w(relative(value)).bg(theme.accent)),
        )
}

/// An empty state: a title and an optional line saying why.
pub fn empty(title: impl Into<SharedString>, message: Option<SharedString>, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .py(px(24.))
        .px(px(16.))
        .gap(px(2.))
        .text_size(px(13.))
        .text_color(theme.ink_3)
        .child(div().text_color(theme.ink_2).font_weight(FontWeight::MEDIUM).child(title.into()))
        .when_some(message, |el, m| el.child(div().child(m)))
}

/// A small caption-style note.
pub fn note(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div().px(px(8.)).mt(px(8.)).text_size(px(12.)).text_color(theme.ink_3).child(text.into())
}
