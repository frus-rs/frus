# Screens, navigation and dialogs

An application of more than one screen is a **router**: each screen is a route, named by a
path, and the router keeps the stack of screens the person has opened. The back button, the
back gesture, the page transitions and, on the web, the browser's address bar all follow it.

## Routes

```rust
use frus::{
    component, BuildContext, Button, Column, FrusApp, GoRoute, GoRouter, Text, Widget,
};

fn home(cx: &BuildContext) -> Box<dyn Widget> {
    let router = cx.router();
    let settings = router.clone();
    Box::new(
        Column::new()
            .spacing(12.0)
            .child(Text::new("Home"))
            .child(Button::new("Open user 42").on_press(move || router.go("/users/42")))
            .child(Button::new("Settings").on_press(move || settings.push("/settings"))),
    )
}

fn user(id: String) -> Box<dyn Widget> {
    Box::new(Text::new(format!("User {id}")))
}

fn app() -> FrusApp {
    let router = GoRouter::new(vec![
        GoRoute::new("/", |_, _| component(home)).routes(vec![
            GoRoute::new("users/:id", |_, state| {
                user(state.param("id").unwrap_or_default().to_string())
            })
            .name("user"),
        ]),
        GoRoute::new("/settings", |_, _| Text::new("Settings")),
    ]);
    FrusApp::router(router).title("People")
}
```

`frus::main!(app())` starts it. From any screen, `cx.router()` gives the router:

| Call | What it does |
| --- | --- |
| `go("/users/42")` | Shows that location, with the routes it is made of underneath (here, home) so there is a page to go back to. |
| `push("/settings")` | Puts a screen on top of the current one. |
| `pop()` | Goes back one screen. |
| `replace("/login")` | Swaps the top screen for another. |
| `go_named("user", &[("id", "42")], &[])` | Goes by the route's name, so the paths can change without breaking the callers. |

A screen reads what its location said from the `GoRouterState` its builder receives:
`param("id")` for `:id`, `query("tab")` for `?tab=`, `extra::<T>()` for a value passed along
with the navigation.

## Keeping someone out: redirects

A redirect is asked before every navigation and can send it elsewhere — to the sign-in screen
while nobody is signed in:

```rust
use std::cell::Cell;
use std::rc::Rc;

use frus::{GoRoute, GoRouter, Text};

fn guarded(signed_in: Rc<Cell<bool>>) -> GoRouter {
    GoRouter::new(vec![
        GoRoute::new("/", |_, _| Text::new("Your account")),
        GoRoute::new("/login", |_, _| Text::new("Sign in")),
    ])
    .redirect(move |state| {
        let to_login = !signed_in.get() && state.location() != "/login";
        to_login.then(|| "/login".to_string())
    })
}
```

`.refresh_listenable(&auth)` asks the redirect again whenever `auth` changes — so that signing
out sends the person back to the sign-in screen at once.

## Tabs that keep their pages

A bottom bar whose tabs each keep **their own stack of screens** is a *shell* route: a frame
around branches. Switching tabs keeps what each one had open, and what its screens hold.

```rust
use frus::{
    text, BottomNavigationBar, BottomNavigationBarItem, GoRoute, GoRouter, Icons, Scaffold,
    ShellBranch,
};

fn tabs() -> GoRouter {
    GoRouter::new(vec![
        GoRoute::shell(
            |_, shell, body| {
                let tabs = shell.clone();
                Scaffold::new().body(body).bottom_navigation_bar(
                    BottomNavigationBar::new(shell.current_index(), move |i| tabs.go_branch(i))
                        .items(vec![
                            BottomNavigationBarItem::new(Icons::HOME, "Feed"),
                            BottomNavigationBarItem::new(Icons::SEARCH, "Search"),
                        ]),
                )
            },
            vec![
                ShellBranch::new(vec![GoRoute::new("/feed", |_, _| text("Feed"))
                    .routes(vec![GoRoute::new("post/:id", |_, _| text("A post"))])]),
                ShellBranch::new(vec![GoRoute::new("/search", |_, _| text("Search"))]),
            ],
        ),
        // Outside the shell: a screen that covers the bar too.
        GoRoute::new("/settings", |_, _| text("Settings")),
    ])
    .initial_location("/feed")
}
```

