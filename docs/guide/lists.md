# Lists and grids

A list of a hundred thousand rows costs what the rows **on screen** cost: `ListView` builds
only those, as they scroll in. Give it how many rows there are, how tall each one is, and a
function that builds row `i`.

## A long list

```rust
use frus::{component, Icon, Icons, ListTile, ListView, Widget};

fn invoices() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let selected = cx.use_state(|| None::<usize>);
        let current = selected.get();
        let select = selected.clone();
        Box::new(ListView::new(10_000, 56.0, move |i| {
            let select = select.clone();
            ListTile::new()
                .leading(Icon::new(Icons::RECEIPT))
                .title(format!("Invoice #{}", i + 1))
                .subtitle(format!("{:.2} €", (i as f32 * 7.31) % 500.0))
                .selected(current == Some(i))
                .on_tap(move || select.set(Some(i)))
        }))
    }))
}
```

A list takes the room it is given: the whole page, or what a `Column` leaves it when it is
wrapped in `Expanded`. `.axis(Axis::Horizontal)` makes it run across, `.reverse()` start at the
end (a chat), `.padding(16.0)` keep its rows off the edges.

`ListTile` is the usual row: `leading`, `title`, `subtitle`, `trailing`, `on_tap`. Any widget
can be a row, as long as it is as tall as the list says. `CheckboxListTile`,
`SwitchListTile` and `RadioListTile` are a tile and a control in one.

### The data behind it

The rows usually come from a `Vec` the state holds. The builder is called with an index and
reads the item it needs:

```rust
use std::rc::Rc;

use frus::{component, ListTile, ListView, Widget};

#[derive(Clone)]
struct Contact {
    name: String,
    phone: String,
}

fn contacts(people: Vec<Contact>) -> Box<dyn Widget> {
    let people = Rc::new(people);
    Box::new(component(move |_cx| {
        let rows = people.clone();
        Box::new(ListView::new(people.len(), 64.0, move |i| {
            ListTile::new().title(rows[i].name.clone()).subtitle(rows[i].phone.clone())
        }))
    }))
}
```

## Rows of different heights

When the rows are not all the same height — posts of different lengths, messages with and
without a picture — `ListView::builder(count, build)` lays each one out at its own height. It
still builds only the rows in view: a row is measured the first time it scrolls in, and the
rows not seen yet are counted at the mean of those that were.

```rust
use std::rc::Rc;

use frus::{component, Column, ListView, Text, Widget};

struct Post {
    author: String,
    body: String,
}

fn feed(posts: Vec<Post>) -> Box<dyn Widget> {
    let posts = Rc::new(posts);
    Box::new(component(move |_cx| {
        let shown = posts.clone();
        Box::new(
            ListView::builder(posts.len(), move |i| {
                Column::new()
                    .spacing(4.0)
                    .child(Text::new(shown[i].author.clone()).size(13.0))
                    .child(Text::new(shown[i].body.clone()).wrap())
            })
            .estimated_extent(120.0)
            .padding(16.0),
        )
    }))
}
```

`.estimated_extent(..)` is the height to count a row at before any is measured; a guess close
to the real rows keeps the scrollbar steady. Each row that can move — be removed, reordered,
inserted above — is given a key with `keyed(id, row)`, so that its state stays with it rather
than with its position.

## Pull to refresh, swipe to remove

`RefreshIndicator` wraps a list and calls you when it is pulled down; it spins for as long as
you say it is refreshing:

```rust
use std::time::Duration;

use frus::{component, host, ListTile, ListView, RefreshIndicator, Widget};

fn headlines() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let refreshing = cx.use_state(|| false);
        let fetched = cx.use_state(|| 1u32);
        let (start, done, count) = (refreshing.clone(), refreshing.clone(), fetched.clone());
        let on_refresh = move || {
            start.set(true);
            let (done, count) = (done.clone(), count.clone());
            // Stands for the request that fetches the new rows.
            host::after(Duration::from_millis(1500), move || {
                count.update(|n| *n += 1);
                done.set(false);
            });
        };
        let shown = fetched.get();
        let list = ListView::new(20, 48.0, move |i| {
            ListTile::new().title(format!("Headline {} (fetch #{shown})", i + 1))
        });
        Box::new(
            RefreshIndicator::new(list)
                .on_refresh(on_refresh)
                .refreshing(refreshing.get()),
        )
    }))
}
```

