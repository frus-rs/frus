// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! A conversation (milestone 815): [`ChatList`], [`MessageBubble`], [`TypingIndicator`] and
//! [`ChatComposer`], over the application's [`ChatMessage`]s.
//!
//! # The list
//!
//! A [`ChatList`] is a [`ListView`] built **from the far end**: the newest message sits at the
//! bottom, the list rests there, and a message arriving comes into view under the others. A
//! reader who scrolled up to read something older keeps their place: the list
//! [holds its position](ListView::hold_position) while messages arrive, and a button offers to
//! jump back to the newest, with how many came in meanwhile.
//!
//! Only the messages in view are built — a conversation of fifty thousand messages builds the
//! dozen on the screen — and each row is put together from its message and its two neighbours:
//! whether it starts a day (a date between the messages), whether it starts or ends a run of
//! messages from one sender (a name over the first, an avatar beside the last, tighter corners
//! between them).
//!
//! # Where "scrolled up" lives
//!
//! In the application, as a [`ChatListState`], like every other state here: the list sends a
//! new one when the reader leaves the newest messages or comes back to them — not on every
//! frame of a scroll — and when the jump button is pressed.

use std::rc::Rc;

use frus_core::{BorderRadiusDirectional, Color, Insets, Role, SemanticsProperties, TextStyle};
use frus_layout::{Align, Justify};

use crate::avatar::CircleAvatar;
use crate::barrier::ExcludeSemantics;
use crate::button::{Button, Variant};
use crate::composite::composite_widget;
use crate::container::Container;
use crate::datepickerdialog::CalendarDate;
use crate::dsl::keyed;
use crate::expanded::{Expanded, Flexible};
use crate::flex::Flex;
use crate::icon::Icon;
use crate::iconbutton::{IconButton, IconButtonVariant};
use crate::icons::{IconData, Icons};
use crate::lazy::Lazy;
use crate::list::ListView;
use crate::positioned::Positioned;
use crate::semantics::Semantics;
use crate::stack::{Stack, StackFit};
use crate::text::Text;
use crate::textinput::TextField;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;

/// How far from its newest message a conversation may be and still count as following it.
const AWAY: f32 = 24.0;
/// Messages from one sender this close together, in seconds, are one run, unless told.
const GROUP_WINDOW: i64 = 5 * 60;
/// The key of the typing row.
const TYPING_KEY: &str = "\u{0}typing";
/// How far back the list counts unseen messages.
const UNSEEN_CAP: usize = 999;

/// Where a message one sends has got to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MessageStatus {
    /// On its way: not yet taken by the server.
    Sending,
    /// Taken by the server.
    #[default]
    Sent,
    /// On the other side's device.
    Delivered,
    /// Read.
    Read,
    /// Not sent: the bubble offers to try again.
    Failed,
}

/// One message of a conversation: plain data, which the application keeps.
///
/// `time` is in seconds since 1970-01-01 00:00 **in the reader's local time** — the
/// moment's Unix time plus the reader's offset from UTC — which is what days and times are
/// read from: the date between two days, the time under a run of messages.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatMessage {
    /// What identifies it: the key its row is kept by, and what a retry is sent with.
    pub id: String,
    /// Who sent it, as the application names people; messages from one author in a row
    /// are grouped.
    pub author: String,
    /// The name shown over a run of its messages, and read out.
    pub author_name: String,
    /// What it says. Empty for a message that is only an attachment.
    pub text: String,
    /// When, in local seconds; see the type.
    pub time: i64,
    /// Whether the reader sent it: drawn at the end side, in the accent.
    pub outgoing: bool,
    /// Where it has got to; shown on the reader's own messages.
    pub status: MessageStatus,
}

impl ChatMessage {
    /// A message someone else sent.
    pub fn incoming(
        id: impl Into<String>,
        author: impl Into<String>,
        text: impl Into<String>,
        time: i64,
    ) -> Self {
        let author = author.into();
        Self {
            id: id.into(),
            author_name: author.clone(),
            author,
            text: text.into(),
            time,
            outgoing: false,
            status: MessageStatus::Sent,
        }
    }

    /// A message the reader sent, [sent](MessageStatus::Sent) unless told otherwise.
    pub fn outgoing(id: impl Into<String>, text: impl Into<String>, time: i64) -> Self {
        Self {
            id: id.into(),
            author: String::new(),
            author_name: String::new(),
            text: text.into(),
            time,
            outgoing: true,
            status: MessageStatus::Sent,
        }
    }

    /// The name shown and read out for its author, when it is not the author itself.
    #[must_use]
    pub fn author_name(mut self, name: impl Into<String>) -> Self {
        self.author_name = name.into();
        self
    }

    /// Where it has got to.
    #[must_use]
    pub fn status(mut self, status: MessageStatus) -> Self {
        self.status = status;
        self
    }

    /// The day it was sent on, in days since 1970-01-01, local time.
    pub fn day(&self) -> i64 {
        self.time.div_euclid(86_400)
    }
}

/// The words a conversation says, English unless told (milestone 815). `{n}` and `{name}`
/// are replaced where they appear.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatStrings {
    /// The date over today's messages.
    pub today: String,
    /// The date over yesterday's.
    pub yesterday: String,
    /// Read out for a message on its way.
    pub sending: String,
    /// Read out for a message the server took.
    pub sent: String,
    /// Read out for a delivered message.
    pub delivered: String,
    /// Read out for a read message.
    pub read: String,
    /// Shown under a message that was not sent.
    pub failed: String,
    /// The button that sends it again.
    pub retry: String,
    /// What the jump button is called.
    pub jump: String,
    /// The jump button's words when messages came in meanwhile: `{n}` of them.
    pub new_messages: String,
    /// One person typing: `{name}`.
    pub typing_one: String,
    /// Several: `{name}` is their names, joined.
    pub typing_many: String,
    /// What the reader is called when their messages are read out.
    pub you: String,
    /// The composer's empty field.
    pub placeholder: String,
    /// The composer's send button.
    pub send: String,
}