- `push("/feed/post/7")` from the feed opens the post **under the bar**; the bar stays.
  `push("/settings")` covers the whole window.
- `go_branch(i)` shows tab `i` as it was left. Choosing the tab already on show goes back to
  its first screen.
- **Back** pops the tab's own screens first. On the first screen of another tab it shows the
  first tab, and from there it leaves the application. `.shell_back(ShellBack::Leave)` on the
  shell route leaves from the first screen of any tab instead. The edge swipe only ever pops a
  screen; it never switches tabs.
- A location — a link, the browser's address — picks the tab and builds its stack:
  `/feed/post/7` opens the feed tab with the post on top. On the web, the address follows the
  tab on show, and the browser's Back returns to where the previous tab was left.
- A branch whose first route has a parameter needs a start:
  `ShellBranch::new(..).initial_location("/users/me")`.

## A screen that answers

A screen pushed to *ask* something — a colour, a contact, a confirmation — hands its answer back
when it pops:

```rust
use frus::{BuildContext, Button, Widget};

fn choose(cx: &BuildContext) -> Box<dyn Widget> {
    let router = cx.router();
    let ask = cx.task_scope().async_callback(move || {
        let router = router.clone();
        async move {
            let picked: Option<String> = router.push_for_result("/pick").await;
            frus::log::info!("picked {picked:?}");
        }
    });
    Box::new(Button::new("Pick a colour").on_press(ask))
}

// On the `/pick` screen:
// router.pop_with(String::from("blue"));
```

The answer is `None` when the screen went any other way: Back, the edge swipe, the app bar's
arrow, a `go` elsewhere. Without `async`, `push_then("/pick", |picked: Option<String>| ..)`
takes a closure instead.

## Watching the screens

`router.observe(|change| ..)` is told of every move — a push, a pop, a swap, a `go`, a tab
switch — with where it came from and where it went: what an analytics or a log of screens
needs. Keep the `Subscription` it returns for as long as you listen, or set it up for the
router's whole life with `.observer(..)` when you make the router (that one is also told of the
first screen).

```rust
use frus::{GoRoute, GoRouter, Text};

fn logged() -> GoRouter {
    GoRouter::new(vec![GoRoute::new("/", |_, _| Text::new("Home"))])
        .observer(|change| frus::log::info!("{:?}: {}", change.kind, change.to))
}
```

## The shape of a screen: `Scaffold`

`Scaffold` puts the usual parts of a screen where people expect them: an app bar at the top,
the body, a floating action button, a navigation bar or a drawer. It also keeps the body clear
of the status bar, the notch and the keyboard.

```rust
use frus::{
    component, AppBar, Center, FloatingActionButton, IconButton, Icons, Scaffold, Text, Widget,
};

fn notes() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let count = cx.use_state(|| 0u32);
        let tab = cx.use_state(|| 0usize);
        let (add, pick) = (count.clone(), tab.clone());
        Box::new(
            Scaffold::new()
                .app_bar(
                    AppBar::new()
                        .title(Text::new("Notes"))
                        .action(IconButton::new(Icons::SEARCH).on_press(|| {})),
                )
                .body(Center::new(Text::new(format!("{} notes", count.get()))))
                .fab(FloatingActionButton::new(Icons::ADD).on_press(move || add.update(|n| *n += 1)))
                .nav(tab.get(), move |i: usize| pick.set(i))
                .destination(Icons::NOTE, "Notes")
                .destination(Icons::ALARM, "Reminders"),
        )
    }))
}
```

