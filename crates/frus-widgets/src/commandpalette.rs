// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`CommandPalette`], [`Command`] and [`CommandPaletteState`]: a window over the page that
//! finds a command by typing a few letters of its name, and runs it (milestone 809).
//!
//! # Where "is it open" lives
//!
//! In the application, as a [`CommandPaletteState`]: whether it is open, what has been typed,
//! and which result is highlighted. The palette is sent the new state for every key — a
//! letter typed, an arrow, Escape — and the command's own message when one is chosen.

use std::rc::Rc;

use frus_core::{
    BorderRadius, BorderSide, Color, Insets, Point, Rect, ResolvedTextStyle, Scene, ShapeBorder,
    Size, TextRun, TextStyle,
};
use frus_layout::{Dimension, FlexDirection, Style};

use crate::flex::Flex;
use crate::hovercard::{Surface, SurfaceStyle};
use crate::icons::{IconData, Icons};
use crate::interaction::Status;
use crate::portal::Placement;
use crate::scroll::SingleChildScrollView;
use crate::shortcuts::{KeyStroke, ShortcutKey};
use crate::textinput::TextField;
use crate::theme::Theme;
use crate::widget::Widget;

/// How wide the palette is, unless said otherwise.
const WIDTH: f32 = 560.0;
/// How tall the list of results is: always, so that the field does not move as the list
/// grows and shrinks under it.
const LIST_HEIGHT: f32 = 320.0;
/// One result's height.
const ROW_HEIGHT: f32 = 40.0;
/// A group's heading's height.
const GROUP_HEIGHT: f32 = 28.0;
/// The room either side of a row's words.
const PAD_X: f32 = 12.0;
/// An icon's size, and the room after it.
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 12.0;
/// The least room between a label and its keys.
const SHORTCUT_GAP: f32 = 24.0;

/// **How well `query` matches `text`**, and which characters of `text` matched, or `None` when
/// it does not.
///
/// The query's characters must appear in the text **in order**, not necessarily side by side,
/// ignoring case and the spaces typed: "gtst" finds "Go to settings". Among the texts that
/// match, a match scores higher where its characters run together, where they start words,
/// and the earlier it starts; a text that starts with the query scores highest. An empty query
/// matches everything, equally, and marks nothing.
pub fn fuzzy_match(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let wanted: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if wanted.is_empty() {
        return Some((0, Vec::new()));
    }
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let starts_word = |i: usize| {
        i == 0 || {
            let before = chars[i - 1];
            !before.is_alphanumeric() || (before.is_lowercase() && chars[i].is_uppercase())
        }
    };
    // Greedy, but preferring a word start for each character when there is one ahead: the
    // marks land where a reader would put them ("Go To Settings" for "gts").
    let mut found = Vec::with_capacity(wanted.len());
    let mut from = 0;
    for (k, &want) in wanted.iter().enumerate() {
        let rest_fits = |at: usize| {
            // The characters after this one must still fit after `at`.
            let mut j = at + 1;
            for &next in &wanted[k + 1..] {
                match lower[j..].iter().position(|&c| c == next) {
                    Some(p) => j += p + 1,
                    None => return false,
                }
            }
            true
        };
        let first = lower[from..].iter().position(|&c| c == want)? + from;
        let adjacent = found.last().is_some_and(|&last: &usize| last + 1 == first);
        let at = if adjacent {
            first
        } else {
            (first..lower.len())
                .find(|&i| lower[i] == want && starts_word(i) && rest_fits(i))
                .unwrap_or(first)
        };
        found.push(at);
        from = at + 1;
    }
    let mut score = 0;
    for (k, &i) in found.iter().enumerate() {
        score += 1;
        if starts_word(i) {
            score += 8;
        }
        if k > 0 && found[k - 1] + 1 == i {
            score += 5;
        }
    }
    score -= found[0] as i32;
    if found[0] == 0 && found.windows(2).all(|w| w[0] + 1 == w[1]) {
        score += 100;
    }
    Some((score, found))
}

/// **One command a [`CommandPalette`] offers**: what it is called, what it sends, and how it
/// is found and shown.
pub struct Command<Msg = crate::callback::Callback> {
    label: String,
    message: Msg,
    group: Option<String>,
    shortcut: Option<String>,
    keywords: Vec<String>,
    icon: Option<IconData>,
    enabled: bool,
}

impl<Msg> Command<Msg> {
    /// A command called `label`, sending `message` when it is chosen.
    pub fn new(label: impl Into<String>, message: Msg) -> Self {
        Self {
            label: label.into(),
            message,
            group: None,
            shortcut: None,
            keywords: Vec::new(),
            icon: None,
            enabled: true,
        }
    }

    /// The **group** it is listed under: "Navigation", "File". Commands of a group are listed
    /// together, under its heading, in the order the groups first appear.
    #[must_use]
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// The **keys** that run it elsewhere, shown on the right: `"Ctrl+O"`. Shown and read
    /// out, not bound — binding is the application's.
    #[must_use]
    pub fn shortcut(mut self, keys: impl Into<String>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    /// Other words it is **found by**, not shown: "preferences" for "Settings".
    #[must_use]
    pub fn keywords<S: Into<String>>(mut self, words: impl IntoIterator<Item = S>) -> Self {
        self.keywords = words.into_iter().map(Into::into).collect();
        self
    }

    /// A picture before its label.
    #[must_use]
    pub fn icon(mut self, icon: IconData) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Whether it can be run now. An unavailable command is still listed, greyed, and is
    /// passed over by the arrows.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// **Whether a [`CommandPalette`] is open, what has been typed, and which result is
/// highlighted.** Kept by the application, and given back to it, changed, through the
/// palette's message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandPaletteState {
    open: bool,
    query: String,
    highlighted: usize,
}

impl CommandPaletteState {
    /// The palette closed.
    pub fn closed() -> Self {
        Self::default()
    }