impl Default for ChatStrings {
    fn default() -> Self {
        Self {
            today: "Today".into(),
            yesterday: "Yesterday".into(),
            sending: "Sending".into(),
            sent: "Sent".into(),
            delivered: "Delivered".into(),
            read: "Read".into(),
            failed: "Not sent".into(),
            retry: "Retry".into(),
            jump: "Jump to the newest messages".into(),
            new_messages: "{n} new".into(),
            typing_one: "{name} is typing".into(),
            typing_many: "{name} are typing".into(),
            you: "You".into(),
            placeholder: "Message".into(),
            send: "Send".into(),
        }
    }
}

impl ChatStrings {
    fn status(&self, status: MessageStatus) -> &str {
        match status {
            MessageStatus::Sending => &self.sending,
            MessageStatus::Sent => &self.sent,
            MessageStatus::Delivered => &self.delivered,
            MessageStatus::Read => &self.read,
            MessageStatus::Failed => &self.failed,
        }
    }

    fn typing(&self, names: &[String]) -> String {
        match names {
            [] => String::new(),
            [one] => self.typing_one.replace("{name}", one),
            many => self.typing_many.replace("{name}", &many.join(", ")),
        }
    }
}

/// Defaults for a conversation's look (milestone 815): the theme's, under each widget's own
/// [`style`](ChatList::style). Every field unset is the framework's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChatTheme {
    /// Others' bubbles. Unset, `surface_container_high`.
    pub incoming_color: Option<Color>,
    /// Their words. Unset, `on_surface`.
    pub incoming_text_color: Option<Color>,
    /// The reader's bubbles. Unset, `primary`.
    pub outgoing_color: Option<Color>,
    /// Their words. Unset, `on_primary`.
    pub outgoing_text_color: Option<Color>,
    /// A bubble's corners. Unset, eighteen.
    pub radius: Option<f32>,
    /// The corners between two bubbles of one run, on the sender's side. Unset, four.
    pub grouped_radius: Option<f32>,
    /// The room inside a bubble. Unset, twelve across and eight down.
    pub padding: Option<Insets>,
    /// The widest a bubble grows. Unset, 560.
    pub max_bubble_width: Option<f32>,
    /// A message's type. Unset, `body_large`.
    pub text_style: Option<TextStyle>,
    /// The time and status under a run. Unset, `label_small`.
    pub meta_text_style: Option<TextStyle>,
    /// Their colour, and the typing dots'. Unset, the theme's muted ink.
    pub meta_color: Option<Color>,
    /// The mark of a read message. Unset, `primary`.
    pub read_color: Option<Color>,
    /// The author's name over a run. Unset, `label_medium`.
    pub name_text_style: Option<TextStyle>,
    /// The date between days. Unset, `label_medium`.
    pub separator_text_style: Option<TextStyle>,
    /// The diameter of an avatar. Unset, twenty-eight.
    pub avatar_size: Option<f32>,
}

impl ChatTheme {
    /// `self` over `under`, field by field.
    fn over(self, under: ChatTheme) -> ChatTheme {
        ChatTheme {
            incoming_color: self.incoming_color.or(under.incoming_color),
            incoming_text_color: self.incoming_text_color.or(under.incoming_text_color),
            outgoing_color: self.outgoing_color.or(under.outgoing_color),
            outgoing_text_color: self.outgoing_text_color.or(under.outgoing_text_color),
            radius: self.radius.or(under.radius),
            grouped_radius: self.grouped_radius.or(under.grouped_radius),
            padding: self.padding.or(under.padding),
            max_bubble_width: self.max_bubble_width.or(under.max_bubble_width),
            text_style: self.text_style.or(under.text_style),
            meta_text_style: self.meta_text_style.or(under.meta_text_style),
            meta_color: self.meta_color.or(under.meta_color),
            read_color: self.read_color.or(under.read_color),
            name_text_style: self.name_text_style.or(under.name_text_style),
            separator_text_style: self.separator_text_style.or(under.separator_text_style),
            avatar_size: self.avatar_size.or(under.avatar_size),
        }
    }
}

/// A conversation's look, every word decided: the caller's, then the theme's, then the
/// framework's.
#[derive(Clone, Copy, Debug)]
struct Look {
    incoming: Color,
    on_incoming: Color,
    outgoing: Color,
    on_outgoing: Color,
    radius: f32,
    grouped: f32,
    padding: Insets,
    max_width: f32,
    text: TextStyle,
    meta: TextStyle,
    meta_color: Color,
    read_color: Color,
    name: TextStyle,
    separator: TextStyle,
    avatar: f32,
    error: Color,
}

impl Look {
    fn of(caller: ChatTheme, theme: &Theme) -> Self {
        let t = caller.over(theme.widgets.chat);
        let scale = crate::theme::type_scale(Some(theme));
        Self {
            incoming: t
                .incoming_color
                .unwrap_or(theme.scheme.surface_container_high),
            on_incoming: t.incoming_text_color.unwrap_or(theme.on_surface),
            outgoing: t.outgoing_color.unwrap_or(theme.primary),
            on_outgoing: t.outgoing_text_color.unwrap_or(theme.on_primary),
            radius: t.radius.unwrap_or(18.0),
            grouped: t.grouped_radius.unwrap_or(4.0),
            padding: t.padding.unwrap_or(Insets::symmetric(12.0, 8.0)),
            max_width: t.max_bubble_width.unwrap_or(560.0),
            text: t.text_style.unwrap_or(scale.body_large),
            meta: t.meta_text_style.unwrap_or(scale.label_small),
            meta_color: t.meta_color.unwrap_or(theme.muted),
            read_color: t.read_color.unwrap_or(theme.primary),
            name: t.name_text_style.unwrap_or(scale.label_medium),
            separator: t.separator_text_style.unwrap_or(scale.label_medium),
            avatar: t.avatar_size.unwrap_or(28.0),
            error: theme.error,
        }
    }
}

