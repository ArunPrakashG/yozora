//! Hover tooltips, in their own `xdg_popup` surface because the bar is only as tall as its
//! buttons. A tooltip never grabs, takes focus or takes input, so the pointer passes through.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use gpui::{
    AnyWindowHandle, App, Bounds, Context, IntoElement, ParentElement, Pixels, Render, SharedString, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, popup::*, prelude::*, px, size,
};
use yozora_ui::Theme;

/// How long the pointer rests before the tooltip shows.
const DELAY: Duration = Duration::from_millis(450);
/// How often a showing tooltip checks the pointer is still over its module.
const POLL: Duration = Duration::from_millis(100);
/// Room around the card for its shadow.
const MARGIN: Pixels = px(14.);
const MAX_WIDTH: f32 = 360.;
const LINE_HEIGHT: f32 = 17.;
const PAD_X: f32 = 10.;
const PAD_Y: f32 = 6.;

struct Tooltip {
    text: SharedString,
    width: Pixels,
}

thread_local! {
    /// The open tooltip, and a count that goes up on every hover change so a pending tooltip
    /// can tell the pointer moved on.
    static OPEN: RefCell<Option<AnyWindowHandle>> = const { RefCell::new(None) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Shows `text` under `anchor` (in `parent`'s coordinates) after the hover delay, or hides
/// the tooltip when `hovered` is false.
pub fn hover(hovered: bool, text: String, anchor: Rc<Cell<Bounds<Pixels>>>, window: &Window, cx: &mut App) {
    let generation = GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    hide(cx);
    if !hovered || text.trim().is_empty() {
        return;
    }
    let parent = window.window_handle();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(DELAY).await;
        if GENERATION.with(Cell::get) != generation {
            return;
        }
        cx.update(|cx| match open(parent, anchor.get(), text, cx) {
            Ok(handle) => OPEN.with_borrow_mut(|o| *o = Some(handle)),
            Err(e) => tracing::warn!("could not open a tooltip: {e:#}"),
        });
        // GPUI sends no hover change when the pointer leaves the bar's surface altogether, so
        // watch for that while the tooltip shows.
        loop {
            cx.background_executor().timer(POLL).await;
            if GENERATION.with(Cell::get) != generation {
                return;
            }
            let inside = parent
                .update(cx, |_, window, _| {
                    window.is_window_hovered() && anchor.get().contains(&window.mouse_position())
                })
                .unwrap_or(false);
            if !inside {
                GENERATION.with(|g| g.set(g.get() + 1));
                cx.update(hide);
                return;
            }
        }
    })
    .detach();
}

/// Hides the tooltip, if one is showing.
pub fn hide(cx: &mut App) {
    if let Some(handle) = OPEN.with_borrow_mut(Option::take) {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

fn open(
    parent: AnyWindowHandle,
    anchor: Bounds<Pixels>,
    text: String,
    cx: &mut App,
) -> anyhow::Result<AnyWindowHandle> {
    // A rough measure of the text: 6.8 px a character at 12 px is right for Geist and Inter.
    let lines: Vec<&str> = text.lines().collect();
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
    let width = (longest * 6.8 + PAD_X * 2. + 2.).clamp(40., MAX_WIDTH);
    let wraps: usize = lines
        .iter()
        .map(|l| ((l.chars().count() as f32 * 6.8) / (MAX_WIDTH - PAD_X * 2.)).ceil().max(1.) as usize)
        .sum();
    let height = wraps.max(1) as f32 * LINE_HEIGHT + PAD_Y * 2. + 2.;
    let window_size = size(px(width) + MARGIN * 2., px(height) + MARGIN * 2.);
    let handle = cx.open_window(
        WindowOptions {
            titlebar: None,
            focus: false,
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin: point(px(0.), px(0.)), size: window_size })),
            window_background: WindowBackgroundAppearance::Transparent,
            inactive_frame_interval: None,
            kind: WindowKind::AnchoredPopup(PopupOptions {
                parent,
                anchor_rect: anchor,
                anchor: PopupAnchor::Bottom,
                gravity: PopupGravity::Bottom,
                constraint_adjustment: PopupConstraintAdjustment::SLIDE_X,
                offset: point(px(0.), px(4.) - MARGIN),
                grab: false,
            }),
            ..Default::default()
        },
        |window, cx| {
            window.set_input_region(Some(&[]));
            cx.new(|_| Tooltip { text: text.into(), width: px(width) })
        },
    )?;
    Ok(handle.into())
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx);
        div().size_full().p(MARGIN).child(
            div()
                .w(self.width)
                .px(px(PAD_X))
                .py(px(PAD_Y))
                .rounded(px(8.))
                .bg(theme.glass_pop)
                .border_1()
                .border_color(theme.hair_2)
                .shadow(theme.popover_shadow())
                .text_color(theme.ink)
                .text_size(px(12.))
                .line_height(px(LINE_HEIGHT))
                .font_family(theme.font.clone())
                .child(self.text.clone()),
        )
    }
}