    /// The palette open, with nothing typed and the first result highlighted.
    pub fn opened() -> Self {
        Self {
            open: true,
            ..Self::default()
        }
    }

    /// Whether the palette is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// What has been typed.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Which result is highlighted, counting from the first one listed.
    pub fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// The state with `query` typed: the first result is highlighted again.
    pub fn with_query(&self, query: impl Into<String>) -> Self {
        Self {
            open: self.open,
            query: query.into(),
            highlighted: 0,
        }
    }
}

/// Defaults for [`CommandPalette`] (milestone 809). Every field unset is the framework's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CommandPaletteTheme {
    /// The palette's surface. Unset, `surface_container`.
    pub background: Option<Color>,
    /// What shape it is. Unset, rounded by the theme's radius.
    pub shape: Option<ShapeBorder>,
    /// How far off the page it sits. Unset, three.
    pub elevation: Option<f32>,
    /// The colour of its shadow; [`Color::TRANSPARENT`] casts none.
    pub shadow_color: Option<Color>,
    /// Its outline. Unset, a hairline in `outline_variant`.
    pub border: Option<BorderSide>,
    /// The room inside it. Unset, eight.
    pub padding: Option<Insets>,
    /// How wide it is. Unset, 560 — never wider than the window.
    pub width: Option<f32>,
    /// How tall the list of results is. Unset, 320.
    pub list_height: Option<f32>,
    /// One result's height. Unset, forty.
    pub row_height: Option<f32>,
    /// The results' type. Unset, `body_large`.
    pub text_style: Option<TextStyle>,
    /// The results' colour. Unset, `on_surface`.
    pub text_color: Option<Color>,
    /// The colour of the letters a query matched. Unset, `primary`.
    pub match_color: Option<Color>,
    /// The highlighted result's background. Unset, `on_surface` at twelve per cent.
    pub highlight_color: Option<Color>,
    /// The groups' headings' type. Unset, `label_medium`.
    pub group_text_style: Option<TextStyle>,
    /// Their colour. Unset, the theme's muted ink.
    pub group_color: Option<Color>,
    /// The keys' colour on the right of a result. Unset, the theme's muted ink.
    pub shortcut_color: Option<Color>,
    /// The scrim over the page. Unset, the scheme's scrim at half strength.
    pub scrim: Option<Color>,
}

impl CommandPaletteTheme {
    fn surface(theme: &Theme) -> SurfaceStyle {
        let t = &theme.widgets.command_palette;
        SurfaceStyle {
            background: t.background,
            shape: t.shape,
            elevation: t.elevation,
            shadow_color: t.shadow_color,
            border: t.border,
            padding: t.padding.or(Some(Insets::uniform(8.0))),
            width: t.width.or(Some(WIDTH)),
            max_width: None,
        }
    }
}

/// What a caller said about the list, every word optional.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ListStyle {
    list_height: Option<f32>,
    row_height: Option<f32>,
    text_style: Option<TextStyle>,
    text_color: Option<Color>,
    match_color: Option<Color>,
    highlight_color: Option<Color>,
    group_text_style: Option<TextStyle>,
    group_color: Option<Color>,
    shortcut_color: Option<Color>,
    scrim: Option<Color>,
}

impl ListStyle {
    fn row_height(&self, theme: Option<&Theme>) -> f32 {
        let style = self.text(theme);
        let floor = self
            .row_height
            .or(theme.and_then(|t| t.widgets.command_palette.row_height))
            .unwrap_or(ROW_HEIGHT);
        frus_text::line_box(floor, &style, 0.0)
    }

    fn group_height(&self, theme: Option<&Theme>) -> f32 {
        frus_text::line_box(GROUP_HEIGHT, &self.group(theme), 0.0)
    }

    fn list_height(&self, theme: &Theme) -> f32 {
        self.list_height
            .or(theme.widgets.command_palette.list_height)
            .unwrap_or(LIST_HEIGHT)
    }

    fn text(&self, theme: Option<&Theme>) -> ResolvedTextStyle {
        self.text_style
            .or(theme.and_then(|t| t.widgets.command_palette.text_style))
            .unwrap_or_else(|| crate::theme::type_scale(theme).body_large)
            .resolved()
    }

    fn group(&self, theme: Option<&Theme>) -> ResolvedTextStyle {
        self.group_text_style
            .or(theme.and_then(|t| t.widgets.command_palette.group_text_style))
            .unwrap_or_else(|| crate::theme::type_scale(theme).label_medium)
            .resolved()
    }
}

/// One line of the list: a group's heading, or a result.
enum Line {
    Group(String),
    /// The command's index, the characters its label matched, and its place among the results.
    Result(usize, Vec<usize>, usize),
}

