// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! How the workbench looks: where each panel goes, what it shows, and which panel a click
//! lands in. The places are computed once, by [`regions`], for drawing and for clicks alike.

use std::ops::Range;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

use crate::state::{
    App, Focus, Input, JobState, Line, LineKind, Pane, Row, RunStatus, Tab, BUILD_TARGETS,
};

/// The workbench's colours. [`Theme::default`] is coloured; [`Theme::plain`] uses none, for
/// a terminal without colours or a developer who set `NO_COLOR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The tab shown, the focused panel's border, the keys in the key bar.
    pub accent: Style,
    /// Panels without the focus, the build's own output, explanations.
    pub dim: Style,
    /// Failures.
    pub error: Style,
    /// What went well: an application up, a reload done, a check passed.
    pub good: Style,
    /// What the tool says.
    pub tool: Style,
    /// The entry picked in a list.
    pub selected: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            accent: Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            dim: Style::new().fg(Color::DarkGray),
            error: Style::new().fg(Color::Red),
            good: Style::new().fg(Color::Green),
            tool: Style::new().fg(Color::Blue),
            selected: Style::new().add_modifier(Modifier::REVERSED),
        }
    }
}

impl Theme {
    /// No colours: emphasis by boldness and reversal alone.
    pub fn plain() -> Self {
        Theme {
            accent: Style::new().add_modifier(Modifier::BOLD),
            dim: Style::new(),
            error: Style::new().add_modifier(Modifier::BOLD),
            good: Style::new(),
            tool: Style::new(),
            selected: Style::new().add_modifier(Modifier::REVERSED),
        }
    }
}

/// Where everything goes on a screen of the app's size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Regions {
    pub tabs: Rect,
    pub notice: Rect,
    pub keys: Rect,
    /// The Run tab's Devices and Runs panels.
    pub devices: Rect,
    pub runs: Rect,
    /// The Build tab's list of targets.
    pub list: Rect,
    /// The tab's scrolling pane: the log, the build's output, the doctor's report.
    pub pane: Rect,
}

pub(crate) fn regions(app: &App) -> Regions {
    let (width, height) = app.size;
    let line = |y: u16| Rect::new(0, y, width, u16::from(height > y));
    let mut regions = Regions {
        tabs: line(0),
        notice: line(height.saturating_sub(2)),
        keys: line(height.saturating_sub(1)),
        ..Regions::default()
    };
    let body = Rect::new(0, 1, width, height.saturating_sub(3));
    match app.tab {
        Tab::Run => {
            let left = (width * 2 / 5).clamp(28, 48).min(width / 2);
            let lines = device_lines(app).len().max(1) as u16;
            let devices = (lines + 2)
                .clamp(3, (body.height * 3 / 5).max(3))
                .min(body.height);
            regions.devices = Rect::new(0, body.y, left, devices);
            regions.runs = Rect::new(0, body.y + devices, left, body.height - devices);
            regions.pane = Rect::new(left, body.y, width - left, body.height);
        }
        Tab::Build => {
            let left = (width / 3).clamp(24, 40).min(width / 2);
            regions.list = Rect::new(0, body.y, left, body.height);
            regions.pane = Rect::new(left, body.y, width - left, body.height);
        }
        Tab::Doctor => regions.pane = body,
    }
    regions
}

/// How many lines of the tab's pane are seen.
pub(crate) fn pane_height(app: &App) -> usize {
    usize::from(regions(app).pane.height.saturating_sub(2))
}

/// What a click or the wheel lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Tab(Tab),
    /// A key of the key bar, which a click presses.
    Button(Input),
    /// The box in front of a device, which marks it.
    Mark(usize),
    DeviceRow(usize),
    RunRow(usize),
    BuildRow(usize),
    Pane,
}

fn inside(rect: Rect, column: u16, row: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && (rect.x..rect.x + rect.width).contains(&column)
        && (rect.y..rect.y + rect.height).contains(&row)
}

/// The inside of a bordered panel.
fn inner(rect: Rect) -> Rect {
    Rect::new(
        rect.x + 1,
        rect.y + 1,
        rect.width.saturating_sub(2),
        rect.height.saturating_sub(2),
    )
}