/// `HH:MM`, on a 24-hour clock, of a local time in seconds.
pub fn chat_time(time: i64) -> String {
    let secs = time.rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}

/// The words for day `day` seen on day `today` (both in days since 1970-01-01): today,
/// yesterday, or the date — with its year when it is not this year's.
fn day_label(day: i64, today: i64, strings: &ChatStrings) -> String {
    if day == today {
        return strings.today.clone();
    }
    if day == today - 1 {
        return strings.yesterday.clone();
    }
    let date = CalendarDate::from_days(day);
    let words = crate::localizations::of().medium_date(date.weekday(), date.month, date.day);
    if date.year == CalendarDate::from_days(today).year {
        words
    } else {
        format!("{words}, {}", date.year)
    }
}

// --- The bubble ----------------------------------------------------------------------------

/// What a bubble is made of.
struct BubbleParts<Msg> {
    message: ChatMessage,
    first: bool,
    last: bool,
    show_name: bool,
    show_avatar: bool,
    avatar: Option<Box<dyn Widget<Msg>>>,
    attachment: Option<Box<dyn Widget<Msg>>>,
    on_retry: Option<Msg>,
    time_text: Option<String>,
    strings: Rc<ChatStrings>,
    style: ChatTheme,
}

/// **One message**, as a conversation draws it (milestone 815): a bubble at the start side
/// for someone else's, at the end side in the accent for the reader's, with what goes round
/// it.
///
/// A [`ChatList`] makes these itself; a bubble is public for a conversation laid out some
/// other way — a thread under a post, a single quoted message.
///
/// - **Runs**: a message that [starts](Self::first_in_group) a run from its sender shows the
///   sender's name over it; one that [ends](Self::last_in_group) a run shows the avatar
///   beside it and the time (and, for the reader's own, the [status](MessageStatus)) under
///   it. Between two bubbles of a run the corners on the sender's side are tighter.
/// - **The words can be selected** and copied.
/// - **A failed message** says so under it, with a [retry](Self::on_retry) button.
/// - **An attachment** — a picture, a file — goes above the words, inside the bubble.
///
/// It reads to a screen reader as one item: who, when, what, and where it has got to.
pub struct MessageBubble<Msg = crate::callback::Callback> {
    inner: Lazy<BubbleParts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> MessageBubble<Msg> {
    /// The bubble of `message`, alone: the first and the last of its run.
    pub fn new(message: &ChatMessage) -> Self {
        Self {
            inner: Lazy::new(
                BubbleParts {
                    message: message.clone(),
                    first: true,
                    last: true,
                    show_name: true,
                    show_avatar: true,
                    avatar: None,
                    attachment: None,
                    on_retry: None,
                    time_text: None,
                    strings: Rc::new(ChatStrings::default()),
                    style: ChatTheme::default(),
                },
                build_bubble::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut BubbleParts<Msg> {
        self.inner.parts_mut()
    }

    /// Whether it starts a run from its sender: the name goes over it.
    #[must_use]
    pub fn first_in_group(mut self, first: bool) -> Self {
        self.p().first = first;
        self
    }

    /// Whether it ends a run: the avatar goes beside it, the time under it.
    #[must_use]
    pub fn last_in_group(mut self, last: bool) -> Self {
        self.p().last = last;
        self
    }

    /// Whether someone else's message shows its author's name over a run. On, unless told —
    /// a conversation between two people usually turns it off.
    #[must_use]
    pub fn show_name(mut self, show: bool) -> Self {
        self.p().show_name = show;
        self
    }

    /// Whether someone else's message keeps room for an avatar. On, unless told.
    #[must_use]
    pub fn show_avatar(mut self, show: bool) -> Self {
        self.p().show_avatar = show;
        self
    }

    /// The avatar, instead of the author's initials in a circle.
    #[must_use]
    pub fn avatar(mut self, avatar: impl Widget<Msg> + 'static) -> Self {
        self.p().avatar = Some(Box::new(avatar));
        self
    }

    /// What goes above the words inside the bubble: a picture, a file, a link's preview.
    #[must_use]
    pub fn attachment(mut self, attachment: impl Widget<Msg> + 'static) -> Self {
        self.p().attachment = Some(Box::new(attachment));
        self
    }

    /// What the retry button under a [failed](MessageStatus::Failed) message sends. Without
    /// it a failed message says so and offers nothing.
    #[must_use]
    pub fn on_retry(mut self, message: impl Into<Msg>) -> Self {
        self.p().on_retry = Some(message.into());
        self
    }

    /// The time under it, instead of `HH:MM`.
    #[must_use]
    pub fn time_text(mut self, text: impl Into<String>) -> Self {
        self.p().time_text = Some(text.into());
        self
    }

    /// The words it says, instead of the English ones.
    #[must_use]
    pub fn strings(mut self, strings: ChatStrings) -> Self {
        self.p().strings = Rc::new(strings);
        self
    }

    /// Its look, over the theme's [`ChatTheme`].
    #[must_use]
    pub fn style(mut self, style: ChatTheme) -> Self {
        self.p().style = style;
        self
    }

    fn with_strings(mut self, strings: Rc<ChatStrings>) -> Self {
        self.p().strings = strings;
        self
    }
}

composite_widget!(MessageBubble);

fn build_bubble<Msg: Clone + 'static>(p: BubbleParts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| bubble(p, theme)))
}

/// The row a message is drawn in, under `theme`.
fn bubble<Msg: Clone + 'static>(p: BubbleParts<Msg>, theme: &Theme) -> Box<dyn Widget<Msg>> {
    let look = Look::of(p.style, theme);
    let m = &p.message;
    let (bg, fg) = if m.outgoing {
        (look.outgoing, look.on_outgoing)
    } else {
        (look.incoming, look.on_incoming)
    };
    // The sender's side is tighter between two bubbles of a run: the start side for
    // someone else, the end side for the reader. `resolve` mirrors it right to left.
    let top = if p.first { look.radius } else { look.grouped };
    let bottom = if p.last { look.radius } else { look.grouped };
    let corners = if m.outgoing {
        BorderRadiusDirectional {
            top_start: look.radius,
            top_end: top,
            bottom_end: bottom,
            bottom_start: look.radius,
        }
    } else {
        BorderRadiusDirectional {
            top_start: top,
            top_end: look.radius,
            bottom_end: look.radius,
            bottom_start: bottom,
        }
    };

    let mut inside: Flex<Msg> = Flex::column().gap(6.0).align(Align::Start);
    if let Some(attachment) = p.attachment {
        inside = inside.child_boxed(attachment);
    }
    // The bubble's own words — the text, the name, the time, the failure — are said once,
    // by the item's label below, so each is kept out of the tree. What the caller hands
    // in (an attachment, an avatar) keeps its voice: it may be operable.
    if !m.text.is_empty() {
        inside = inside.child(ExcludeSemantics::new(
            Text::styled(m.text.clone(), look.text)
                .color(fg)
                .wrap()
                .selectable(),
        ));
    }
    let body = Container::new()
        .padding_insets(look.padding)
        .color(bg)
        .radius(corners.resolve(theme.direction))
        .max_width(look.max_width)
        .child(inside);

    let failed = m.outgoing && m.status == MessageStatus::Failed;
    let mut column: Flex<Msg> =
        Flex::column()
            .gap(2.0)
            .align(if m.outgoing { Align::End } else { Align::Start });
    if !m.outgoing && p.show_name && p.first && !m.author_name.is_empty() {
        column = column.child(ExcludeSemantics::new(
            Container::new()
                .padding_each(0.0, 12.0, 2.0, 12.0)
                .child(Text::styled(m.author_name.clone(), look.name).color(look.meta_color)),
        ));
    }
    column = column.child(body);
    let time = p.time_text.clone().unwrap_or_else(|| chat_time(m.time));
    if failed {
        let mut line: Flex<Msg> =
            Flex::row()
                .gap(4.0)
                .align(Align::Center)
                .child(ExcludeSemantics::new(
                    Flex::row()
                        .gap(4.0)
                        .align(Align::Center)
                        .child(Icon::new(Icons::ERROR_OUTLINE).size(16.0).color(look.error))
                        .child(Text::styled(p.strings.failed.clone(), look.meta).color(look.error)),
                ));
        if let Some(retry) = p.on_retry.clone() {
            line = line.child(
                Button::new(p.strings.retry.clone())
                    .variant(Variant::Text)
                    .on_press(retry),
            );
        }
        column = column.child(line);
    } else if p.last || (m.outgoing && m.status == MessageStatus::Sending) {
        let mut line: Flex<Msg> = Flex::row()
            .gap(4.0)
            .align(Align::Center)
            .child(Text::styled(time.clone(), look.meta).color(look.meta_color));
        if m.outgoing {
            let (mark, colour) = match m.status {
                MessageStatus::Sending => (Icons::SCHEDULE, look.meta_color),
                MessageStatus::Sent => (Icons::DONE, look.meta_color),
                MessageStatus::Delivered => (Icons::DONE_ALL, look.meta_color),
                MessageStatus::Read => (Icons::DONE_ALL, look.read_color),
                MessageStatus::Failed => (Icons::ERROR_OUTLINE, look.error),
            };
            line = line.child(Icon::new(mark).size(14.0).color(colour));
        }
        column = column.child(ExcludeSemantics::new(
            Container::new()
                .padding_each(0.0, 4.0, 0.0, 4.0)
                .child(line),
        ));
    }

    // Read as one item — unless it offers a retry, which must stay a button of its own.
    let who = if m.outgoing {
        p.strings.you.clone()
    } else {
        m.author_name.clone()
    };
    let said = if m.outgoing {
        format!("{who}, {time}, {}: {}", p.strings.status(m.status), m.text)
    } else {
        format!("{who}, {time}: {}", m.text)
    };
    let semantics = SemanticsProperties::new(Role::ListItem).label(said);
    // The bubble is what is announced, not the row: `Semantics` is a transparent box that
    // hugs its child, so wrapped round the row it left the row as narrow as its bubble, with
    // no end to hang the reader's bubbles from.
    let column = Semantics::new(semantics, column).merging(!failed);

    // Room on the other side, so that a long message does not run the whole width.
    const OTHER_SIDE: f32 = 48.0;
    let mut row: Flex<Msg> = Flex::row().align(Align::End);
    if m.outgoing {
        row = row
            .justify(Justify::End)
            .child(Container::new().width(OTHER_SIDE))
            .child(Flexible::new(column));
    } else {
        if p.show_avatar {
            let slot = Container::new()
                .width(look.avatar)
                .margin_each(0.0, 8.0, 0.0, 0.0);
            let slot = match (p.last, p.avatar) {
                (true, Some(avatar)) => slot.child(avatar),
                (true, None) => slot.child(ExcludeSemantics::new(
                    CircleAvatar::initials(m.author_name.clone()).size(look.avatar),
                )),
                (false, _) => slot.height(1.0),
            };
            row = row.child(slot);
        }
        row = row
            .child(Flexible::new(column))
            .child(Container::new().width(OTHER_SIDE));
    }

    Box::new(row)
}

mod typing;
pub use typing::TypingIndicator;

// --- The list ------------------------------------------------------------------------------

/// **Where a [`ChatList`] is**, as far as the application needs to know (milestone 815):
/// whether the reader has left the newest messages, and which message was the newest when
/// they did. Kept by the application and sent, changed, by the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatListState {
    away: bool,
    seen: Option<String>,
    /// Each new number glides the list back to its newest message.
    jump: u32,
    /// Gliding back: the reports on the way are not a reader leaving.
    returning: bool,
}

impl ChatListState {
    /// A list following its newest messages.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the reader has scrolled away from the newest messages.
    pub fn is_away(&self) -> bool {
        self.away
    }