/// **The results for `query`** among `commands`: in their groups, the groups in the order they
/// first appear; within a group by how well they match, unless `filter` is off, when every
/// command is listed in the order given. A command found only by a keyword ranks under one
/// found by its label.
fn results<Msg>(commands: &[Command<Msg>], query: &str, filter: bool) -> Vec<(usize, Vec<usize>)> {
    let mut scored: Vec<(usize, i32, Vec<usize>)> = Vec::new();
    for (index, command) in commands.iter().enumerate() {
        let by_label = fuzzy_match(query, &command.label);
        let found = match by_label {
            Some((score, marks)) => Some((score, marks)),
            None => command
                .keywords
                .iter()
                .filter_map(|word| fuzzy_match(query, word))
                .map(|(score, _)| score - 50)
                .max()
                .map(|score| (score, Vec::new())),
        };
        match (found, filter) {
            (Some((score, marks)), _) => scored.push((index, score, marks)),
            (None, false) => scored.push((index, i32::MIN, Vec::new())),
            (None, true) => {}
        }
    }
    let mut groups: Vec<Option<&str>> = Vec::new();
    for (index, _, _) in &scored {
        let group = commands[*index].group.as_deref();
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    let ranked = filter && !query.trim().is_empty();
    let mut out = Vec::with_capacity(scored.len());
    for group in groups {
        let mut members: Vec<&(usize, i32, Vec<usize>)> = scored
            .iter()
            .filter(|(i, _, _)| commands[*i].group.as_deref() == group)
            .collect();
        if ranked {
            // Stable: equal scores keep the order the commands were given in.
            members.sort_by(|a, b| b.1.cmp(&a.1));
        }
        out.extend(members.into_iter().map(|(i, _, m)| (*i, m.clone())));
    }
    out
}

/// **A window over the page that finds a command by typing**, and runs it — every action of an
/// application in one place, reached from the keyboard.
///
/// ```
/// use frus_widgets::{Command, CommandPalette, CommandPaletteState, KeyStroke, ShortcutKey, Text};
///
/// #[derive(Clone)]
/// enum Msg { Palette(CommandPaletteState), Open, Save, Settings }
///
/// let state = CommandPaletteState::closed();
/// let page = Text::new("The page");
/// let palette = CommandPalette::new(page, &state, Msg::Palette)
///     .open_on(KeyStroke::new(ShortcutKey::Char('k')).ctrl())
///     .command(Command::new("Open file", Msg::Open).group("File").shortcut("Ctrl+O"))
///     .command(Command::new("Save", Msg::Save).group("File").shortcut("Ctrl+S"))
///     .command(Command::new("Settings", Msg::Settings).keywords(["preferences"]));
/// # let _: CommandPalette<Msg> = palette;
/// ```
///
/// - **Typing filters the commands** by a fuzzy match: the letters typed must appear in the
///   name in order, and the letters that matched are drawn in the match colour. A command is
///   also found by its [`keywords`](Command::keywords). Results are listed under their
///   [groups](Command::group), each with its [keys](Command::shortcut) on the right.
/// - **The keyboard drives it**: Up and Down move the highlight (round the ends, past
///   unavailable commands), Enter runs the highlighted command, Escape closes the palette. So
///   does a press outside it. The key that [opens](Self::open_on) it closes it again.
/// - **Nothing found** shows [`empty_text`](Self::empty_text) ("No results."), and an
///   application fetching results itself says so with [`loading`](Self::loading) and turns off
///   the palette's own [`filter`](Self::filter).
///
/// A command's message is sent when it is chosen. **Close the palette when you handle it**
/// (`self.palette = CommandPaletteState::closed()`), as a menu's rows are closed: a choice is
/// one message, and it is the command's.
///
/// Every part of its look is the caller's, then [`CommandPaletteTheme`]'s, then the
/// framework's. It reads to a screen reader as a dialog with its [`label`](Self::label).
pub struct CommandPalette<Msg = crate::callback::Callback> {
    state: CommandPaletteState,
    on_change: Rc<dyn Fn(CommandPaletteState) -> Msg>,
    commands: Vec<Command<Msg>>,
    open_on: Option<KeyStroke>,
    placeholder: String,
    empty_text: String,
    loading_text: String,
    label: String,
    loading: bool,
    filter: bool,
    surface: SurfaceStyle,
    list: ListStyle,
    /// The key that opens or closes the palette, with the message it sends.
    bindings: Vec<(KeyStroke, Msg)>,
    /// `[page]`, or `[page, palette]` while it is open.
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone + 'static> CommandPalette<Msg> {
    /// A palette over `page`, open as `state` says, sending `on_change` the state it should
    /// have next.
    pub fn new<R1: crate::callback::IntoMsg<Msg>>(
        page: impl Widget<Msg> + 'static,
        state: &CommandPaletteState,
        on_change: impl Fn(CommandPaletteState) -> R1 + 'static,
    ) -> Self {
        let on_change = crate::callback::handler1(on_change);
        Self {
            state: state.clone(),
            on_change: Rc::new(on_change),
            commands: Vec::new(),
            open_on: None,
            placeholder: "Type a command or search…".into(),
            empty_text: "No results.".into(),
            loading_text: "Loading…".into(),
            label: "Command palette".into(),
            loading: false,
            filter: true,
            surface: SurfaceStyle::default(),
            list: ListStyle::default(),
            bindings: Vec::new(),
            children: vec![Box::new(page)],
        }
    }

    /// Adds a command, after the others.
    #[must_use]
    pub fn command(mut self, command: Command<Msg>) -> Self {
        self.commands.push(command);
        self.rebuilt()
    }

    /// Adds commands, after the others.
    #[must_use]
    pub fn commands(mut self, commands: impl IntoIterator<Item = Command<Msg>>) -> Self {
        self.commands.extend(commands);
        self.rebuilt()
    }

    /// **The key that opens the palette** — and closes it again — wherever the focus is in the
    /// page: `Ctrl+K` is a common choice. Without one, the application opens it itself.
    #[must_use]
    pub fn open_on(mut self, stroke: KeyStroke) -> Self {
        self.open_on = Some(stroke);
        self.rebuilt()
    }

    /// What the field says before anything is typed.
    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self.rebuilt()
    }

    /// What the list says when nothing matches. "No results.", unless told.
    #[must_use]
    pub fn empty_text(mut self, text: impl Into<String>) -> Self {
        self.empty_text = text.into();
        self.rebuilt()
    }

    /// What the list says while results are on their way. "Loading…", unless told.
    #[must_use]
    pub fn loading_text(mut self, text: impl Into<String>) -> Self {
        self.loading_text = text.into();
        self.rebuilt()
    }

    /// What a screen reader calls the palette. "Command palette", unless told.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self.rebuilt()
    }

    /// **Results are on their way**: the list says so under what it already has. For an
    /// application that fetches its commands for each query — from a server, a search index
    /// — as the query changes in the state it is sent.
    #[must_use]
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self.rebuilt()
    }

    /// Whether the palette **filters** the commands by the query itself. On, unless told; an
    /// application that already chose the results for the query turns it off, and they are
    /// listed as given, the letters that match still marked.
    #[must_use]
    pub fn filter(mut self, filter: bool) -> Self {
        self.filter = filter;
        self.rebuilt()
    }

    /// The palette's surface, over the theme's and `surface_container`.
    #[must_use]
    pub fn background(mut self, color: Color) -> Self {
        self.surface.background = Some(color);
        self.rebuilt()
    }

    /// What shape it is, over the theme's and the theme's corner.
    #[must_use]
    pub fn shape(mut self, shape: ShapeBorder) -> Self {
        self.surface.shape = Some(shape);
        self.rebuilt()
    }

    /// The shorthand for a rounded rectangle.
    #[must_use]
    pub fn radius(self, radius: impl Into<BorderRadius>) -> Self {
        self.shape(ShapeBorder::rounded(radius.into()))
    }

    /// How far off the page it sits, over the theme's and three.
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.surface.elevation = Some(elevation);
        self.rebuilt()
    }

    /// The colour of its shadow; [`Color::TRANSPARENT`] casts none.
    #[must_use]
    pub fn shadow_color(mut self, color: Color) -> Self {
        self.surface.shadow_color = Some(color);
        self.rebuilt()
    }

    /// Its outline, over the theme's and a hairline in `outline_variant`.
    #[must_use]
    pub fn border(mut self, side: BorderSide) -> Self {
        self.surface.border = Some(side);
        self.rebuilt()
    }

    /// The room inside it, over the theme's and eight.
    #[must_use]
    pub fn padding(mut self, padding: Insets) -> Self {
        self.surface.padding = Some(padding);
        self.rebuilt()
    }

    /// How wide it is, over the theme's and 560. Never wider than the window.
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.surface.width = Some(width);
        self.rebuilt()
    }

    /// How tall the list of results is, over the theme's and 320. It keeps that height
    /// however many results there are, so the field never moves.
    #[must_use]
    pub fn list_height(mut self, height: f32) -> Self {
        self.list.list_height = Some(height);
        self.rebuilt()
    }

    /// One result's height, over the theme's and forty; it grows if its type needs more.
    #[must_use]
    pub fn row_height(mut self, height: f32) -> Self {
        self.list.row_height = Some(height);
        self.rebuilt()
    }

    /// The results' type, over the theme's and `body_large`.
    #[must_use]
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.list.text_style = Some(style);
        self.rebuilt()
    }

    /// The results' colour, over the theme's and `on_surface`.
    #[must_use]
    pub fn text_color(mut self, color: Color) -> Self {
        self.list.text_color = Some(color);
        self.rebuilt()
    }

    /// The colour of the letters a query matched, over the theme's and `primary`.
    #[must_use]
    pub fn match_color(mut self, color: Color) -> Self {
        self.list.match_color = Some(color);
        self.rebuilt()
    }

    /// The highlighted result's background, over the theme's.
    #[must_use]
    pub fn highlight_color(mut self, color: Color) -> Self {
        self.list.highlight_color = Some(color);
        self.rebuilt()
    }

    /// The groups' headings' type, over the theme's and `label_medium`.
    #[must_use]
    pub fn group_text_style(mut self, style: TextStyle) -> Self {
        self.list.group_text_style = Some(style);
        self.rebuilt()
    }

    /// The groups' headings' colour, over the theme's and the muted ink.
    #[must_use]
    pub fn group_color(mut self, color: Color) -> Self {
        self.list.group_color = Some(color);
        self.rebuilt()
    }

    /// The keys' colour, over the theme's and the muted ink.
    #[must_use]
    pub fn shortcut_color(mut self, color: Color) -> Self {
        self.list.shortcut_color = Some(color);
        self.rebuilt()
    }

    /// The scrim over the page, alpha included; [`Color::TRANSPARENT`] darkens nothing.
    #[must_use]
    pub fn scrim(mut self, color: Color) -> Self {
        self.list.scrim = Some(color);
        self.rebuilt()
    }

    fn send(&self, state: CommandPaletteState) -> Msg {
        (self.on_change)(state)
    }

    /// Builds the palette again (child 1), from everything said so far.
    fn rebuilt(mut self) -> Self {
        self.bindings.clear();
        if let Some(stroke) = self.open_on {
            let next = if self.state.open {
                CommandPaletteState::closed()
            } else {
                CommandPaletteState::opened()
            };
            self.bindings.push((stroke, self.send(next)));
        }
        self.children.truncate(1);
        if !self.state.open {
            return self;
        }
        let found = results(&self.commands, &self.state.query, self.filter);
        // The results that can be run, by their place in the list: what the arrows move over.
        let runnable: Vec<usize> = (0..found.len())
            .filter(|&k| self.commands[found[k].0].enabled)
            .collect();
        let highlighted = if runnable.is_empty() {
            None
        } else {
            let wanted = self.state.highlighted.min(found.len().saturating_sub(1));
            Some(
                runnable
                    .iter()
                    .copied()
                    .find(|&k| k >= wanted)
                    .unwrap_or(runnable[runnable.len() - 1]),
            )
        };
        let step = |by: isize| -> Msg {
            let next = match (highlighted, runnable.is_empty()) {
                (_, true) => 0,
                (None, false) => runnable[0],
                (Some(at), false) => {
                    let place = runnable.iter().position(|&k| k == at).unwrap_or(0) as isize;
                    let len = runnable.len() as isize;
                    runnable[(place + by).rem_euclid(len) as usize]
                }
            };
            self.send(CommandPaletteState {
                highlighted: next,
                ..self.state.clone()
            })
        };
        let run = highlighted.map(|k| self.commands[found[k].0].message.clone());
        let mut keys = vec![
            (KeyStroke::new(ShortcutKey::Down), step(1)),
            (KeyStroke::new(ShortcutKey::Up), step(-1)),
        ];
        if let Some(run) = &run {
            keys.push((KeyStroke::new(ShortcutKey::Enter), run.clone()));
        }
        // The lines: a heading wherever the group changes (and the group has a name).
        let mut lines = Vec::with_capacity(found.len() + 4);
        let mut group: Option<Option<String>> = None;
        for (place, (index, marks)) in found.iter().enumerate() {
            let this = self.commands[*index].group.clone();
            if group.as_ref() != Some(&this) {
                if let Some(name) = &this {
                    lines.push(Line::Group(name.clone()));
                }
                group = Some(this);
            }
            lines.push(Line::Result(*index, marks.clone(), place));
        }
        let rows: Vec<Box<dyn Widget<Msg>>> = lines
            .into_iter()
            .map(|line| -> Box<dyn Widget<Msg>> {
                match line {
                    Line::Group(name) => Box::new(GroupHeading {
                        label: name,
                        style: self.list,
                    }),
                    Line::Result(index, marks, place) => {
                        let command = &self.commands[index];
                        Box::new(ResultRow {
                            label: command.label.clone(),
                            marks,
                            shortcut: command.shortcut.clone(),
                            icon: command.icon,
                            enabled: command.enabled,
                            highlighted: highlighted == Some(place),
                            style: self.list,
                            message: command.message.clone(),
                        })
                    }
                }
            })
            .collect();
        let status = if found.is_empty() && !self.loading {
            Some(self.empty_text.clone())
        } else if self.loading {
            Some(self.loading_text.clone())
        } else {
            None
        };
        let list = ResultList {
            children: rows,
            note: status,
            style: self.list,
        };
        let mut field = TextField::new(self.state.query.clone())
            .borderless()
            .prefix_icon(Icons::SEARCH)
            .placeholder(self.placeholder.clone())
            .autofocus(true);
        {
            let on_change = self.on_change.clone();
            let state = self.state.clone();
            field = field.on_input(move |query: String| on_change(state.with_query(query)));
        }
        if let Some(run) = run {
            field = field.on_submit(run);
        }
        let list_height = ListHeight {
            style: self.list,
            child: vec![Box::new(
                SingleChildScrollView::new()
                    .scrollbars(crate::physics::Scrollbars::Never)
                    .child(list),
            )],
        };
        let body = Flex::column().child(field).child(list_height);
        let keyed = crate::CallbackShortcuts::new(body);
        let keyed = keys
            .into_iter()
            .fold(keyed, |keyed, (stroke, msg)| keyed.bind(stroke, msg));
        self.children.push(Box::new(Surface {
            children: vec![Box::new(keyed)],
            style: self.surface,
            gap: Some(0.0),
            themed_gap: |_| 0.0,
            semantics: Some(
                frus_core::SemanticsProperties::new(frus_core::Role::Dialog)
                    .label(self.label.clone()),
            ),
            themed: CommandPaletteTheme::surface,
        }));
        self
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for CommandPalette<Msg> {
    fn style(&self) -> Style {
        Style {
            flex_direction: FlexDirection::Column,
            ..Default::default()
        }
    }

    /// The palette is not a box round its page: it is the page, with something that may float
    /// over it.
    fn style_themed(&self, theme: &Theme) -> Style {
        match self.children.first() {
            Some(page) => Style {
                padding: Insets::ZERO,
                ..page.style_themed(theme)
            },
            None => Style::default(),
        }
    }

    fn fill_axes(&self, theme: &Theme) -> crate::widget::FillAxes {
        self.children
            .first()
            .map_or(crate::widget::FillAxes::NONE, |page| page.fill_axes(theme))
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn shortcut_callbacks(&self) -> &[(KeyStroke, Msg)] {
        &self.bindings
    }

    fn overlay(&self) -> Option<(&dyn Widget<Msg>, Placement)> {
        self.children
            .get(1)
            .map(|palette| (palette.as_ref(), Placement::Center))
    }

    fn overlay_dismiss(&self) -> Option<Msg> {
        self.state
            .open
            .then(|| self.send(CommandPaletteState::closed()))
    }

    fn overlay_scrim(&self, theme: &Theme) -> Option<Color> {
        self.list.scrim.or(theme.widgets.command_palette.scrim)
    }

    fn overlay_traps_focus(&self) -> bool {
        self.state.open
    }

    /// Escape closes the palette, from wherever the focus is inside it.
    fn on_key(&self, key: &crate::Key) -> crate::KeyResponse<Msg> {
        // The palette is open or shut; its commands, not it, are enabled or not.
        match key {
            crate::Key::Escape if self.state.open => {
                crate::KeyResponse::Handled(Some(self.send(CommandPaletteState::closed())))
            }
            _ => crate::KeyResponse::Ignored,
        }
    }

    fn debug_name(&self) -> &'static str {
        "CommandPalette"
    }
}

/// The list's fixed height, round the scroll that holds the results.
struct ListHeight<Msg> {
    style: ListStyle,
    child: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone> Widget<Msg> for ListHeight<Msg> {
    fn style(&self) -> Style {
        self.style_themed(&Theme::default())
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        Style {
            flex_direction: FlexDirection::Column,
            height: Dimension::Length(self.style.list_height(theme)),
            padding: Insets::new(4.0, 0.0, 0.0, 0.0),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.child
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }
}

/// The results, one under the other, with a note under them when there is nothing to show or
/// more on the way. It asks its scroll to keep the highlighted result in view.
struct ResultList<Msg> {
    children: Vec<Box<dyn Widget<Msg>>>,
    note: Option<String>,
    style: ListStyle,
}

impl<Msg: Clone> ResultList<Msg> {
    fn note_height(&self, theme: Option<&Theme>) -> f32 {
        if self.note.is_some() {
            self.style.row_height(theme) * 2.0
        } else {
            0.0
        }
    }
}

impl<Msg: Clone> Widget<Msg> for ResultList<Msg> {
    fn style(&self) -> Style {
        Style {
            flex_direction: FlexDirection::Column,
            padding: Insets::new(0.0, 0.0, self.note_height(None), 0.0),
            ..Default::default()
        }
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        Style {
            flex_direction: FlexDirection::Column,
            padding: Insets::new(0.0, 0.0, self.note_height(Some(theme)), 0.0),
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes {
            horizontal: true,
            vertical: false,
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let Some(note) = &self.note else {
            return;
        };
        let style = self.style.text(Some(theme));
        let height = self.note_height(Some(theme));
        let width = frus_text::measure_resolved(note, &style).width;
        let y = bounds.y + bounds.height - height + (height - style.line_height()) * 0.5;
        scene.text(
            Point::new(bounds.x + (bounds.width - width) * 0.5, y),
            note.clone(),
            &style,
            theme.muted.fade(status.opacity),
        );
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    /// The highlighted result, so that the arrows never take it out of sight.
    fn keep_visible(&self, size: Size, theme: &Theme) -> Option<crate::ui::KeepVisible> {
        let row = self.style.row_height(Some(theme));
        let heading = self.style.group_height(Some(theme));
        let mut y = 0.0;
        for (place, child) in self.children.iter().enumerate() {
            match child.debug_name() {
                "CommandPaletteGroup" => y += heading,
                _ => {
                    if child.debug_name() == "CommandPaletteRowHighlighted" {
                        return Some(crate::ui::KeepVisible {
                            key: place as u64,
                            rect: Rect::new(0.0, y, size.width, row),
                            centre: false,
                        });
                    }
                    y += row;
                }
            }
        }
        None
    }
}

/// A group's heading.
struct GroupHeading {
    label: String,
    style: ListStyle,
}

impl<Msg> Widget<Msg> for GroupHeading {
    fn style(&self) -> Style {
        Style {
            height: Dimension::Length(self.style.group_height(None)),
            ..Default::default()
        }
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        Style {
            height: Dimension::Length(self.style.group_height(Some(theme))),
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes {
            horizontal: true,
            vertical: false,
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let style = self.style.group(Some(theme));
        let colour = self
            .style
            .group_color
            .or(theme.widgets.command_palette.group_color)
            .unwrap_or(theme.muted);
        let width = frus_text::measure_resolved(&self.label, &style).width;
        let x = if theme.direction.is_rtl() {
            bounds.x + bounds.width - PAD_X - width
        } else {
            bounds.x + PAD_X
        };
        scene.text(
            Point::new(x, bounds.y + bounds.height - style.line_height() - 4.0),
            self.label.clone(),
            &style,
            colour.fade(status.opacity),
        );
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        // A heading answers nothing, so it is never enabled or disabled.
        Some(
            frus_core::SemanticsProperties::new(frus_core::Role::Heading).label(self.label.clone()),
        )
    }

    fn debug_name(&self) -> &'static str {
        "CommandPaletteGroup"
    }
}

/// One result: its icon, its label with the matched letters marked, its keys on the right.
struct ResultRow<Msg> {
    label: String,
    marks: Vec<usize>,
    shortcut: Option<String>,
    icon: Option<IconData>,
    enabled: bool,
    highlighted: bool,
    style: ListStyle,
    message: Msg,
}

impl<Msg> ResultRow<Msg> {
    /// The label as runs: the matched letters in the match colour and a heavier weight.
    fn runs(&self, style: &ResolvedTextStyle, ink: Color, mark: Color) -> Vec<TextRun> {
        let run = |text: String, colour: Color, matched: bool| TextRun {
            text,
            size: style.size,
            weight: if matched {
                frus_core::FontWeight::Bold
            } else {
                style.weight
            },
            italic: style.italic,
            color: colour,
            decoration: style.decoration,
            decoration_color: None,
            letter_spacing: style.letter_spacing,
            shaping: style.shaping.clone(),
        };
        let mut runs: Vec<TextRun> = Vec::new();
        let mut current = String::new();
        let mut matched = false;
        for (i, c) in self.label.chars().enumerate() {
            let this = self.marks.contains(&i);
            if this != matched && !current.is_empty() {
                let colour = if matched { mark } else { ink };
                runs.push(run(std::mem::take(&mut current), colour, matched));
            }
            matched = this;
            current.push(c);
        }
        if !current.is_empty() {
            runs.push(run(current, if matched { mark } else { ink }, matched));
        }
        runs
    }
}

impl<Msg: Clone> Widget<Msg> for ResultRow<Msg> {
    fn style(&self) -> Style {
        Style {
            height: Dimension::Length(self.style.row_height(None)),
            ..Default::default()
        }
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        Style {
            height: Dimension::Length(self.style.row_height(Some(theme))),
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes {
            horizontal: true,
            vertical: false,
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let o = status.opacity;
        let t = &theme.widgets.command_palette;
        let s = &self.style;
        let surface = t.background.unwrap_or(theme.scheme.surface_container);
        // The highlighted result keeps the tint the pointer gives the others.
        let lit = s
            .highlight_color
            .or(t.highlight_color)
            .unwrap_or_else(|| surface.lerp(theme.on_surface, 0.12));
        if self.highlighted {
            scene.draw_rect(
                bounds,
                lit.fade(o),
                theme.radius.min(8.0),
                0.0,
                Color::TRANSPARENT,
            );
        } else if self.enabled {
            let tinted = theme.state_layer(surface, theme.on_surface, &status);
            if tinted != surface {
                scene.draw_rect(
                    bounds,
                    tinted.fade(o),
                    theme.radius.min(8.0),
                    0.0,
                    Color::TRANSPARENT,
                );
            }
        }
        let ink = if self.enabled {
            s.text_color.or(t.text_color).unwrap_or(theme.on_surface)
        } else {
            crate::disabled::disabled_content(theme)
        };
        let mark = if self.enabled {
            s.match_color.or(t.match_color).unwrap_or(theme.primary)
        } else {
            ink
        };
        let style = s.text(Some(theme));
        let rtl = theme.direction.is_rtl();
        let at = |x: f32, width: f32| {
            if rtl {
                bounds.x + bounds.width - (x - bounds.x) - width
            } else {
                x
            }
        };
        let mut x = bounds.x + PAD_X;
        if let Some(icon) = self.icon {
            let y = bounds.y + (bounds.height - ICON) * 0.5;
            let path = icon.placed(ICON, at(x, ICON), y, theme.direction);
            scene.fill_path(&path, ink.fade(o));
            x += ICON + ICON_GAP;
        }
        let ty = bounds.y + (bounds.height - style.line_height()) * 0.5;
        let runs = self.runs(&style, ink.fade(o), mark.fade(o));
        let width = frus_text::measure_runs(&runs).width;
        scene.rich_text(Point::new(at(x, width), ty), runs);
        if let Some(keys) = &self.shortcut {
            let measured = frus_text::measure_resolved(keys, &style).width;
            let colour = s.shortcut_color.or(t.shortcut_color).unwrap_or(theme.muted);
            let kx = bounds.x + bounds.width - PAD_X - measured;
            if kx > x + width + SHORTCUT_GAP * 0.5 {
                scene.text(
                    Point::new(at(kx, measured), ty),
                    keys.clone(),
                    &style,
                    colour.fade(o),
                );
            }
        }
    }

    fn on_click(&self) -> Option<Msg> {
        self.enabled.then(|| self.message.clone())
    }

    /// A button, with its keys after its name.
    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let mut spoken = self.label.clone();
        if let Some(keys) = &self.shortcut {
            spoken.push_str(", ");
            spoken.push_str(keys);
        }
        let mut semantics = frus_core::SemanticsProperties::new(frus_core::Role::Button)
            .label(spoken)
            .disabled(!self.enabled);
        if self.enabled {
            semantics = semantics.clickable();
        }
        Some(semantics)
    }

    fn debug_name(&self) -> &'static str {
        if self.highlighted {
            "CommandPaletteRowHighlighted"
        } else {
            "CommandPaletteRow"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Container, Runtime};
    use frus_core::Primitive;

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Palette(CommandPaletteState),
        Open,
        Save,
        Settings,
        Quit,
    }

    fn marks(query: &str, text: &str) -> Option<Vec<usize>> {
        fuzzy_match(query, text).map(|(_, m)| m)
    }

    #[test]
    fn a_query_matches_letters_in_order_and_prefers_word_starts() {
        assert_eq!(marks("gts", "Go to settings"), Some(vec![0, 3, 6]));
        assert_eq!(marks("SET", "Go to settings"), Some(vec![6, 7, 8]));
        assert_eq!(marks("ts", "Go to settings"), Some(vec![3, 6]));
        assert_eq!(marks("xyz", "Go to settings"), None);
        assert_eq!(marks("sg", "Go to settings"), Some(vec![6, 12]));
        assert_eq!(marks("", "anything"), Some(vec![]));
        assert_eq!(
            marks("o f", "Open file"),
            Some(vec![0, 5]),
            "spaces are not letters"
        );
        // A prefix beats a scattered match.
        let prefix = fuzzy_match("sa", "Save").unwrap().0;
        let scattered = fuzzy_match("sa", "Select all").unwrap().0;
        assert!(prefix > scattered, "{prefix} {scattered}");
    }

    fn commands() -> Vec<Command<Msg>> {
        vec![
            Command::new("Open file", Msg::Open)
                .group("File")
                .shortcut("Ctrl+O"),
            Command::new("Save", Msg::Save)
                .group("File")
                .shortcut("Ctrl+S"),
            Command::new("Settings", Msg::Settings)
                .group("App")
                .keywords(["preferences"]),
            Command::new("Quit", Msg::Quit).group("App").enabled(false),
        ]
    }

    #[test]
    fn results_keep_their_groups_and_rank_inside_them() {
        let cmds = commands();
        let order = |q: &str| -> Vec<usize> {
            results(&cmds, q, true)
                .into_iter()
                .map(|(i, _)| i)
                .collect()
        };
        assert_eq!(order(""), vec![0, 1, 2, 3], "everything, as given");
        assert_eq!(order("s"), vec![1, 2], "Open file has no s");
        assert_eq!(order("fi"), vec![0], "fi: Open file");
        assert_eq!(
            order("e"),
            vec![0, 1, 2],
            "within File, ranked: equal scores keep their order"
        );
        assert_eq!(order("pref"), vec![2], "found by a keyword");
        assert_eq!(order("zz"), Vec::<usize>::new());
        assert_eq!(
            results(&cmds, "zz", false).len(),
            4,
            "unfiltered, everything is listed"
        );
    }

    fn palette(state: &CommandPaletteState) -> CommandPalette<Msg> {
        CommandPalette::new(
            Container::<Msg>::new().width(600.0).height(500.0),
            state,
            Msg::Palette,
        )
        .open_on(KeyStroke::new(ShortcutKey::Char('k')).ctrl())
        .commands(commands())
    }

    fn frame(widget: &CommandPalette<Msg>) -> crate::Ui<Msg> {
        build_ui(
            widget,
            Size::new(800.0, 600.0),
            &Runtime::default(),
            &Theme::default(),
        )
    }

    fn words(ui: &crate::Ui<Msg>) -> Vec<String> {
        ui.scene()
            .primitives()
            .iter()
            .filter_map(|p| match p {
                Primitive::Text { text, .. } => Some(text.clone()),
                Primitive::RichText { runs, .. } => {
                    Some(runs.iter().map(|r| r.text.as_str()).collect())
                }
                _ => None,
            })
            .collect()
    }

    /// The key it was told opens it, and closes it again.
    #[test]
    fn its_key_opens_and_closes_it() {
        let stroke = KeyStroke::new(ShortcutKey::Char('k')).ctrl();
        let closed = frame(&palette(&CommandPaletteState::closed()));
        assert_eq!(
            closed.keystroke(stroke, None),
            vec![Msg::Palette(CommandPaletteState::opened())]
        );
        assert!(
            !words(&closed).contains(&"Save".to_string()),
            "nothing shown shut"
        );
        let open = palette(&CommandPaletteState::opened());
        assert!(Widget::<Msg>::shortcut_callbacks(&open)
            .iter()
            .any(|(s, m)| *s == stroke && *m == Msg::Palette(CommandPaletteState::closed())));
    }

    /// Open, it lists the groups and the commands with their keys, in a dialog.
    #[test]
    fn open_it_lists_groups_commands_and_keys() {
        let ui = frame(&palette(&CommandPaletteState::opened()));
        let shown = words(&ui);
        for word in [
            "File",
            "Open file",
            "Ctrl+O",
            "Save",
            "App",
            "Settings",
            "Quit",
        ] {
            assert!(shown.contains(&word.to_string()), "{word}: {shown:?}");
        }
        assert!(ui
            .semantics()
            .iter()
            .any(|(_, _, s)| s.role == frus_core::Role::Dialog
                && s.label.as_deref() == Some("Command palette")));
        assert_eq!(
            ui.top_dismiss(),
            Some(Msg::Palette(CommandPaletteState::closed()))
        );
    }

    /// The arrows move the highlight round the ends and past what cannot run; Enter runs it.
    #[test]
    fn the_arrows_move_the_highlight_and_enter_runs_it() {
        let open = CommandPaletteState::opened();
        let ui = frame(&palette(&open));
        let key = |ui: &crate::Ui<Msg>, k| ui.keystroke(KeyStroke::new(k), None);
        let at = |n: usize| {
            Msg::Palette(CommandPaletteState {
                highlighted: n,
                ..open.clone()
            })
        };
        assert_eq!(key(&ui, ShortcutKey::Down), vec![at(1)]);
        assert_eq!(key(&ui, ShortcutKey::Up), vec![at(2)], "round, past Quit");
        assert_eq!(key(&ui, ShortcutKey::Enter), vec![Msg::Open]);
        let on_settings = CommandPaletteState {
            highlighted: 2,
            ..open.clone()
        };
        let ui = frame(&palette(&on_settings));
        assert_eq!(key(&ui, ShortcutKey::Down), vec![at(0)], "past Quit, round");
        assert_eq!(key(&ui, ShortcutKey::Enter), vec![Msg::Settings]);
    }

    /// Typing filters; nothing found says so; results on their way say so.
    #[test]
    fn typing_filters_and_nothing_found_says_so() {
        let typed = CommandPaletteState::opened().with_query("sav");
        let shown = words(&frame(&palette(&typed)));
        assert!(shown.contains(&"Save".to_string()));
        assert!(!shown.contains(&"Settings".to_string()), "{shown:?}");
        let none = CommandPaletteState::opened().with_query("zzz");
        assert!(words(&frame(&palette(&none))).contains(&"No results.".to_string()));
        let waiting = palette(&none).loading(true).empty_text("Nothing");
        let shown = words(&frame(&waiting));
        assert!(shown.contains(&"Loading…".to_string()) && !shown.contains(&"Nothing".to_string()));
    }

    /// The matched letters are drawn in the match colour, the caller's over the theme's.
    #[test]
    fn matched_letters_take_the_match_colour() {
        let mark = Color::rgb(0.8, 0.2, 0.6);
        let typed = CommandPaletteState::opened().with_query("sav");
        let ui = frame(&palette(&typed).match_color(mark));
        let marked: Vec<String> = ui
            .scene()
            .primitives()
            .iter()
            .filter_map(|p| match p {
                Primitive::RichText { runs, .. } => Some(runs.clone()),
                _ => None,
            })
            .flatten()
            .filter(|r| r.color == mark)
            .map(|r| r.text)
            .collect();
        assert_eq!(marked, vec!["Sav".to_string()]);
    }
}