pub(crate) fn hit(app: &App, column: u16, row: u16) -> Option<Target> {
    let regions = regions(app);
    if inside(regions.tabs, column, row) {
        return tab_ranges(app)
            .into_iter()
            .find(|(_, range)| range.contains(&column))
            .map(|(tab, _)| Target::Tab(tab));
    }
    if inside(regions.keys, column, row) {
        return key_ranges(app)
            .into_iter()
            .find(|(_, range)| range.contains(&column))
            .map(|(input, _)| Target::Button(input));
    }
    if inside(regions.pane, column, row) {
        return Some(Target::Pane);
    }
    let lists = [(regions.devices, 0), (regions.runs, 1), (regions.list, 2)];
    for (rect, which) in lists {
        let inside_list = inner(rect);
        if !inside(inside_list, column, row) {
            continue;
        }
        let line = usize::from(row - inside_list.y);
        return match which {
            0 => {
                let lines = device_lines(app);
                let first = first_shown(cursor_line(app, &lines), lines.len(), inside_list.height);
                match lines.get(first + line)?.1? {
                    index
                        if column < inside_list.x + 4
                            && matches!(app.rows()[index], Row::Device(_)) =>
                    {
                        Some(Target::Mark(index))
                    }
                    index => Some(Target::DeviceRow(index)),
                }
            }
            // Two lines a run: its device, then where it is.
            1 => {
                let first = first_shown(app.run_cursor, app.runs.len(), inside_list.height / 2);
                let index = first + line / 2;
                (index < app.runs.len()).then_some(Target::RunRow(index))
            }
            _ => (line / 2 < BUILD_TARGETS.len()).then_some(Target::BuildRow(line / 2)),
        };
    }
    None
}

/// The first line a list shows so that `cursor` is seen.
fn first_shown(cursor: usize, count: usize, height: u16) -> usize {
    let height = usize::from(height.max(1));
    cursor
        .saturating_sub(height - 1)
        .min(count.saturating_sub(height))
}

/// The tab bar: the name, then each tab's label, at the columns it takes.
fn tab_ranges(app: &App) -> Vec<(Tab, Range<u16>)> {
    let _ = app;
    let mut x = " frus ".len() as u16 + 1;
    Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let width = tab_label(i, *tab).chars().count() as u16;
            let range = x..x + width;
            x += width + 1;
            (*tab, range)
        })
        .collect()
}

fn tab_label(index: usize, tab: Tab) -> String {
    format!(" {} {} ", index + 1, tab.title())
}

/// The key bar of a tab: each key, what it does, and what a click on it presses.
fn keys(app: &App) -> Vec<(&'static str, &'static str, Input)> {
    let mut keys = match app.tab {
        Tab::Run => vec![
            ("Enter", "run", Input::Enter),
            ("Space", "mark", Input::Char(' ')),
            ("r", "reload", Input::Char('r')),
            ("R", "restart", Input::Char('R')),
            ("s", "stop", Input::Char('s')),
            ("Tab", "panel", Input::Tab),
            ("c", "clear", Input::Char('c')),
        ],
        Tab::Build => vec![
            ("Enter", "build", Input::Enter),
            ("↑↓", "target", Input::Down),
            ("c", "cancel", Input::Char('c')),
        ],
        Tab::Doctor => vec![
            ("Enter", "look again", Input::Enter),
            ("f", "fix: install what is missing", Input::Char('f')),
        ],
    };
    keys.push(("q", "quit", Input::Char('q')));
    keys
}

fn key_ranges(app: &App) -> Vec<(Input, Range<u16>)> {
    let mut x = 1u16;
    keys(app)
        .into_iter()
        .map(|(key, what, input)| {
            let width = (key.chars().count() + 1 + what.chars().count()) as u16;
            let range = x..x + width;
            x += width + 2;
            (input, range)
        })
        .collect()
}

/// The Devices panel's lines, each with the entry of [`App::rows`] it shows (`None`: a heading).
fn device_lines(app: &App) -> Vec<(TextLine<'static>, Option<usize>)> {
    device_lines_styled(app, &Theme::plain())
}