    /// The newest message's id when the reader scrolled away, which the ones after it are
    /// counted from.
    pub fn seen(&self) -> Option<&str> {
        self.seen.as_deref()
    }

    /// The state that **glides the list back to its newest message** — what the jump button
    /// sends, and what an application sets after the reader sends a message of their own.
    #[must_use]
    pub fn jump_to_newest(&self) -> Self {
        Self {
            away: false,
            seen: None,
            jump: self.jump.wrapping_add(1),
            returning: true,
        }
    }

    /// What a list at `offset` from its newest message reports, if anything: a change of
    /// "away", and nothing else.
    fn report(&self, offset: f32, newest: Option<&str>) -> Option<Self> {
        let away = offset > AWAY;
        if self.returning {
            // On the way back, until it is there.
            return (!away).then(|| Self {
                returning: false,
                ..self.clone()
            });
        }
        (away != self.away).then(|| Self {
            away,
            seen: if away {
                newest.map(str::to_owned)
            } else {
                None
            },
            jump: self.jump,
            returning: false,
        })
    }
}

/// A builder of a widget for a message, when there is one.
type ForMessage<Msg> = Rc<dyn Fn(&ChatMessage) -> Option<Box<dyn Widget<Msg>>>>;

/// What a list is made of.
struct ListParts<Msg> {
    messages: Rc<Vec<ChatMessage>>,
    state: ChatListState,
    on_change: Rc<dyn Fn(ChatListState) -> Msg>,
    on_retry: Option<Rc<dyn Fn(String) -> Msg>>,
    typing: Vec<String>,
    attachment: Option<ForMessage<Msg>>,
    avatar: Option<ForMessage<Msg>>,
    show_names: bool,
    show_avatars: bool,
    group_window: i64,
    today: Option<i64>,
    format_time: Option<Rc<dyn Fn(i64) -> String>>,
    format_day: Option<Rc<dyn Fn(i64, i64) -> String>>,
    strings: Rc<ChatStrings>,
    style: ChatTheme,
    padding: Insets,
}

/// **A conversation**: its messages, newest at the bottom, built only as far as they are seen
/// (milestone 815).
///
/// ```
/// use std::rc::Rc;
/// use frus_widgets::{ChatList, ChatListState, ChatMessage, MessageStatus};
///
/// let messages = Rc::new(vec![
///     ChatMessage::incoming("1", "ada", "Did the build pass?", 1_700_000_000),
///     ChatMessage::outgoing("2", "It did.", 1_700_000_060).status(MessageStatus::Read),
/// ]);
/// let state = ChatListState::new();
/// let chat: ChatList<()> = ChatList::new(messages, &state, |_next: ChatListState| ())
///     .typing(["Ada"]);
/// # let _ = chat;
/// ```
///
/// - **Newest at the bottom**, resting there: a message arriving comes into view. The
///   messages are given **oldest first**, as an application appends them; keep them in an
///   `Rc` so that a rebuild does not copy them.
/// - **The reader keeps their place**: scrolled up, the rows in view stay where they are as
///   messages arrive, and a button offers to jump back — saying how many came in. The list
///   sends `on_change` a new [`ChatListState`] when the reader leaves the newest messages, comes
///   back, or presses the button.
/// - **Runs and days**: messages from one sender close together are drawn as a run, with a
///   date between days ("Today", "Yesterday", the date).
/// - **Typing**: [`typing`](Self::typing) shows the dots under the newest message.
/// - **Fast however long**: only the rows in view are built, each from its message and its
///   two neighbours, and only when it comes into view.
///
/// It fills the room it is given: put it in an [`Expanded`] in a column, over a
/// [`ChatComposer`]. Every part of its look is the caller's [`style`](Self::style), then the
/// theme's [`ChatTheme`], then the framework's; every word is [`strings`](Self::strings).
pub struct ChatList<Msg = crate::callback::Callback> {
    inner: Lazy<ListParts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> ChatList<Msg> {
    /// The conversation of `messages`, oldest first, where `state` says, sending `on_change`
    /// the state it should have next.
    pub fn new<R: crate::callback::IntoMsg<Msg>>(
        messages: impl Into<Rc<Vec<ChatMessage>>>,
        state: &ChatListState,
        on_change: impl Fn(ChatListState) -> R + 'static,
    ) -> Self {
        let on_change = crate::callback::handler1(on_change);
        Self {
            inner: Lazy::new(
                ListParts {
                    messages: messages.into(),
                    state: state.clone(),
                    on_change: Rc::new(on_change),
                    on_retry: None,
                    typing: Vec::new(),
                    attachment: None,
                    avatar: None,
                    show_names: true,
                    show_avatars: true,
                    group_window: GROUP_WINDOW,
                    today: None,
                    format_time: None,
                    format_day: None,
                    strings: Rc::new(ChatStrings::default()),
                    style: ChatTheme::default(),
                    padding: Insets::symmetric(12.0, 8.0),
                },
                build_list::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut ListParts<Msg> {
        self.inner.parts_mut()
    }

    /// What a failed message's retry button sends, given the message's id.
    #[must_use]
    pub fn on_retry<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_retry: impl Fn(String) -> R + 'static,
    ) -> Self {
        self.p().on_retry = Some(Rc::new(crate::callback::handler1(on_retry)));
        self
    }

    /// Who is typing, by name: the dots show under the newest message while anyone is.
    #[must_use]
    pub fn typing(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.p().typing = names.into_iter().map(Into::into).collect();
        self
    }

    /// What goes inside a message's bubble above its words — a picture, a file — when there is
    /// something: asked for each message as it comes into view.
    #[must_use]
    pub fn attachment<W: Widget<Msg> + 'static>(
        mut self,
        attachment: impl Fn(&ChatMessage) -> Option<W> + 'static,
    ) -> Self {
        self.p().attachment = Some(Rc::new(move |m: &ChatMessage| {
            attachment(m).map(|w| Box::new(w) as Box<dyn Widget<Msg>>)
        }));
        self
    }

    /// The avatar beside a run of someone's messages, instead of their initials, when there
    /// is one.
    #[must_use]
    pub fn avatar<W: Widget<Msg> + 'static>(
        mut self,
        avatar: impl Fn(&ChatMessage) -> Option<W> + 'static,
    ) -> Self {
        self.p().avatar = Some(Rc::new(move |m: &ChatMessage| {
            avatar(m).map(|w| Box::new(w) as Box<dyn Widget<Msg>>)
        }));
        self
    }

    /// Whether the authors' names show over their runs. On, unless told; a conversation
    /// between two people usually turns it off.
    #[must_use]
    pub fn show_names(mut self, show: bool) -> Self {
        self.p().show_names = show;
        self
    }

    /// Whether others' runs have an avatar beside them. On, unless told.
    #[must_use]
    pub fn show_avatars(mut self, show: bool) -> Self {
        self.p().show_avatars = show;
        self
    }

    /// How close together, in seconds, two messages from one sender must be to be one run.
    /// Five minutes, unless told.
    #[must_use]
    pub fn group_window(mut self, seconds: i64) -> Self {
        self.p().group_window = seconds.max(0);
        self
    }

    /// Which day is today, for "Today" and "Yesterday". Unset, the system clock's day in UTC —
    /// an application that knows the reader's offset says it here.
    #[must_use]
    pub fn today(mut self, today: CalendarDate) -> Self {
        self.p().today = Some(today.to_days());
        self
    }

    /// How a time is written under a run, given the local seconds. `HH:MM` unless told.
    #[must_use]
    pub fn format_time(mut self, format: impl Fn(i64) -> String + 'static) -> Self {
        self.p().format_time = Some(Rc::new(format));
        self
    }

    /// How a day is written between days, given it and today, in days since 1970-01-01.
    /// "Today", "Yesterday" and the date unless told.
    #[must_use]
    pub fn format_day(mut self, format: impl Fn(i64, i64) -> String + 'static) -> Self {
        self.p().format_day = Some(Rc::new(format));
        self
    }

    /// The words it says, instead of the English ones.
    #[must_use]
    pub fn strings(mut self, strings: ChatStrings) -> Self {
        self.p().strings = Rc::new(strings);
        self
    }

    /// Its look, over the theme's [`ChatTheme`].
    #[must_use]
    pub fn style(mut self, style: ChatTheme) -> Self {
        self.p().style = style;
        self
    }

    /// The room round the messages, inside the scrolling area. Twelve across and eight down,
    /// unless told.
    #[must_use]
    pub fn padding(mut self, padding: Insets) -> Self {
        self.p().padding = padding;
        self
    }
}

composite_widget!(ChatList);

/// Whether `a` and `b`, one after the other, are one run: one day, one sender, at most
/// `window` seconds apart.
fn same_run(a: &ChatMessage, b: &ChatMessage, window: i64) -> bool {
    a.day() == b.day()
        && a.outgoing == b.outgoing
        && a.author == b.author
        && (b.time - a.time).abs() <= window
}

/// Everything a row is made from, shared by the rows of one build.
struct Rows<Msg> {
    p: ListParts<Msg>,
    today: i64,
    typing_rows: usize,
}

impl<Msg: Clone + 'static> Rows<Msg> {
    /// The message row `row` shows, if it shows one.
    fn message_at(&self, row: usize) -> Option<usize> {
        let n = self.p.messages.len();
        let from_newest = row.checked_sub(self.typing_rows)?;
        n.checked_sub(1 + from_newest)
    }

