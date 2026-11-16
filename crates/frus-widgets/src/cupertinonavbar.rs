// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`CupertinoNavigationBar`]: the Apple-style bar along the top of a page — a leading
//! control, a centred title, a trailing control, a back chevron with the previous page's
//! title, over a translucent background that blurs what scrolls under it (milestone 812).

use std::cell::{OnceCell, RefCell};

use frus_core::{Color, InsetsDirectional, Path, Point, Rect, Scene, Size, Stroke, StrokeCap};
use frus_layout::{Dimension, Style};

use crate::cupertinobutton::CupertinoButton;
use crate::cupertinotheme::CupertinoTheme;
use crate::customlayout::{ChildAxis, ChildConstraints, CustomMultiChildLayout};
use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// The bar's height, under the status bar (`nav_bar.dart:46`): a finger's.
pub const CUPERTINO_NAV_BAR_HEIGHT: f32 = 44.0;
/// What a large title adds under it (`:50`).
pub const CUPERTINO_NAV_BAR_LARGE_TITLE_EXTENSION: f32 = 52.0;
/// The room at either end (`:62`).
const EDGE: f32 = 16.0;
/// The room under a large title (`:64`).
const LARGE_TITLE_BOTTOM: f32 = 8.0;
/// The least room between the title and the controls either side of it (`:1852`).
const MIDDLE_SPACING: f32 = 6.0;
/// The back button's least width (`:66`).
const BACK_TAP_WIDTH: f32 = 50.0;
/// The longest previous title shown beside the chevron; a longer one says "Back" (`:2384`).
const PREVIOUS_TITLE_MAX: usize = 12;
/// The blur under a translucent bar (`:255`).
const BLUR_SIGMA: f32 = 10.0;
/// The hairline under the bar unless told (`:106`).
pub const CUPERTINO_NAV_BAR_BORDER: Color = Color::from_argb_u32(0x4D000000);

/// What a bar was told, until it is built against its theme.
struct Parts<Msg> {
    leading: Option<Box<dyn Widget<Msg>>>,
    middle: Option<Box<dyn Widget<Msg>>>,
    trailing: Option<Box<dyn Widget<Msg>>>,
    large_title: Option<Box<dyn Widget<Msg>>>,
    back: Option<Msg>,
    previous_page_title: Option<String>,
    background_color: Option<Color>,
    border: Option<Option<Color>>,
    padding: Option<InsetsDirectional>,
    blur: bool,
}

/// **The Apple-style navigation bar**: a leading control, a title centred across the whole
/// bar — moved aside only where the controls would cover it — and a trailing control.
///
/// ```
/// use frus_widgets::{CupertinoButton, CupertinoNavigationBar};
///
/// #[derive(Clone)]
/// enum Msg { Back, Edit }
/// let _bar: CupertinoNavigationBar<Msg> = CupertinoNavigationBar::new()
///     .title("Inbox")
///     .back(Msg::Back)
///     .previous_page_title("Mailboxes")
///     .trailing(CupertinoButton::new("Edit").padding(Default::default()).on_press(Msg::Edit));
/// ```
///
/// [`back`](Self::back) puts the back chevron at the start, with the previous page's title
/// beside it when it is short enough, "Back" when it is not. [`large`](Self::large) is the
/// bar with a large title under it, flush with the start.
///
/// Its background is the Apple-style theme's bar colour, nearly opaque: what passes under
/// it is blurred — the renderer's backdrop blur — and shows through a little. A hairline
/// runs along its bottom. The status bar's room is added above it.
pub struct CupertinoNavigationBar<Msg = crate::callback::Callback> {
    parts: RefCell<Option<Parts<Msg>>>,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> Default for CupertinoNavigationBar<Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Msg: Clone + 'static> CupertinoNavigationBar<Msg> {
    /// An empty bar.
    pub fn new() -> Self {
        Self {
            parts: RefCell::new(Some(Parts {
                leading: None,
                middle: None,
                trailing: None,
                large_title: None,
                back: None,
                previous_page_title: None,
                background_color: None,
                border: None,
                padding: None,
                blur: true,
            })),
            built: OnceCell::new(),
        }
    }

    /// A bar with a **large title** under it, flush with the start: the top of a list.
    pub fn large(title: impl Into<String>) -> Self {
        Self::new().large_title(crate::Text::new(title.into()).heading())
    }