`.nav` with `.destination`s builds the current design's navigation bar. For the classic one —
every item the same width, or *shifting*, where the selected item widens and the bar takes its
colour — hand the scaffold a `BottomNavigationBar`:

```rust
use frus::{
    component, BottomNavigationBar, BottomNavigationBarItem, Icons, Scaffold, Text, Widget,
};

fn library() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let tab = cx.use_state(|| 0usize);
        let pick = tab.clone();
        Box::new(
            Scaffold::new()
                .body(Text::new(["Albums", "Artists", "Search"][tab.get()]))
                .bottom_navigation_bar(
                    BottomNavigationBar::new(tab.get(), move |i| pick.set(i))
                        .item(BottomNavigationBarItem::new(Icons::ALBUM, "Albums"))
                        .item(BottomNavigationBarItem::new(Icons::PERSON, "Artists"))
                        .item(BottomNavigationBarItem::new(Icons::SEARCH, "Search")),
                ),
        )
    }))
}
```

Three items or fewer make a fixed bar and more a shifting one, unless `.kind` says otherwise;
`theme.widgets.bottom_navigation_bar` sets the defaults for every bar of an application.

A screen the router pushed over another gets the way back on its own: its app bar shows a
back arrow (a chevron on Apple's systems) that pops the router, unless the screen has a drawer
or gives the bar a `leading` of its own. The system's back button and the back gesture pop it
too.

## A message at the bottom: snack bars

`host::show_snack_bar` shows a short message at the bottom of the window, over whatever screen
is showing, for four seconds. Several wait their turn. An action runs and hides the bar:

```rust
use frus::{host, Button, SnackBar, Widget};

fn archive() -> Box<dyn Widget> {
    Box::new(Button::new("Archive").on_press(|| {
        // … archive the message …
        host::show_snack_bar(SnackBar::new("Archived").action("Undo", (|| {
            // … put it back …
        }).into()));
    }))
}
```

`host::show_snack_bar_for(bar, duration)` chooses how long; `host::hide_current_snack_bar()`
hides it from code, and `.close_icon(host::hide_current_snack_bar)` gives it a cross.

## Dialogs and sheets

A dialog is part of the screen's state: it is open while a field of the state says so. The
dialog's buttons change that field, and the screen it covers is its `body`:

```rust
use frus::{
    AlertDialog, Button, State, StateContext, StatefulWidget, Text, Variant, Widget,
};

struct Draft;

#[derive(Default)]
struct DraftState {
    asking: bool,
    discarded: bool,
}

impl StatefulWidget for Draft {
    type State = DraftState;
    fn create_state(&self) -> DraftState {
        DraftState::default()
    }
}

impl State for DraftState {
    type Widget = Draft;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let screen = Button::new(if self.discarded { "Discarded" } else { "Discard draft" })
            .on_press(cx.callback(|s: &mut Self| s.asking = true));
        Box::new(
            AlertDialog::new(self.asking)
                .title("Discard draft?")
                .content("Your message has not been sent.")
                .action(
                    Button::new("Keep editing")
                        .variant(Variant::Text)
                        .on_press(cx.callback(|s: &mut Self| s.asking = false)),
                )
                .action(Button::new("Discard").variant(Variant::Text).on_press(cx.callback(
                    |s: &mut Self| {
                        s.discarded = true;
                        s.asking = false;
                    },
                )))
                .on_dismiss(cx.callback(|s: &mut Self| s.asking = false))
                .body(screen),
        )
    }
}

```

Because it is state, a dialog survives a rotation, can be opened from a test by setting the
field, and is never left open by a screen that has gone. `BottomSheet::new(open)` is the same
shape for a sheet that rises from the bottom (`.sheet(content)`, `.body(screen)`,
`.on_dismiss(..)`), and so are `SimpleDialog`, `Dialog`, `MenuAnchor` and `PopupMenuButton`.

## Menus, cards and panels over the screen

Four more pieces of the screen keep their state the same way: a field of the state, and a
message that hands back the new value.

- **`CommandPalette`** wraps the screen and finds a command by typing: a fuzzy match, the
  matched letters marked, groups, the keys shown on the right, Up, Down, Enter and Escape. It
  opens on the key you give `open_on`. An application fetching results itself turns off
  `filter` and says `loading` while they come.
- **`ContextMenu`** wraps part of the screen and opens its menu where a right-click or a long
  press landed: `MenuItem`s (ticks, `MenuItem::radio` choices, keys, rules, unavailable rows) and
  `SubmenuButton`s, with the menu bar's keyboard.
- **`ResizablePanels`** puts panels side by side, or one above the other, with a handle
  between each two that the pointer, a finger or the arrow keys move. Each panel has a least
  and a largest share and may collapse. Save `sizes()` to bring the layout back.
- **`HoverCard`** needs no state at all: it opens when the pointer rests on its anchor, or
  after a long press, and closes when the pointer leaves.

```rust
use frus::{
    Command, CommandPalette, CommandPaletteState, ContextMenu, ContextMenuState, HoverCard,
    KeyStroke, MenuItem, ResizablePanel, ResizablePanels, ResizablePanelsState, ShortcutKey,
    State, StateContext, StatefulWidget, Text, Widget,
};

struct Editor;

struct EditorState {
    palette: CommandPaletteState,
    menu: ContextMenuState,
    panels: ResizablePanelsState,
    saved: bool,
}

impl StatefulWidget for Editor {
    type State = EditorState;
    fn create_state(&self) -> EditorState {
        EditorState {
            palette: CommandPaletteState::closed(),
            menu: ContextMenuState::closed(),
            panels: ResizablePanelsState::new([0.3, 0.7]),
            saved: false,
        }
    }
}

impl State for EditorState {
    type Widget = Editor;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let save = cx.callback(|s: &mut Self| {
            s.saved = true;
            // A command or a row is one message: close what sent it.
            s.palette = CommandPaletteState::closed();
            s.menu = ContextMenuState::closed();
        });
        let files = HoverCard::new(Text::new("notes.md")).content(Text::new("Edited today"));
        let page = ContextMenu::new(
            Text::new(if self.saved { "Saved" } else { "Unsaved" }),
            &self.menu,
            cx.handler(|s: &mut Self, menu: ContextMenuState| s.menu = menu),
        )
        .item(MenuItem::new("Save", save.clone()).shortcut("Ctrl+S"));
        let panels = ResizablePanels::horizontal(
            &self.panels,
            cx.handler(|s: &mut Self, panels: ResizablePanelsState| s.panels = panels),
        )
        .panel(ResizablePanel::new(files).min(0.2).collapsible(true))
        .panel(ResizablePanel::new(page));
        Box::new(
            CommandPalette::new(
                panels,
                &self.palette,
                cx.handler(|s: &mut Self, palette: CommandPaletteState| s.palette = palette),
            )
            .open_on(KeyStroke::new(ShortcutKey::Char('k')).ctrl())
            .command(Command::new("Save", save).group("File").shortcut("Ctrl+S")),
        )
    }
}
```

Each of them takes its look from its own builders first, then from the theme
(`theme.widgets.command_palette`, `context_menu`, `resizable_panels`, `hover_card`), and
mirrors itself in right-to-left text.

## Tabs and drawers

`TabBar` with a `TabBarView`, `NavigationBar`, `NavigationRail` and `NavigationDrawer` switch
between sections of one screen; the selected index is the state's, like any other control. For
tabs that each keep a stack of screens of their own, see
[Tabs that keep their pages](#tabs-that-keep-their-pages). The
widget gallery (`frus-examples/widgets`, under Navigation) has each one, and
`frus-examples/navigation` is a shop of eight screens built on the router: nested routes,
names, queries, a guard and a page that is not found.