    fn same_run(&self, a: &ChatMessage, b: &ChatMessage) -> bool {
        same_run(a, b, self.p.group_window)
    }

    fn row(&self, row: usize) -> Box<dyn Widget<Msg>> {
        let Some(i) = self.message_at(row) else {
            return self.typing_row();
        };
        let messages = &self.p.messages;
        let m = &messages[i];
        let before = i.checked_sub(1).map(|j| &messages[j]);
        let after = messages.get(i + 1);
        let new_day = before.is_none_or(|b| b.day() != m.day());
        let first = before.is_none_or(|b| !self.same_run(b, m));
        let last = after.is_none_or(|a| !self.same_run(m, a));

        let mut bubble = MessageBubble::new(m)
            .first_in_group(first)
            .last_in_group(last)
            .show_name(self.p.show_names)
            .show_avatar(self.p.show_avatars)
            .style(self.p.style)
            .with_strings(self.p.strings.clone());
        if let Some(format) = &self.p.format_time {
            bubble = bubble.time_text(format(m.time));
        }
        if let Some(widget) = self.p.attachment.as_ref().and_then(|a| a(m)) {
            bubble.p().attachment = Some(widget);
        }
        if let Some(widget) = self.p.avatar.as_ref().and_then(|a| a(m)) {
            bubble.p().avatar = Some(widget);
        }
        if m.status == MessageStatus::Failed {
            if let Some(retry) = &self.p.on_retry {
                bubble = bubble.on_retry(retry(m.id.clone()));
            }
        }

        let mut column: Flex<Msg> = Flex::column().align(Align::Stretch).padding_each(
            if first { 8.0 } else { 2.0 },
            0.0,
            0.0,
            0.0,
        );
        if new_day {
            let label = match &self.p.format_day {
                Some(format) => format(m.day(), self.today),
                None => day_label(m.day(), self.today, &self.p.strings),
            };
            column = column.child(separator(label, self.p.style));
        }
        Box::new(keyed(m.id.clone(), column.child(bubble)))
    }