`Dismissible` lets a row be swiped away, with what to show underneath while it moves and what
to do once it is gone:

```rust
use frus::{component, keyed, Align, Colors, Column, Container, Dismissible, ListTile, Text, Widget};

fn inbox() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let mail = cx.use_state(|| vec!["Lunch on Friday?", "Your invoice", "Weekly report"]);
        let rows = mail.get().into_iter().fold(
            Column::new().cross_axis_alignment(Align::Stretch),
            |column, subject| {
                let remove = mail.clone();
                let row = Dismissible::new(ListTile::new().title(subject))
                    .height(56.0)
                    .background(Container::new().color(Colors::RED).child(Text::new("Delete")))
                    .on_dismiss_to_start(move || remove.update(|m| m.retain(|s| *s != subject)));
                column.child(keyed(subject, row))
            },
        );
        Box::new(rows)
    }))
}
```

## Grids

`GridView::new(columns)` lays out the cells it is given; `GridView::extent(max, count, build)`
fits as many columns as it can of tiles no wider than `max`; `GridView::builder(columns, count,
build)` is the virtualised one, for thousands of tiles.

```rust
use frus::{Center, Color, Container, GridView, Text, Widget};

fn tile(i: usize) -> Container {
    Container::new()
        .radius(6.0)
        .color(Color::hex("#6366F1"))
        .child(Center::new(Text::new(format!("{}", i + 1))))
}

fn photos() -> Box<dyn Widget> {
    Box::new(GridView::extent(120.0, 60, tile).gap(6.0).aspect(1.0))
}

fn catalogue() -> Box<dyn Widget> {
    Box::new(GridView::builder(3, 5_000, tile).gap(4.0).tile_height(120.0))
}
```

## A collapsing app bar over a grid and a list

`CustomScrollView` scrolls several pieces, called slivers, as one: an app bar that collapses,
a grid, a list. The lists and grids in it are lazy: only the rows in view, and 250 pixels
around it (`.cache_extent(..)`), are built.

```rust
use frus::{
    Container, CustomScrollView, FlexibleSpaceBar, ListTile, SliverAppBar, SliverGrid,
    SliverList, SliverPadding, Text, Widget,
};

fn places() -> Box<dyn Widget> {
    Box::new(
        CustomScrollView::new()
            .sliver(
                SliverAppBar::new()
                    .pinned(true)
                    .expanded_height(220.0)
                    .flexible_space(FlexibleSpaceBar::new().title(Text::new("Lisbon"))),
            )
            .sliver(
                SliverPadding::new(SliverGrid::extent(160.0, 24, |_| Container::new()).gap(8.0))
                    .padding(12.0),
            )
            .sliver(SliverList::new(10_000, 56.0, |i| {
                ListTile::new().title(format!("Street {i}"))
            })),
    )
}
```

- `SliverAppBar`: `.pinned(true)` keeps the toolbar once collapsed; `.floating(true)` brings it
  back as soon as the reader scrolls up, and `.snap(true)` finishes that once the scroll stops.
  `FlexibleSpaceBar` gives the expanded bar a title that shrinks into the toolbar and a
  background that fades out. The colours come from the app bar theme (`background`,
  `scrolled_under_background`, `scrolled_under_elevation`), and every one has a builder method.
- `SliverList::new(count, extent, build)`, `SliverList::builder(count, build)` for rows of any
  height, `SliverList::children(..)` for a few given widgets.
- `SliverGrid::count(columns, count, build)` and `SliverGrid::extent(max_width, count, build)`.
- `SliverToBoxAdapter::new(widget)` for a single widget, `SliverPadding` for room around a
  piece, `SliverFillRemaining` for what is left of the viewport, and `SliverPersistentHeader`
  for a header of your own that collapses between two extents.