fn device_lines_styled(app: &App, theme: &Theme) -> Vec<(TextLine<'static>, Option<usize>)> {
    let mut lines = Vec::new();
    if !app.devices_known {
        lines.push((TextLine::styled("Looking for devices…", theme.dim), None));
        return lines;
    }
    if app.devices.is_empty() {
        lines.push((TextLine::styled("No device found", theme.dim), None));
    }
    for (index, row) in app.rows().into_iter().enumerate() {
        if row == Row::Emulator(0) {
            lines.push((TextLine::styled("Emulators", theme.dim), None));
        }
        let selected = index == app.device_cursor;
        let line = match row {
            Row::Device(i) => {
                let device = &app.devices[i];
                let mark = if app.marked.contains(&device.id) {
                    "[x] "
                } else {
                    "[ ] "
                };
                let running = if app.live_run_on(&device.id).is_some() {
                    " ▶"
                } else {
                    ""
                };
                let style = if device.unavailable.is_some() {
                    theme.dim
                } else {
                    Style::new()
                };
                let mut spans = vec![
                    Span::raw(mark),
                    Span::styled(device.name.clone(), style),
                    Span::styled(format!(" {}", device.platform), theme.dim),
                    Span::styled(running, theme.good),
                ];
                if let Some(reason) = &device.unavailable {
                    spans.push(Span::styled(format!(" ({reason})"), theme.dim));
                }
                TextLine::from(spans)
            }
            Row::Emulator(i) => {
                let emulator = &app.emulators[i];
                TextLine::from(vec![
                    Span::raw(" ⏻  "),
                    Span::raw(emulator.name.clone()),
                    Span::styled(format!(" {}", emulator.platform), theme.dim),
                ])
            }
        };
        let line = if selected && app.focus == Focus::Devices && app.tab == Tab::Run {
            line.patch_style(theme.selected)
        } else if selected {
            line.patch_style(Style::new().add_modifier(Modifier::BOLD))
        } else {
            line
        };
        lines.push((line, Some(index)));
    }
    lines
}

fn cursor_line(app: &App, lines: &[(TextLine<'static>, Option<usize>)]) -> usize {
    lines
        .iter()
        .position(|(_, row)| *row == Some(app.device_cursor))
        .unwrap_or(0)
}

/// Draws the whole workbench.
pub fn draw(frame: &mut Frame, app: &App, theme: &Theme) {
    let regions = regions(app);
    draw_tabs(frame, app, theme, regions.tabs);
    match app.tab {
        Tab::Run => {
            draw_devices(frame, app, theme, regions.devices);
            draw_runs(frame, app, theme, regions.runs);
            let title = if app.log.scroll > 0 {
                format!("Log ({} lines below: End follows)", app.log.scroll)
            } else {
                "Log".to_string()
            };
            draw_pane(
                frame,
                &app.log,
                &title,
                app.focus == Focus::Log,
                theme,
                regions.pane,
            );
        }
        Tab::Build => {
            draw_targets(frame, app, theme, regions.list);
            let title = match (&app.build_state, &app.build_target) {
                (JobState::Running, Some(target)) => format!("frus build {target}: building…"),
                (JobState::Done { code: Some(0) }, Some(target)) => {
                    format!("frus build {target}: done")
                }
                (JobState::Done { .. }, Some(target)) => format!("frus build {target}: failed"),
                _ => "Output".to_string(),
            };
            draw_pane(frame, &app.build, &title, true, theme, regions.pane);
        }
        Tab::Doctor => {
            let title = if app.fix_state == JobState::Running {
                "frus doctor --fix: installing…"
            } else if app.doctor_state == JobState::Running {
                "frus doctor: looking…"
            } else {
                "frus doctor"
            };
            draw_pane(frame, &app.doctor, title, true, theme, regions.pane);
        }
    }
    let notice = match &app.notice {
        Some((text, true)) => Span::styled(format!(" {text}"), theme.error),
        Some((text, false)) => Span::raw(format!(" {text}")),
        None if !app.devices_known && app.tab == Tab::Run => {
            Span::styled(" Looking for devices…", theme.dim)
        }
        None => Span::raw(""),
    };
    frame.render_widget(Paragraph::new(TextLine::from(notice)), regions.notice);
    let mut spans = vec![Span::raw(" ")];
    for (key, what, _) in keys(app) {
        spans.push(Span::styled(key, theme.accent));
        spans.push(Span::raw(format!(" {what}  ")));
    }
    frame.render_widget(Paragraph::new(TextLine::from(spans)), regions.keys);
}

fn draw_tabs(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let mut spans = vec![Span::styled(" frus ", theme.accent), Span::raw(" ")];
    for (i, tab) in Tab::ALL.iter().enumerate() {
        let style = if *tab == app.tab {
            theme.selected.patch(theme.accent)
        } else {
            Style::new()
        };
        spans.push(Span::styled(tab_label(i, *tab), style));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(TextLine::from(spans)), area);
    let project = match &app.project {
        Some(name) => format!("{name} "),
        None => "not in an application ".to_string(),
    };
    let width = project.chars().count() as u16;
    let used = tab_ranges(app).last().map_or(0, |(_, r)| r.end + 1);
    if area.width > used + width {
        let right = Rect::new(area.x + area.width - width, area.y, width, 1);
        frame.render_widget(Paragraph::new(Span::styled(project, theme.dim)), right);
    }
}

fn block<'a>(title: &'a str, focused: bool, theme: &Theme) -> Block<'a> {
    Block::new()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(if focused { theme.accent } else { theme.dim })
}