    fn typing_row(&self) -> Box<dyn Widget<Msg>> {
        let label = self.p.strings.typing(&self.p.typing);
        let style = self.p.style;
        let show_avatars = self.p.show_avatars;
        let row = ThemeBuilder::new(move |theme: &Theme| {
            let look = Look::of(style, theme);
            let mut row: Flex<Msg> = Flex::row().align(Align::End);
            if show_avatars {
                row = row.child(
                    Container::new()
                        .width(look.avatar)
                        .height(1.0)
                        .margin_each(0.0, 8.0, 0.0, 0.0),
                );
            }
            let dots = TypingIndicator::new().color(look.meta_color).label(label);
            row.child(
                Container::new()
                    .padding_each(12.0, 14.0, 12.0, 14.0)
                    .color(look.incoming)
                    .radius(look.radius)
                    .child(dots),
            )
        });
        Box::new(keyed(
            TYPING_KEY,
            Flex::column()
                .align(Align::Stretch)
                .padding_each(8.0, 0.0, 0.0, 0.0)
                .child(row),
        ))
    }
}

/// The date between two days: a small pill in the middle, read as a heading.
fn separator<Msg: Clone + 'static>(label: String, style: ChatTheme) -> impl Widget<Msg> {
    ThemeBuilder::new(move |theme: &Theme| {
        let look = Look::of(style, theme);
        Flex::row()
            .justify(Justify::Center)
            .padding_each(8.0, 0.0, 8.0, 0.0)
            .child(Semantics::heading(
                Container::new()
                    .padding_each(4.0, 12.0, 4.0, 12.0)
                    .radius(12.0)
                    .color(theme.scheme.surface_container)
                    .child(Text::styled(label, look.separator).color(look.meta_color)),
            ))
    })
}