    fn with(self, f: impl FnOnce(&mut Parts<Msg>)) -> Self {
        if let Some(parts) = self.parts.borrow_mut().as_mut() {
            f(parts);
        }
        self
    }

    /// Its title, in the bar's title style.
    #[must_use]
    pub fn title(self, title: impl Into<String>) -> Self {
        self.middle(crate::Text::new(title.into()).heading())
    }

    /// What is centred in it: any widget, in the bar's title style.
    #[must_use]
    pub fn middle(self, middle: impl Widget<Msg> + 'static) -> Self {
        let middle: Box<dyn Widget<Msg>> = Box::new(middle);
        self.with(|p| p.middle = Some(middle))
    }

    /// What sits at its start, 16 pixels in, in the bar's action style. It takes the place
    /// of the back chevron.
    #[must_use]
    pub fn leading(self, leading: impl Widget<Msg> + 'static) -> Self {
        let leading: Box<dyn Widget<Msg>> = Box::new(leading);
        self.with(|p| p.leading = Some(leading))
    }

    /// What sits at its end, 16 pixels in, in the bar's action style.
    #[must_use]
    pub fn trailing(self, trailing: impl Widget<Msg> + 'static) -> Self {
        let trailing: Box<dyn Widget<Msg>> = Box::new(trailing);
        self.with(|p| p.trailing = Some(trailing))
    }

    /// A large title under the bar: any widget, in the large title style.
    #[must_use]
    pub fn large_title(self, title: impl Widget<Msg> + 'static) -> Self {
        let title: Box<dyn Widget<Msg>> = Box::new(title);
        self.with(|p| p.large_title = Some(title))
    }

    /// The back chevron at its start, sending `message` when pressed.
    #[must_use]
    pub fn back(self, message: impl Into<Msg>) -> Self {
        let message = message.into();
        self.with(|p| p.back = Some(message))
    }

    /// The previous page's title, beside the back chevron. Longer than twelve characters,
    /// "Back" is said instead.
    #[must_use]
    pub fn previous_page_title(self, title: impl Into<String>) -> Self {
        let title = title.into();
        self.with(|p| p.previous_page_title = Some(title))
    }

    /// Its background, in place of the theme's bar colour. Opaque, nothing is blurred.
    #[must_use]
    pub fn background_color(self, color: Color) -> Self {
        self.with(|p| p.background_color = Some(color))
    }

    /// The hairline along its bottom, in `color`.
    #[must_use]
    pub fn border_color(self, color: Color) -> Self {
        self.with(|p| p.border = Some(Some(color)))
    }

    /// No hairline along its bottom.
    #[must_use]
    pub fn no_border(self) -> Self {
        self.with(|p| p.border = Some(None))
    }

    /// The room at its start and its end, in place of 16 pixels each.
    #[must_use]
    pub fn padding(self, padding: InsetsDirectional) -> Self {
        self.with(|p| p.padding = Some(padding))
    }

    /// Whether a translucent background blurs what is under it. On by default; off, the
    /// background is only translucent, and costs nothing more than a fill.
    #[must_use]
    pub fn enable_background_filter_blur(self, blur: bool) -> Self {
        self.with(|p| p.blur = blur)
    }
}

/// Hands `child` a default text style.
fn texted<Msg: 'static>(
    style: frus_core::TextStyle,
    icon: Option<(Color, f32)>,
    child: impl Widget<Msg> + 'static,
) -> crate::Themed<Msg> {
    crate::Themed::tweak(
        move |t: &mut Theme| {
            t.widgets.text.style = style;
            if let Some((color, size)) = icon {
                t.widgets.icon.color = Some(color);
                t.widgets.icon.size = Some(size);
            }
        },
        child,
    )
}

/// Keeps the text inside `child` to one line, ending in an ellipsis where it is cut.
fn single_line<Msg: 'static>(child: impl Widget<Msg> + 'static) -> crate::Themed<Msg> {
    crate::Themed::tweak(
        |t: &mut Theme| {
            t.widgets.text.soft_wrap = Some(false);
            t.widgets.text.overflow = Some(frus_core::TextOverflow::Ellipsis);
        },
        child,
    )
}