fn draw_devices(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let lines = device_lines_styled(app, theme);
    let height = inner(area).height;
    let first = first_shown(cursor_line(app, &lines), lines.len(), height);
    let shown: Vec<TextLine> = lines.into_iter().skip(first).map(|(l, _)| l).collect();
    frame.render_widget(
        Paragraph::new(shown).block(block("Devices", app.focus == Focus::Devices, theme)),
        area,
    );
}

fn draw_runs(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let mut lines = Vec::new();
    if app.runs.is_empty() {
        lines.push(TextLine::styled("Enter runs on the device", theme.dim));
    }
    let first = first_shown(app.run_cursor, app.runs.len(), inner(area).height / 2);
    for (index, run) in app.runs.iter().enumerate().skip(first) {
        let (dot, status) = match &run.status {
            RunStatus::Building(what) => (Span::styled("● ", theme.tool), what.clone()),
            RunStatus::Running { hot: true } => {
                (Span::styled("● ", theme.good), "up, hot reload".to_string())
            }
            RunStatus::Running { hot: false } => (Span::styled("● ", theme.good), "up".to_string()),
            RunStatus::Reloading { full: false } => {
                (Span::styled("● ", theme.tool), "reloading…".to_string())
            }
            RunStatus::Reloading { full: true } => {
                (Span::styled("● ", theme.tool), "restarting…".to_string())
            }
            RunStatus::Stopped { error: None } => {
                (Span::styled("○ ", theme.dim), "ended".to_string())
            }
            RunStatus::Stopped { error: Some(error) } => {
                (Span::styled("○ ", theme.error), format!("failed: {error}"))
            }
        };
        let mut name = TextLine::from(vec![dot, Span::raw(run.device_name.clone())]);
        let mut status = TextLine::from(Span::styled(format!("  {status}"), theme.dim));
        if index == app.run_cursor && app.focus == Focus::Runs {
            name = name.patch_style(theme.selected);
            status = status.patch_style(theme.selected);
        }
        lines.push(name);
        lines.push(status);
    }
    frame.render_widget(
        Paragraph::new(lines).block(block("Runs", app.focus == Focus::Runs, theme)),
        area,
    );
}

fn draw_targets(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let mut lines = Vec::new();
    for (index, (name, what)) in BUILD_TARGETS.iter().enumerate() {
        let mut line = TextLine::raw(format!(" {name}"));
        if index == app.build_cursor {
            line = line.patch_style(theme.selected);
        }
        lines.push(line);
        lines.push(TextLine::styled(format!("   {what}"), theme.dim));
    }
    frame.render_widget(
        Paragraph::new(lines).block(block("frus build", false, theme)),
        area,
    );
}

/// A line cut into rows of `width` columns at most: a log line is read whole, not cut off.
fn wrapped(line: TextLine<'static>, width: usize) -> Vec<TextLine<'static>> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut row: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for span in line.spans {
        let mut text = String::new();
        for c in span.content.chars() {
            let w = UnicodeWidthChar::width(c).unwrap_or(0);
            if used + w > width && used > 0 {
                if !text.is_empty() {
                    row.push(Span::styled(std::mem::take(&mut text), span.style));
                }
                rows.push(TextLine::from(std::mem::take(&mut row)));
                used = 0;
            }
            text.push(c);
            used += w;
        }
        if !text.is_empty() {
            row.push(Span::styled(text, span.style));
        }
    }
    rows.push(TextLine::from(row));
    rows
}