fn build_list<Msg: Clone + 'static>(p: ListParts<Msg>) -> Box<dyn Widget<Msg>> {
    let today = p.today.unwrap_or_else(|| CalendarDate::today().to_days());
    let typing_rows = usize::from(!p.typing.is_empty());
    let n = p.messages.len();
    let state = p.state.clone();
    let on_change = p.on_change.clone();
    let messages = p.messages.clone();

    let strings = p.strings.clone();

    // How many of the others' messages came after the newest one the reader saw.
    let unseen = state.seen().map_or(0, |seen| {
        messages
            .iter()
            .rev()
            .take(UNSEEN_CAP)
            .take_while(|m| m.id != seen)
            .filter(|m| !m.outgoing)
            .count()
    });

    let rows = Rc::new(Rows {
        p,
        today,
        typing_rows,
    });
    let find = {
        let messages = messages.clone();
        move |key: crate::ChildKey| {
            if typing_rows > 0 && key.is(&TYPING_KEY) {
                return Some(0);
            }
            messages
                .iter()
                .rposition(|m| key.is(&m.id))
                .map(|i| n - 1 - i + typing_rows)
        }
    };
    let report = {
        let state = state.clone();
        let on_change = on_change.clone();
        let messages = messages.clone();
        move |position: crate::ScrollPosition| {
            let newest = messages.last().map(|m| m.id.as_str());
            state
                .report(position.offset.1, newest)
                .map(|next| on_change(next))
        }
    };
    let padding = rows.p.padding;
    let list = {
        let rows = rows.clone();
        ListView::builder(n + typing_rows, move |r| rows.row(r))
    }
    .reverse()
    .estimated_extent(56.0)
    .hold_position(true)
    .find_child_index(find)
    .padding_each(padding.top, padding.right, padding.bottom, padding.left)
    .scroll_filter(report)
    .back_to_start_on(state.jump);

    let mut stack: Stack<Msg> = Stack::new().fit(StackFit::Expand).layer(list);
    if state.is_away() {
        let jump = on_change(state.jump_to_newest());
        let button: Box<dyn Widget<Msg>> = if unseen > 0 {
            let words = strings.new_messages.replace("{n}", &unseen.to_string());
            Box::new(
                Button::with_child(
                    Flex::row()
                        .gap(6.0)
                        .align(Align::Center)
                        .child(Icon::new(Icons::ARROW_DOWNWARD).size(18.0))
                        .child(Text::new(words)),
                )
                .variant(Variant::Tonal)
                .elevation(3.0)
                .on_press(jump),
            )
        } else {
            Box::new(
                IconButton::new(Icons::ARROW_DOWNWARD)
                    .variant(IconButtonVariant::Tonal)
                    .label(strings.jump.clone())
                    .on_press(jump),
            )
        };
        stack = stack.layer(Positioned::new(button).bottom(16.0).end(16.0));
    }
    Box::new(stack)
}

// --- The composer --------------------------------------------------------------------------