/// The back chevron, drawn in the colour of the text around it — two strokes meeting at a
/// point, mirrored in a right-to-left script (`nav_bar.dart:2326`).
struct BackChevron;

/// The chevron's box: the glyph and the room either side of it (`:2337`).
const CHEVRON_W: f32 = 12.0;
const CHEVRON_H: f32 = 21.0;
const CHEVRON_START: f32 = 6.0;
const CHEVRON_END: f32 = 2.0;
const CHEVRON_STROKE: f32 = 3.0;

impl<Msg> Widget<Msg> for BackChevron {
    fn style(&self) -> Style {
        Style {
            width: Dimension::Length(CHEVRON_START + CHEVRON_W + CHEVRON_END),
            height: Dimension::Length(CHEVRON_H),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let colour = theme
            .widgets
            .text
            .style
            .color
            .unwrap_or(CupertinoTheme::of(theme).primary_color);
        let rtl = theme.direction == frus_core::TextDirection::Rtl;
        let inset = CHEVRON_STROKE * 0.5;
        // In left-to-right reading the point is on the left.
        let (tip_x, back_x) = if rtl {
            (
                bounds.x + bounds.width - CHEVRON_START - inset,
                bounds.x + CHEVRON_END + inset,
            )
        } else {
            (
                bounds.x + CHEVRON_START + inset,
                bounds.x + bounds.width - CHEVRON_END - inset,
            )
        };
        let cy = bounds.y + bounds.height * 0.5;
        let half = CHEVRON_H * 0.5 - inset;
        let stroke =
            Stroke::new(colour.fade(status.opacity), CHEVRON_STROKE).with_cap(StrokeCap::Round);
        for end in [cy - half, cy + half] {
            let arm = Path::new()
                .move_to(Point::new(tip_x, cy))
                .line_to(Point::new(back_x, end));
            scene.paint_path(&arm, None, Some(stroke));
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }
}

/// The back button: the chevron and the previous title, dimming when pressed
/// (`nav_bar.dart:2288`).
fn back_button<Msg: Clone + 'static>(
    message: Msg,
    previous: Option<String>,
    style: frus_core::TextStyle,
) -> CupertinoButton<Msg> {
    let back = crate::localizations::of().back_button_label().to_string();
    let label = previous.map(|title| {
        if title.chars().count() > PREVIOUS_TITLE_MAX {
            back.clone()
        } else {
            title
        }
    });
    let mut row = crate::Row::new()
        .main_axis_size(crate::MainAxisSize::Min)
        .cross_axis_alignment(crate::Align::Center)
        .child(crate::SizedBox::<Msg>::empty().width(8.0))
        .child(BackChevron);
    if let Some(label) = label {
        row = row
            .child(crate::SizedBox::<Msg>::empty().width(6.0))
            .child(crate::Flexible::new(
                crate::Text::new(label).no_wrap().ellipsis(),
            ));
    }
    let button =
        CupertinoButton::with_child(crate::ExcludeSemantics::new(texted(style, None, row)))
            .semantics_label(back)
            .padding(frus_core::Insets::ZERO)
            .minimum_size(Size::new(BACK_TAP_WIDTH, CUPERTINO_NAV_BAR_HEIGHT))
            .on_press(message);
    match style.color {
        Some(colour) => button.foreground_color(colour),
        None => button,
    }
}

/// Lays the three out as the reference's toolbar does (`navigation_toolbar.dart`): the
/// leading at the start and the trailing at the end, both the bar's height; the middle
/// centred across the whole width, and moved aside only where it would run into either.
pub(crate) fn toolbar_layout(size: Size, layout: &mut crate::customlayout::ChildLayout<'_>) {
    let rtl = layout.theme().direction == frus_core::TextDirection::Rtl;
    let tall = ChildConstraints {
        width: ChildAxis::AtMost(size.width),
        height: ChildAxis::Exactly(size.height),
    };
    let mut leading_w = 0.0;
    if layout.has_child("leading") {
        leading_w = layout.layout_child("leading", tall).width;
        let x = if rtl { size.width - leading_w } else { 0.0 };
        layout.position_child("leading", Point::new(x, 0.0));
    }
    let mut trailing_w = 0.0;
    if layout.has_child("trailing") {
        trailing_w = layout.layout_child("trailing", tall).width;
        let x = if rtl { 0.0 } else { size.width - trailing_w };
        layout.position_child("trailing", Point::new(x, 0.0));
    }
    if layout.has_child("middle") {
        let spacing = |w: f32| if w > 0.0 { w + MIDDLE_SPACING } else { 0.0 };
        let room = (size.width - spacing(leading_w) - spacing(trailing_w)).max(0.0);
        let m = layout.layout_child(
            "middle",
            ChildConstraints {
                width: ChildAxis::AtMost(room),
                height: ChildAxis::AtMost(size.height),
            },
        );
        let start_margin = spacing(leading_w);
        let end_margin = spacing(trailing_w);
        let mut x = (size.width - m.width) * 0.5;
        if x + m.width > size.width - end_margin {
            x = size.width - end_margin - m.width;
        } else if x < start_margin {
            x = start_margin;
        }
        let x = if rtl { size.width - m.width - x } else { x };
        layout.position_child("middle", Point::new(x, (size.height - m.height) * 0.5));
    }
}

fn build<Msg: Clone + 'static>(p: Parts<Msg>, theme: &Theme) -> Box<dyn Widget<Msg>> {
    let c = CupertinoTheme::of(theme);
    let media = crate::MediaQuery::of();
    let background = p.background_color.unwrap_or(c.bar_background_color);
    let border = p.border.unwrap_or(Some(CUPERTINO_NAV_BAR_BORDER));
    let hairline = 1.0 / media.device_pixel_ratio.max(1.0);
    let padding = p
        .padding
        .unwrap_or(InsetsDirectional::new(0.0, EDGE, 0.0, EDGE));
    let action = c.text_theme.nav_action_text_style;
    let action_icon = Some((c.primary_color, 32.0));