## Moving a list from code

A list wrapped in `keyed(key, list)` can be scrolled from anywhere with `host::scroll_to`: to
its start, its end, or an offset. It animates unless told `.instant()`.

```rust
use frus::{host, keyed, Button, Column, Expanded, ListTile, ListView, ScrollTo, Widget};

const MESSAGES: &str = "messages";

fn conversation() -> Box<dyn Widget> {
    Box::new(
        Column::new()
            .child(Expanded::new(keyed(
                MESSAGES,
                ListView::new(500, 48.0, |i| ListTile::new().title(format!("Message {i}"))),
            )))
            .child(
                Button::new("Latest").on_press(|| host::scroll_to(MESSAGES, ScrollTo::end())),
            ),
    )
}
```

`ScrollTo::index(n)` brings row `n` to the top of the window — the message a notification
pointed at, the selected row of a long list — and `.aligned(0.5)` to its middle. It works in a
list of rows of any height too: the rows on the way are measured as the list travels, and the
row lands where it was asked for.

```rust
use frus::{host, keyed, Button, Column, Expanded, ListTile, ListView, ScrollTo, Widget};

fn threads() -> Box<dyn Widget> {
    Box::new(
        Column::new()
            .child(Button::new("Go to the unread one").on_press(|| {
                host::scroll_to("threads", ScrollTo::index(240).aligned(0.5))
            }))
            .child(Expanded::new(keyed(
                "threads",
                ListView::new(1000, 56.0, |i| ListTile::new().title(format!("Thread {i}"))),
            ))),
    )
}
```

`.on_scroll(|position| ..)` tells you where the list is — to load the next page when the reader
nears the end, for instance: `position.offset.1 > position.max.1 - 400.0`.

## Actions behind a row

`SwipeActions` is a row that slides aside to show what can be done with it, and stays: it
rests open on its actions, springs closed short of half of them, and a swipe all the way
performs the first action of that side. Rows of the same `.group` are open one at a time.
Leading actions are on the reading start side, so a right-to-left interface mirrors them.

```rust
use frus::{component, keyed, Align, Column, Icons, ListTile, SwipeAction, SwipeActions, Widget};

fn mailbox() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let mail = cx.use_state(|| vec!["Lunch on Friday?", "Your invoice", "Weekly report"]);
        let rows = mail.get().into_iter().fold(
            Column::new().cross_axis_alignment(Align::Stretch),
            |column, subject| {
                let (archive, delete) = (mail.clone(), mail.clone());
                let row = SwipeActions::new(ListTile::new().title(subject))
                    .trailing(
                        SwipeAction::new("Delete", move || {
                            delete.update(|m| m.retain(|s| *s != subject))
                        })
                        .icon(Icons::DELETE),
                    )
                    .leading(
                        SwipeAction::new("Archive", move || {
                            archive.update(|m| m.retain(|s| *s != subject))
                        })
                        .icon(Icons::ARCHIVE),
                    )
                    .group(1)
                    .height(56.0);
                column.child(keyed(subject, row))
            },
        );
        Box::new(rows)
    }))
}
```

The actions are not only for fingers: a focused row opens with the arrow keys and closes with
Escape, and a screen reader offers every action by name in its actions menu.

## Picking from a wheel

`WheelPicker` is one or more spinning wheels behind one band, the way a time or a date is picked
on a phone. A flick spins a wheel through many values and settles exactly on one; a slow
release goes back to the nearest. A `.looping(true)` column goes round, and the phone ticks under
the finger as each value passes the middle.

```rust
use frus::{component, WheelColumn, WheelPicker, Widget};

fn alarm() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let hour = cx.use_state(|| 7usize);
        let minute = cx.use_state(|| 30usize);
        let (h, m) = (hour.clone(), minute.clone());
        Box::new(
            WheelPicker::new()
                .column(
                    WheelColumn::new(24, |i| format!("{i:02}"), move |i: usize| h.set(i))
                        .selected(hour.get())
                        .looping(true),
                )
                .column(
                    WheelColumn::new(60, |i| format!("{i:02}"), move |i: usize| m.set(i))
                        .selected(minute.get())
                        .looping(true),
                ),
        )
    }))
}
```