/// A way to change the composer's field.
type FieldChange<Msg> = Box<dyn FnOnce(TextField<Msg>) -> TextField<Msg>>;

/// What a composer is made of.
struct ComposerParts<Msg> {
    draft: String,
    on_change: Rc<dyn Fn(String) -> Msg>,
    on_send: Option<Msg>,
    attachment: Option<Box<dyn Widget<Msg>>>,
    max_lines: u16,
    send_on_enter: Option<bool>,
    send_icon: IconData,
    enabled: bool,
    strings: Rc<ChatStrings>,
    field: Option<FieldChange<Msg>>,
}

/// **Where a message is written** (milestone 815): a field that grows with what is typed, a
/// send button, and room for an attachment button.
///
/// ```
/// use frus_widgets::ChatComposer;
///
/// let draft = String::from("Hello");
/// let composer: ChatComposer<()> = ChatComposer::new(&draft, |_text: String| ()).on_send(());
/// # let _ = composer;
/// ```
///
/// - The field starts on one line and grows to [`max_lines`](Self::max_lines) (six), then
///   scrolls.
/// - **The send button is live only when there is something to send**: words other than
///   spaces.
/// - On a desktop **Enter sends** and Shift+Enter starts a new line; on a phone Enter starts a
///   new line and the button sends. [`send_on_enter`](Self::send_on_enter) decides otherwise.
/// - [`attachment`](Self::attachment) puts a button — a paper clip, a camera — at the start.
///
/// The field is a [`TextField`], and [`field`](Self::field) changes anything about it.
pub struct ChatComposer<Msg = crate::callback::Callback> {
    inner: Lazy<ComposerParts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> ChatComposer<Msg> {
    /// A composer showing `draft`, sending `on_change` each new draft.
    pub fn new<R: crate::callback::IntoMsg<Msg>>(
        draft: &str,
        on_change: impl Fn(String) -> R + 'static,
    ) -> Self {
        Self {
            inner: Lazy::new(
                ComposerParts {
                    draft: draft.to_owned(),
                    on_change: Rc::new(crate::callback::handler1(on_change)),
                    on_send: None,
                    attachment: None,
                    max_lines: 6,
                    send_on_enter: None,
                    send_icon: Icons::SEND,
                    enabled: true,
                    strings: Rc::new(ChatStrings::default()),
                    field: None,
                },
                build_composer::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut ComposerParts<Msg> {
        self.inner.parts_mut()
    }

    /// What sending sends: the button, and Enter where Enter sends. The application clears
    /// the draft when it handles it.
    #[must_use]
    pub fn on_send(mut self, message: impl Into<Msg>) -> Self {
        self.p().on_send = Some(message.into());
        self
    }

    /// A widget at the start of the composer: an attachment button, a camera.
    #[must_use]
    pub fn attachment(mut self, widget: impl Widget<Msg> + 'static) -> Self {
        self.p().attachment = Some(Box::new(widget));
        self
    }

    /// The most lines the field grows to before it scrolls. Six, unless told.
    #[must_use]
    pub fn max_lines(mut self, lines: u16) -> Self {
        self.p().max_lines = lines.max(2);
        self
    }

    /// Whether Enter sends (Shift+Enter then starts a new line). Unset, it does on a desktop
    /// and does not on a phone or a tablet.
    #[must_use]
    pub fn send_on_enter(mut self, send: bool) -> Self {
        self.p().send_on_enter = Some(send);
        self
    }

    /// The send button's icon.
    #[must_use]
    pub fn send_icon(mut self, icon: IconData) -> Self {
        self.p().send_icon = icon;
        self
    }

    /// Whether anything can be written or sent.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.p().enabled = enabled;
        self
    }

    /// The words it says — its [`placeholder`](ChatStrings::placeholder) and its
    /// [`send`](ChatStrings::send) button — instead of the English ones.
    #[must_use]
    pub fn strings(mut self, strings: ChatStrings) -> Self {
        self.p().strings = Rc::new(strings);
        self
    }

    /// Changes the field: its variant, its colours, anything a [`TextField`] says.
    #[must_use]
    pub fn field(
        mut self,
        change: impl FnOnce(TextField<Msg>) -> TextField<Msg> + 'static,
    ) -> Self {
        self.p().field = Some(Box::new(change));
        self
    }
}

composite_widget!(ChatComposer);

/// Whether `draft` has something to send.
fn sendable(draft: &str) -> bool {
    !draft.trim().is_empty()
}

fn build_composer<Msg: Clone + 'static>(p: ComposerParts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let enter_sends = p.send_on_enter.unwrap_or(theme.platform.is_desktop());
        let send = p
            .on_send
            .clone()
            .filter(|_| p.enabled && sendable(&p.draft));
        let on_change = p.on_change.clone();
        let mut field = TextField::new(p.draft.clone())
            .placeholder(p.strings.placeholder.clone())
            .max_lines(p.max_lines)
            .submit_on_enter(enter_sends)
            .enabled(p.enabled)
            .on_input(move |text: String| on_change(text));
        if let Some(send) = send.clone() {
            field = field.on_submit(send);
        }
        if let Some(change) = p.field {
            field = change(field);
        }
        let mut button = IconButton::new(p.send_icon)
            .variant(IconButtonVariant::Filled)
            .label(p.strings.send.clone())
            .enabled(send.is_some());
        if let Some(send) = send {
            button = button.on_press(send);
        }
        let mut row: Flex<Msg> = Flex::row().gap(8.0).align(Align::End).padding(8.0);
        if let Some(attachment) = p.attachment {
            row = row.child_boxed(attachment);
        }
        Box::new(row.child(Expanded::new(field)).child(button)) as Box<dyn Widget<Msg>>
    }))
}

#[cfg(test)]
mod tests;