    let leading: Option<Box<dyn Widget<Msg>>> = match (p.leading, p.back) {
        (Some(leading), _) => Some(Box::new(crate::Padding::new(
            InsetsDirectional::only_start(padding.start),
            crate::Aligned::new(
                frus_core::AlignmentDirectional::CENTER_START,
                texted(action, action_icon, leading),
            )
            .width_factor(1.0),
        ))),
        (None, Some(message)) => Some(Box::new(back_button(
            message,
            p.previous_page_title,
            action,
        ))),
        (None, None) => None,
    };
    let trailing: Option<Box<dyn Widget<Msg>>> = p.trailing.map(|trailing| {
        Box::new(crate::Padding::new(
            InsetsDirectional::only_end(padding.end),
            crate::Aligned::new(
                frus_core::AlignmentDirectional::CENTER_END,
                texted(action, action_icon, trailing),
            )
            .width_factor(1.0),
        )) as Box<dyn Widget<Msg>>
    });
    let middle = p
        .middle
        .map(|middle| single_line(texted(c.text_theme.nav_title_text_style, None, middle)));

    let mut toolbar = CustomMultiChildLayout::new(toolbar_layout).height(CUPERTINO_NAV_BAR_HEIGHT);
    if let Some(leading) = leading {
        toolbar = toolbar.child("leading", leading);
    }
    if let Some(middle) = middle {
        toolbar = toolbar.child("middle", middle);
    }
    if let Some(trailing) = trailing {
        toolbar = toolbar.child("trailing", trailing);
    }

    let safe = media.padding;
    let large = p.large_title.is_some();
    let mut column = crate::Column::new()
        .cross_axis_alignment(crate::Align::Stretch)
        .child(crate::SizedBox::<Msg>::empty().height(safe.top))
        .child(toolbar);
    if let Some(title) = p.large_title {
        column = column.child(
            crate::SizedBox::new(crate::Padding::new(
                InsetsDirectional::new(0.0, 0.0, LARGE_TITLE_BOTTOM, padding.start),
                crate::Aligned::new(
                    frus_core::AlignmentDirectional::BOTTOM_START,
                    single_line(texted(c.text_theme.nav_large_title_text_style, None, title)),
                ),
            ))
            .height(CUPERTINO_NAV_BAR_LARGE_TITLE_EXTENSION),
        );
    }
    let height = safe.top
        + CUPERTINO_NAV_BAR_HEIGHT
        + if large {
            CUPERTINO_NAV_BAR_LARGE_TITLE_EXTENSION
        } else {
            0.0
        };