`WheelColumn::numbers(1950..=2050, year, ..)` is a column of numbers, and a date is three
columns side by side. A single wheel of any widgets is `ListWheel`.

## When there is nothing to list

`EmptyState` is what a list shows before it has anything: an icon or an illustration, a title,
a line saying why, and what to do about it.

```rust
use frus::{button, EmptyState, Icons, Widget};

fn no_projects() -> Box<dyn Widget> {
    Box::new(
        EmptyState::new("No projects yet")
            .icon(Icons::FOLDER_OPEN)
            .description("Projects you create show up here.")
            .action(button("New project", || {})),
    )
}
```

## A conversation

`ChatList` shows a conversation: it rests at the newest message and follows new ones as they
arrive, unless the reader has scrolled up to read, in which case it keeps their place and offers
a button back down that says how many came in. Messages are grouped by sender and time, with
dates between days, the status of what was sent (sending, sent, delivered, read, failed with a
retry), and who is typing. `ChatComposer` is where a message is written: it grows to six lines
and sends only something.

```rust
use frus::{
    Column, ChatComposer, ChatList, ChatListState, ChatMessage, Expanded, MessageStatus, State,
    StateContext, StatefulWidget, Widget,
};
use std::rc::Rc;

struct Conversation;
struct ConversationState {
    messages: Rc<Vec<ChatMessage>>,
    list: ChatListState,
    draft: String,
}

impl StatefulWidget for Conversation {
    type State = ConversationState;
    fn create_state(&self) -> ConversationState {
        ConversationState {
            messages: Rc::new(vec![
                ChatMessage::incoming("1", "ada", "Did the build pass?", 1_700_000_000),
                ChatMessage::outgoing("2", "It did.", 1_700_000_060).status(MessageStatus::Read),
            ]),
            list: ChatListState::new(),
            draft: String::new(),
        }
    }
}

impl ConversationState {
    fn send(&mut self) {
        let id = (self.messages.len() + 1).to_string();
        let text = std::mem::take(&mut self.draft);
        Rc::make_mut(&mut self.messages)
            .push(ChatMessage::outgoing(id, text, 1_700_000_120).status(MessageStatus::Sending));
    }
}

impl State for ConversationState {
    type Widget = Conversation;
    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let list = ChatList::new(
            self.messages.clone(),
            &self.list,
            cx.handler(|s: &mut Self, next: ChatListState| s.list = next),
        )
        .typing(["Ada"]);
        let composer = ChatComposer::new(&self.draft, cx.handler(|s: &mut Self, text: String| s.draft = text))
            .on_send(cx.callback(ConversationState::send));
        Box::new(Column::new().child(Expanded::new(list)).child(composer))
    }
}
# let _ = Conversation;
```

Only the rows in view are built, however long the conversation. `MessageBubble` and
`TypingIndicator` are the parts, for a layout of your own; `theme.widgets.chat` styles them all
and `ChatStrings` replaces their words.

## A tree of files

`FileTree` shows folders and files: a chevron opens a folder, a row selects, Right and Left
open and close from the keyboard, and a folder whose children are not loaded yet shows a loading
row until the application gives them.

```rust
use frus::{FileNode, FileTree};

#[derive(Clone)]
enum Msg {
    Toggle(u64, bool),
    Select(u64),
}

let tree: FileTree<Msg> = FileTree::new(vec![
    FileNode::folder(1, "src", [FileNode::file(2, "main.rs"), FileNode::lazy_folder(3, "widgets")]),
    FileNode::file(4, "Cargo.toml"),
])
.expanded([1])
.selected(Some(2))
.on_toggle(Msg::Toggle)
.on_select(Msg::Select);
# let _ = tree;
```