fn styled_line<'a>(line: &'a Line, theme: &Theme) -> TextLine<'a> {
    let style = match line.kind {
        LineKind::App => Style::new(),
        LineKind::AppError | LineKind::ToolError => theme.error,
        LineKind::Tool => theme.tool,
        LineKind::Output => theme.dim,
        LineKind::Good => theme.good,
    };
    let mut spans = Vec::new();
    if let Some(source) = &line.source {
        spans.push(Span::styled(format!("[{source}] "), theme.dim));
    }
    spans.push(Span::styled(line.text.as_str(), style));
    TextLine::from(spans)
}

fn draw_pane(
    frame: &mut Frame,
    pane: &Pane,
    title: &str,
    focused: bool,
    theme: &Theme,
    area: Rect,
) {
    let height = usize::from(inner(area).height);
    let width = usize::from(inner(area).width);
    let scroll = pane.scroll.min(pane.lines.len().saturating_sub(height));
    let end = pane.lines.len() - scroll;
    // From the last line shown upwards, until the pane is full: the newest row at the bottom.
    let mut rows: Vec<TextLine<'static>> = Vec::new();
    for line in pane.lines[..end].iter().rev() {
        if rows.len() >= height {
            break;
        }
        let owned = TextLine::from(
            styled_line(line, theme)
                .spans
                .into_iter()
                .map(|s| Span::styled(s.content.into_owned(), s.style))
                .collect::<Vec<_>>(),
        );
        for row in wrapped(owned, width).into_iter().rev() {
            rows.push(row);
        }
    }
    rows.truncate(height);
    rows.reverse();
    let lines = rows;
    frame.render_widget(
        Paragraph::new(lines).block(block(title, focused, theme)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Device, Emulator, JobKind, Msg, RunEvent};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn device(id: &str, name: &str, platform: &str) -> Device {
        Device {
            id: id.into(),
            name: name.into(),
            platform: platform.into(),
            category: "desktop".into(),
            detail: String::new(),
            emulator: false,
            unavailable: None,
        }
    }

    /// The screen as text, a line a row.
    fn screen(app: &App) -> String {
        let (width, height) = app.size;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, app, &Theme::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            let mut row = String::new();
            for x in 0..width {
                row.push_str(buffer[(x, y)].symbol());
            }
            out.push_str(row.trim_end());
            out.push('\n');
        }
        out
    }

    /// Compares a screen with its snapshot in `snapshots/`; `FRUS_TUI_BLESS=1` writes it.
    fn snapshot(name: &str, app: &App) {
        let text = screen(app);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("snapshots")
            .join(format!("{name}.txt"));
        if std::env::var_os("FRUS_TUI_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("no snapshot {}: run with FRUS_TUI_BLESS=1", path.display()))
            .replace("\r\n", "\n");
        assert!(
            expected == text,
            "the screen `{name}` changed:\n{text}\nexpected:\n{expected}"
        );
    }

    fn running_app() -> App {
        let mut app = App::new(Some("my_app".into()));
        app.update(Msg::Devices(vec![
            device("windows", "This computer", "windows"),
            device("web", "Browser", "web"),
            {
                let mut phone = device("A1", "Phone", "android");
                phone.unavailable = Some("not authorised".into());
                phone
            },
        ]));
        app.update(Msg::Emulators(vec![Emulator {
            id: "Pixel_7".into(),
            name: "Pixel 7".into(),
            platform: "android".into(),
        }]));
        app.update(Msg::Input(Input::Enter));
        for event in [
            RunEvent::Start {
                device_name: "This computer".into(),
            },
            RunEvent::Output("   Compiling my_app v0.1.0".into()),
            RunEvent::Progress {
                message: "Built in 4.2 s".into(),
                error: false,
            },
            RunEvent::Started { hot: true },
            RunEvent::Log {
                text: "count: 1".into(),
                error: false,
            },
            RunEvent::Log {
                text: "a warning".into(),
                error: true,
            },
            RunEvent::Restarted {
                full: false,
                seconds: 0.31,
            },
        ] {
            app.update(Msg::Run { key: 1, event });
        }
        app
    }

    #[test]
    fn the_run_tab_while_looking() {
        snapshot("looking", &App::new(None));
    }

    #[test]
    fn the_run_tab_with_a_run() {
        snapshot("run", &running_app());
    }

    #[test]
    fn the_build_tab() {
        let mut app = running_app();
        app.update(Msg::Input(Input::Char('2')));
        app.update(Msg::Input(Input::Down));
        app.update(Msg::Input(Input::Enter));
        app.update(Msg::JobLine {
            job: JobKind::Build,
            text: "   Compiling my_app v0.1.0".into(),
            error: true,
        });
        app.update(Msg::JobDone {
            job: JobKind::Build,
            code: Some(0),
        });
        snapshot("build", &app);
    }

    #[test]
    fn the_doctor_tab() {
        let mut app = App::new(Some("my_app".into()));
        app.update(Msg::Input(Input::Char('3')));
        app.update(Msg::JobLine {
            job: JobKind::Doctor,
            text: r#"[{"name":"Rust","status":"ready","summary":"rustc 1.96.1","findings":[{"ok":true,"text":"cargo 1.96.1"}]},{"name":"Android","status":"partial","summary":"","findings":[{"ok":false,"text":"NDK is not installed: `frus doctor --fix`"}]}]"#.into(),
            error: false,
        });
        app.update(Msg::JobDone {
            job: JobKind::Doctor,
            code: Some(0),
        });
        snapshot("doctor", &app);
    }

    #[test]
    fn long_lines_wrap() {
        let rows = wrapped(
            TextLine::from(vec![Span::raw("[web] "), Span::raw("abcdefgh")]),
            5,
        );
        let texts: Vec<String> = rows.iter().map(ToString::to_string).collect();
        assert_eq!(texts, ["[web]", " abcd", "efgh"]);
        // Wide characters are not split across the edge.
        let rows = wrapped(TextLine::raw("ab好c"), 3);
        let texts: Vec<String> = rows.iter().map(ToString::to_string).collect();
        assert_eq!(texts, ["ab", "好c"]);
    }

    #[test]
    fn a_narrow_terminal_still_draws() {
        let mut app = running_app();
        app.size = (40, 12);
        snapshot("narrow", &app);
        // And one too small for anything does not panic.
        app.size = (3, 2);
        screen(&app);
        app.size = (0, 0);
        screen(&app);
    }

    #[test]
    fn clicks_land_where_things_are_drawn() {
        let mut app = running_app();
        let text = screen(&app);
        let rows: Vec<&str> = text.lines().collect();
        let find = |needle: &str| -> (u16, u16) {
            for (y, row) in rows.iter().enumerate() {
                if let Some(x) = row.find(needle) {
                    return (row[..x].chars().count() as u16, y as u16);
                }
            }
            panic!("`{needle}` is not on the screen:\n{text}");
        };
        let (x, y) = find("2 Build");
        assert_eq!(hit(&app, x, y), Some(Target::Tab(Tab::Build)));
        let (x, y) = find("r reload");
        assert_eq!(hit(&app, x, y), Some(Target::Button(Input::Char('r'))));
        let (x, y) = find("Browser");
        assert_eq!(hit(&app, x, y), Some(Target::DeviceRow(1)));
        assert_eq!(hit(&app, 1, y), Some(Target::Mark(1)));
        let (x, y) = find("Pixel 7");
        assert_eq!(hit(&app, x, y), Some(Target::DeviceRow(3)));
        let (x, y) = find("up, hot reload");
        assert_eq!(hit(&app, x, y), Some(Target::RunRow(0)));
        let (x, y) = find("● This computer");
        assert_eq!(hit(&app, x, y), Some(Target::RunRow(0)));
        let (x, y) = find("count: 1");
        assert_eq!(hit(&app, x, y), Some(Target::Pane));

        // A click does what the key does.
        let (x, y) = find("R restart  s");
        assert_eq!(
            app.update(Msg::Input(Input::Click(x, y))),
            [crate::state::Effect::Send {
                key: 1,
                command: crate::state::RunCommand::Restart
            }]
        );
        let (x, y) = find("Browser");
        app.update(Msg::Input(Input::Click(1, y)));
        assert!(app.marked.contains("web"));
        let _ = x;
        let (x, y) = find("3 Doctor");
        assert_eq!(
            app.update(Msg::Input(Input::Click(x, y))),
            [crate::state::Effect::Doctor]
        );
        assert_eq!(app.tab, Tab::Doctor);
    }
}