    let surface = crate::CustomPaint::new(0.0, 0.0, move |scene, bounds, _| {
        scene.fill_rect(bounds, background);
        if let Some(line) = border {
            scene.fill_rect(
                Rect::new(
                    bounds.x,
                    bounds.y + bounds.height - hairline,
                    bounds.width,
                    hairline,
                ),
                line,
            );
        }
    });
    let bar = crate::SizedBox::expand(
        crate::Stack::new()
            .fit(crate::StackFit::Expand)
            .layer(crate::Positioned::fill(surface))
            .layer(crate::Padding::new(
                frus_core::Insets::new(0.0, safe.right, 0.0, safe.left),
                column,
            )),
    )
    .height(height);
    Box::new(
        crate::ClipRect::new().child(
            crate::BackdropFilter::blur(BLUR_SIGMA)
                .enabled(p.blur && background.a < 1.0)
                .child(bar),
        ),
    )
}

impl<Msg: Clone + 'static> Widget<Msg> for CupertinoNavigationBar<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes::WIDTH
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.built.get_or_init(|| {
            let Some(parts) = self.parts.borrow_mut().take() else {
                return Vec::new();
            };
            vec![Box::new(crate::themebuilder::ThemeBuilder::boxed(
                move |theme| build(parts, theme),
            ))]
        })
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "CupertinoNavigationBar"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cupertinobutton::AllPrimitives;
    use crate::{build_ui, Container, MediaQuery, Runtime, Ui};
    use frus_core::{Insets, Primitive};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Back,
    }

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);

    fn block(width: f32, colour: Color) -> Container<Msg> {
        Container::new().width(width).height(20.0).color(colour)
    }

    fn painted(ui: &Ui<Msg>, colour: Color) -> Rect {
        ui.scene()
            .all()
            .into_iter()
            .find_map(|p| match p {
                Primitive::Rect { rect, color, .. } if *color == colour => Some(*rect),
                _ => None,
            })
            .expect("painted")
    }

    /// The three laid out by the bar's rule across a 300-pixel bar.
    fn toolbar(leading: f32, middle: f32, trailing: f32, theme: &Theme) -> (Rect, Rect, Rect) {
        let root = CustomMultiChildLayout::new(toolbar_layout)
            .height(CUPERTINO_NAV_BAR_HEIGHT)
            .child("leading", block(leading, RED))
            .child("middle", block(middle, GREEN))
            .child("trailing", block(trailing, BLUE));
        let ui = build_ui(&root, Size::new(300.0, 44.0), &Runtime::default(), theme);
        (painted(&ui, RED), painted(&ui, GREEN), painted(&ui, BLUE))
    }

    /// **The title is centred across the whole bar**, not in what the controls leave: a
    /// wide leading and a narrow trailing do not move it.
    #[test]
    fn the_title_is_centred_across_the_whole_bar() {
        let (leading, middle, trailing) = toolbar(80.0, 60.0, 20.0, &Theme::light());
        assert_eq!(leading.x, 0.0);
        assert_eq!(trailing.x + trailing.width, 300.0);
        assert_eq!(middle.x, 120.0);
        assert_eq!(middle.y, 12.0);
    }

    /// **Only where a control would cover it is it moved aside**, 6 pixels clear of it.
    #[test]
    fn the_title_moves_aside_only_where_it_must() {
        let (_, middle, _) = toolbar(110.0, 100.0, 20.0, &Theme::light());
        assert_eq!(middle.x, 116.0);
        let (_, middle, _) = toolbar(20.0, 100.0, 110.0, &Theme::light());
        assert_eq!(middle.x + middle.width, 300.0 - 116.0);
    }

    /// **Right to left, everything is mirrored**: the leading on the right.
    #[test]
    fn right_to_left_the_bar_is_mirrored() {
        let (leading, middle, trailing) = toolbar(110.0, 100.0, 20.0, &Theme::light().rtl());
        assert_eq!(leading.x + leading.width, 300.0);
        assert_eq!(trailing.x, 0.0);
        assert_eq!(middle.x + middle.width, 300.0 - 116.0);
    }

    /// The bar laid out in a 320 × 480 window with a 20-pixel status bar.
    fn bar(bar: CupertinoNavigationBar<Msg>, theme: &Theme) -> Ui<Msg> {
        let mut media = MediaQuery::new(Size::new(320.0, 480.0));
        media.padding = Insets::new(20.0, 0.0, 0.0, 0.0);
        let _surface = media.install();
        let root = crate::Column::new().child(bar);
        build_ui(&root, Size::new(320.0, 480.0), &Runtime::default(), theme)
    }

    /// **44 pixels under the status bar, the bar's colour behind both, a hairline under it.**
    #[test]
    fn the_bar_is_44_under_the_status_bar() {
        let theme = Theme::light();
        let ui = bar(CupertinoNavigationBar::new().title("Inbox"), &theme);
        let background = CupertinoTheme::of(&theme).bar_background_color;
        let surface = painted(&ui, background);
        assert_eq!(surface, Rect::new(0.0, 0.0, 320.0, 64.0));
        let line = painted(&ui, CUPERTINO_NAV_BAR_BORDER);
        assert_eq!(line, Rect::new(0.0, 63.0, 320.0, 1.0));
        // The large title adds 52.
        let ui = bar(CupertinoNavigationBar::large("Inbox"), &theme);
        assert_eq!(painted(&ui, background).height, 20.0 + 44.0 + 52.0);
        // In the dark, the bar's dark colour; a background of the caller's over it.
        let dark = Theme::dark();
        let ui = bar(CupertinoNavigationBar::new().title("Inbox"), &dark);
        painted(&ui, CupertinoTheme::DEFAULT_BAR_BACKGROUND.dark_color);
        let ui = bar(
            CupertinoNavigationBar::new()
                .background_color(RED)
                .no_border(),
            &theme,
        );
        painted(&ui, RED);
        assert!(!ui.scene().all().into_iter().any(
            |p| matches!(p, Primitive::Rect { color, .. } if *color == CUPERTINO_NAV_BAR_BORDER)
        ));
    }

    /// **The back chevron says "Back" and sends its message**; the previous title beside it
    /// when short, "Back" when not.
    #[test]
    fn the_back_button_says_back() {
        let theme = Theme::light();
        let ui = bar(
            CupertinoNavigationBar::new()
                .title("Inbox")
                .back(Msg::Back)
                .previous_page_title("Mailboxes"),
            &theme,
        );
        let (id, rect, node) = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.role == frus_core::Role::Button)
            .cloned()
            .expect("the back button is announced");
        assert_eq!(node.label.as_deref(), Some("Back"));
        assert!(rect.width >= BACK_TAP_WIDTH && rect.x == 0.0);
        assert_eq!(rect.y, 20.0);
        assert_eq!(ui.msg_for(id), Some(Msg::Back));
        let texts = |ui: &Ui<Msg>| -> Vec<String> {
            ui.scene()
                .all()
                .into_iter()
                .filter_map(|p| match p {
                    Primitive::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };
        assert!(texts(&ui).contains(&"Mailboxes".to_string()));
        let long = bar(
            CupertinoNavigationBar::new()
                .back(Msg::Back)
                .previous_page_title("All the mailboxes"),
            &theme,
        );
        assert!(texts(&long).contains(&"Back".to_string()));
        // The chevron is two strokes in the primary colour.
        let strokes = ui
            .scene()
            .all()
            .into_iter()
            .filter(|p| {
                matches!(p, Primitive::Path { stroke: Some(s), .. }
                    if s.color == CupertinoTheme::of(&theme).primary_color)
            })
            .count();
        assert_eq!(strokes, 2);
    }

    /// **A translucent bar blurs what is under it; an opaque one does not.**
    #[test]
    fn a_translucent_bar_blurs() {
        let theme = Theme::light();
        let layers = |ui: &Ui<Msg>| {
            ui.scene()
                .all()
                .into_iter()
                .filter(
                    |p| matches!(p, Primitive::Layer { filter, .. } if filter.backdrop.is_some()),
                )
                .count()
        };
        let blurred = layers(&bar(CupertinoNavigationBar::new().title("A"), &theme));
        let opaque = layers(&bar(
            CupertinoNavigationBar::new()
                .title("A")
                .background_color(Color::WHITE),
            &theme,
        ));
        assert_eq!((blurred, opaque), (1, 0));
        let off = layers(&bar(
            CupertinoNavigationBar::new()
                .title("A")
                .enable_background_filter_blur(false),
            &theme,
        ));
        assert_eq!(off, opaque);
    }
}
