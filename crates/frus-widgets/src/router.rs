// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **A router**: locations, pages, and moving between them.
//!
//! An application with more than one screen names them. A [`GoRoute`] says which pattern of
//! path it answers to and what page it builds; a [`GoRouter`] holds the routes and the stack
//! of pages the person is looking at, and moves that stack when told a *location* — the
//! string a link, a deep link or a button would carry: `/users/42?tab=posts`.
//!
//! ```
//! use frus_widgets::{text, GoRoute, GoRouter};
//!
//! let router = GoRouter::new(vec![
//!     GoRoute::new("/", |_, _| text("home")).routes(vec![
//!         // relative to its parent: `/users/:id`
//!         GoRoute::new("users/:id", |_, state| {
//!             let id = state.param("id").unwrap_or("?").to_string();
//!             text(format!("user {id}"))
//!         })
//!         .name("user"),
//!     ]),
//!     GoRoute::new("/settings", |_, _| text("settings")),
//! ]);
//!
//! router.go("/users/42?tab=posts");
//! assert_eq!(router.location(), "/users/42?tab=posts");
//! assert!(router.can_pop(), "home is underneath");
//!
//! router.pop();
//! assert_eq!(router.location(), "/");
//! ```
//!
//! ## Two ways to move
//!
//! [`GoRouter::go`] says *where*: it makes the stack whatever the location's routes are — a
//! location that lives under `/` has `/` beneath it, so there is somewhere to go back to.
//! [`GoRouter::push`] says *on top of what is here*: it adds one page and leaves the rest,
//! which is how the same page can be open twice.
//!
//! ## Redirects
//!
//! A redirect looks at where a navigation is heading and may send it elsewhere — to a sign-in
//! page while nobody is signed in, away from it once someone is. It runs on every navigation,
//! and again when a [`refresh listenable`](GoRouter::refresh_listenable) changes; a redirect
//! that answers with another location is asked again about that one, up to a limit.
//!
//! ## Pages under the top
//!
//! A page covered by another is kept: what its components hold is still there when the top
//! page is popped. It is built, but not laid out or drawn, until it is on show again.
//!
//! ## Addresses from outside
//!
//! A location handed in from outside — [`GoRouter::open`], what a browser's Back and Forward
//! come through — that is a page already beneath the top goes back to **that page**, the one
//! that was pushed included, rather than rebuilding the stack from the location: it keeps
//! its state, and what was beneath it. A router that
//! [keeps popped pages](GoRouter::keep_popped_pages) also finds a page it went back from as
//! it was left, when the location comes back — a browser's Forward.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use frus_core::{AnimationController, Curve, SpringDescription};

use crate::component::{request_rebuild, BuildContext};
use crate::navigator::Navigator;
use crate::notifier::{Listenable, Subscription};
use crate::widget::Widget;

// ---------------------------------------------------------------------------------------
// Motion
// ---------------------------------------------------------------------------------------

/// Horizon (s) over which a finger's velocity is projected to decide between going back and
/// staying.
const BACK_PROJECT: f32 = 0.12;
/// Projected position (a fraction) beyond which going back is committed.
const BACK_COMMIT_POS: f32 = 0.5;
/// How fast a released iOS page must be moving, in screen widths per second, for its
/// direction to decide whether it goes (`route.dart:32`).
const CUPERTINO_FLING: f32 = 1.0;
/// How long a released iOS page takes to finish going, or to come back
/// (`route.dart:35`).
const CUPERTINO_SETTLE: f32 = 0.35;
/// Stiffness of the page transition's spring.
const SPRING_K: f32 = 220.0;
/// Its damping: close to critical, so a page arrives without overshooting.
const SPRING_C: f32 = 30.0;

fn spring() -> SpringDescription {
    SpringDescription::new(1.0, SPRING_K, SPRING_C)
}

// ---------------------------------------------------------------------------------------
// Locations
// ---------------------------------------------------------------------------------------

fn decode(text: &str, plus_is_space: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' if plus_is_space => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// A location taken apart: its path's segments, and its query.
struct Parts {
    segments: Vec<String>,
    query: Vec<(String, String)>,
}

fn split_location(location: &str) -> Parts {
    let without_fragment = location.split('#').next().unwrap_or("");
    let (path, query) = without_fragment
        .split_once('?')
        .unwrap_or((without_fragment, ""));
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect();
    let query = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key, true), decode(value, true))
        })
        .collect();
    Parts { segments, query }
}

// ---------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------

/// Where a page is: what location it was reached at, which route it is, and what the
/// location said — the values of the path's `:parameters`, and the query.
#[derive(Clone)]
pub struct GoRouterState {
    location: String,
    path: String,
    name: Option<String>,
    params: HashMap<String, String>,
    query: HashMap<String, String>,
    extra: Option<Rc<dyn Any>>,
    entering: f32,
}

impl GoRouterState {
    /// How far into view this page is: `1.0` once it has settled, and while a transition
    /// brings it in or takes it out, the number that transition is driven by — `0.0` not yet
    /// arrived. A page that wants to move its own contents as it arrives reads it, and one
    /// that does not ignores it.
    pub fn entering(&self) -> f32 {
        self.entering
    }

    /// The whole location, query included: `/users/42?tab=posts`.
    pub fn location(&self) -> &str {
        &self.location
    }

    /// The pattern of the route that answered: `/users/:id`. Empty when no route did.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The route's name, if it has one.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The value of the path parameter `name` — `42` for `:id` in `/users/42`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    /// Every path parameter of the location.
    pub fn params(&self) -> &HashMap<String, String> {
        &self.params
    }

    /// The value of the query parameter `name` — `posts` for `tab` in `?tab=posts`. When it
    /// is given more than once, the last one.
    pub fn query(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }

    /// Every query parameter of the location.
    pub fn query_params(&self) -> &HashMap<String, String> {
        &self.query
    }

    /// The object that came along with the navigation ([`GoRouter::go_with_extra`]), if it
    /// is a `T`.
    pub fn extra<T: 'static>(&self) -> Option<Rc<T>> {
        self.extra.clone().and_then(|extra| extra.downcast().ok())
    }
}

impl fmt::Debug for GoRouterState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoRouterState")
            .field("location", &self.location)
            .field("path", &self.path)
            .field("name", &self.name)
            .field("params", &self.params)
            .field("query", &self.query)
            .finish()
    }
}

// ---------------------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------------------

type RouteBuilder = Rc<dyn Fn(&BuildContext, &GoRouterState) -> Box<dyn Widget>>;
type Redirect = Rc<dyn Fn(&GoRouterState) -> Option<String>>;
type ShellBuilder = Rc<dyn Fn(&BuildContext, &NavigationShell, Box<dyn Widget>) -> Box<dyn Widget>>;

/// One place the router can go: a pattern of path, and the page it builds.
///
/// A path is made of segments: a literal (`users`), a parameter (`:id`), or a trailing `*`
/// that takes whatever is left. A top-level route's path starts with `/`; a sub-route's is
/// relative to its parent's and does not.
///
/// A route can also be a **shell** ([`GoRoute::shell`]): a frame around several branches —
/// the tabs of a bottom navigation bar — each with a stack of pages of its own.
pub struct GoRoute {
    path: String,
    name: Option<String>,
    builder: RouteBuilder,
    routes: Vec<GoRoute>,
    redirect: Option<Redirect>,
    shell: Option<ShellParts>,
}

struct ShellParts {
    builder: ShellBuilder,
    branches: Vec<ShellBranch>,
    back: ShellBack,
}

/// One branch of a [shell](GoRoute::shell): the routes of one tab, and where the tab opens
/// the first time it is shown.
///
/// ```
/// use frus_widgets::{text, GoRoute, ShellBranch};
///
/// let search = ShellBranch::new(vec![GoRoute::new("/search", |_, _| text("search"))])
///     .initial_location("/search?q=rust");
/// # let _ = search;
/// ```
pub struct ShellBranch {
    routes: Vec<GoRoute>,
    initial: Option<String>,
}

impl ShellBranch {
    /// A branch made of `routes`. It opens at its first route's path unless told otherwise
    /// ([`initial_location`](Self::initial_location)).
    pub fn new(routes: Vec<GoRoute>) -> Self {
        Self {
            routes,
            initial: None,
        }
    }

    /// Where the branch opens the first time it is shown: needed when its first route's path
    /// has a `:parameter` or a `*`.
    pub fn initial_location(mut self, location: impl Into<String>) -> Self {
        self.initial = Some(location.into());
        self
    }
}

/// What the system's Back does on the first page of a [shell](GoRoute::shell)'s branch,
/// when that branch is not the first.
///
/// ```
/// use frus_widgets::{text, GoRoute, ShellBack, ShellBranch};
///
/// let tabs = GoRoute::shell(|_, _, body| body, vec![
///     ShellBranch::new(vec![GoRoute::new("/a", |_, _| text("a"))]),
///     ShellBranch::new(vec![GoRoute::new("/b", |_, _| text("b"))]),
/// ])
/// .shell_back(ShellBack::Leave);
/// # let _ = tabs;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShellBack {
    /// Back shows the first branch, as it was left; Back there leaves the shell — the
    /// application, when the shell is its first page. The default, as most applications
    /// with a bottom bar do.
    #[default]
    FirstBranch,
    /// Back leaves the shell from the first page of any branch.
    Leave,
}

impl GoRoute {
    /// A route answering to `path`, building its page with `builder`.
    ///
    /// `builder` is told the [`GoRouterState`] — where the location put the person — and may
    /// return any widget: a [`Component`](crate::Component), a `Box<dyn Widget>`, a
    /// scaffold.
    pub fn new<W: Widget + 'static>(
        path: impl Into<String>,
        builder: impl Fn(&BuildContext, &GoRouterState) -> W + 'static,
    ) -> Self {
        Self {
            path: path.into(),
            name: None,
            builder: Rc::new(move |cx, state| Box::new(builder(cx, state))),
            routes: Vec::new(),
            redirect: None,
            shell: None,
        }
    }

    /// A **shell**: a frame — a scaffold with a bottom navigation bar, a rail — around
    /// `branches`, each of which keeps **its own stack of pages** while another is shown.
    ///
    /// `builder` is handed the [`NavigationShell`] (which branch is shown, and how to show
    /// another) and the body: the pages of the branch on show, which the frame puts where
    /// they go. A page pushed from inside a branch is pushed onto that branch, under the
    /// frame; a page of a route outside the shell covers the whole frame.
    ///
    /// The routes of a branch are written as anywhere else: absolute paths at the top level,
    /// relative ones under a parent route. A location one of them answers to — a deep link, a
    /// browser's address — shows that branch with the stack the location is made of.
    ///
    /// The system's Back pops the branch's own pages first; on a branch's first page it
    /// follows [`shell_back`](Self::shell_back).
    ///
    /// ```
    /// use frus_widgets::{
    ///     text, BottomNavigationBar, BottomNavigationBarItem, GoRoute, GoRouter, Icons,
    ///     Scaffold, ShellBranch,
    /// };
    ///
    /// let router = GoRouter::new(vec![GoRoute::shell(
    ///     |_, shell, body| {
    ///         let tabs = shell.clone();
    ///         Scaffold::new().body(body).bottom_navigation_bar(
    ///             BottomNavigationBar::new(shell.current_index(), move |i| tabs.go_branch(i))
    ///                 .items(vec![
    ///                     BottomNavigationBarItem::new(Icons::HOME, "Home"),
    ///                     BottomNavigationBarItem::new(Icons::SETTINGS, "Settings"),
    ///                 ]),
    ///         )
    ///     },
    ///     vec![
    ///         ShellBranch::new(vec![GoRoute::new("/home", |_, _| text("home"))
    ///             .routes(vec![GoRoute::new("details", |_, _| text("details"))])]),
    ///         ShellBranch::new(vec![GoRoute::new("/settings", |_, _| text("settings"))]),
    ///     ],
    /// )])
    /// .initial_location("/home");
    ///
    /// router.go("/home/details");
    /// router.go("/settings"); // the other tab
    /// router.go_branch(0); // back to the first, still on its details page
    /// assert_eq!(router.location(), "/home/details");
    /// ```
    pub fn shell<W: Widget + 'static>(
        builder: impl Fn(&BuildContext, &NavigationShell, Box<dyn Widget>) -> W + 'static,
        branches: Vec<ShellBranch>,
    ) -> Self {
        Self {
            path: String::new(),
            name: None,
            builder: Rc::new(|_, _| Box::new(crate::SizedBox::empty())),
            routes: Vec::new(),
            redirect: None,
            shell: Some(ShellParts {
                builder: Rc::new(move |cx, shell, body| Box::new(builder(cx, shell, body))),
                branches,
                back: ShellBack::default(),
            }),
        }
    }

    /// What the system's Back does on the first page of a branch that is not the first, for
    /// a [shell](Self::shell). [`ShellBack::FirstBranch`] by default.
    pub fn shell_back(mut self, back: ShellBack) -> Self {
        match self.shell.as_mut() {
            Some(shell) => shell.back = back,
            None => debug_assert!(
                false,
                "`shell_back` is for a route made with `GoRoute::shell`"
            ),
        }
        self
    }

    /// Names the route, so it can be reached without writing its path:
    /// [`GoRouter::go_named`].
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The routes under this one. A location that one of them answers to has this route's page
    /// beneath its own.
    pub fn routes(mut self, routes: Vec<GoRoute>) -> Self {
        self.routes = routes;
        self
    }

    /// Where to send a navigation heading to this route or below it, if not here — `None` to
    /// let it through.
    pub fn redirect(
        mut self,
        redirect: impl Fn(&GoRouterState) -> Option<String> + 'static,
    ) -> Self {
        self.redirect = Some(Rc::new(redirect));
        self
    }
}

enum Segment {
    Literal(String),
    Param(String),
    Splat,
}

struct Node {
    full_path: String,
    segments: Vec<Segment>,
    name: Option<String>,
    builder: RouteBuilder,
    redirect: Option<Redirect>,
    children: Vec<Rc<Node>>,
    /// The shell and branch this route is in, if it is in one.
    member: Option<(u64, usize)>,
    /// When this route is itself a shell: what it is.
    spec: Option<Rc<ShellSpec>>,
}

/// A shell, as the router keeps it.
struct ShellSpec {
    builder: ShellBuilder,
    /// Where each branch opens the first time.
    initial: Vec<String>,
    back: ShellBack,
}

/// What building the route tree collects on the way.
struct Building<'a> {
    names: &'a mut HashMap<String, Rc<Node>>,
    shells: &'a mut HashMap<u64, Rc<ShellSpec>>,
}

fn build_node(
    route: GoRoute,
    parent: Option<&str>,
    member: Option<(u64, usize)>,
    building: &mut Building,
) -> Rc<Node> {
    if let Some(parts) = route.shell {
        assert!(
            member.is_none(),
            "a shell inside a branch of another shell is not supported"
        );
        assert!(!parts.branches.is_empty(), "a shell needs a branch");
        let id = building.shells.len() as u64 + 1;
        let mut children = Vec::new();
        let mut initial = Vec::new();
        for (index, branch) in parts.branches.into_iter().enumerate() {
            assert!(!branch.routes.is_empty(), "a shell's branch needs a route");
            let first = children.len();
            for child in branch.routes {
                children.push(build_node(child, parent, Some((id, index)), building));
            }
            let start = branch.initial.unwrap_or_else(|| {
                let path: &str = &children[first].full_path;
                assert!(
                    !path.contains(':') && !path.contains('*'),
                    "the first route of a shell's branch, `{path}`, has a parameter: give the \
                     branch an `initial_location`"
                );
                path.to_string()
            });
            initial.push(start);
        }
        let spec = Rc::new(ShellSpec {
            builder: parts.builder,
            initial,
            back: parts.back,
        });
        building.shells.insert(id, spec.clone());
        return Rc::new(Node {
            full_path: parent.unwrap_or("").to_string(),
            segments: Vec::new(),
            name: None,
            builder: route.builder,
            redirect: route.redirect,
            children,
            member: None,
            spec: Some(spec),
        });
    }
    let full_path = match parent {
        None => {
            assert!(
                route.path.starts_with('/'),
                "a top-level route's path starts with `/`: `{}`",
                route.path
            );
            route.path.clone()
        }
        Some(parent) => {
            assert!(
                !route.path.starts_with('/'),
                "a sub-route's path is relative to its parent's, and does not start with `/`: `{}`",
                route.path
            );
            assert!(!route.path.is_empty(), "a sub-route needs a path");
            format!("{}/{}", parent.trim_end_matches('/'), route.path)
        }
    };
    let segments = route
        .path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| match segment {
            "*" => Segment::Splat,
            other => match other.strip_prefix(':') {
                Some(name) => Segment::Param(name.to_string()),
                None => Segment::Literal(other.to_string()),
            },
        })
        .collect();
    let children = route
        .routes
        .into_iter()
        .map(|child| build_node(child, Some(&full_path), member, building))
        .collect();
    let node = Rc::new(Node {
        full_path,
        segments,
        name: route.name.clone(),
        builder: route.builder,
        redirect: route.redirect,
        children,
        member,
        spec: None,
    });
    if let Some(name) = route.name {
        let clash = building.names.insert(name.clone(), node.clone());
        assert!(clash.is_none(), "two routes are named `{name}`");
    }
    node
}

/// How many segments of `input` the pattern takes, filling `params` — or `None` if it does
/// not match the start of it.
fn consume(
    segments: &[Segment],
    input: &[String],
    params: &mut HashMap<String, String>,
) -> Option<usize> {
    let mut at = 0;
    for segment in segments {
        match segment {
            Segment::Literal(literal) => {
                if input.get(at)? != literal {
                    return None;
                }
                at += 1;
            }
            Segment::Param(name) => {
                params.insert(name.clone(), decode(input.get(at)?, false));
                at += 1;
            }
            Segment::Splat => {
                params.insert("*".to_string(), input[at.min(input.len())..].join("/"));
                at = input.len();
            }
        }
    }
    Some(at)
}

/// The chain of routes that answers to `input`, the parent first — the deepest route whose
/// pattern takes all of it.
fn match_nodes(
    nodes: &[Rc<Node>],
    input: &[String],
    params: &mut HashMap<String, String>,
) -> Option<Vec<Rc<Node>>> {
    for node in nodes {
        let mut found = params.clone();
        if node.spec.is_some() {
            // A shell takes nothing of the location and is never a page of its own: it is
            // on the way to one of its branches' routes, or not at all.
            if let Some(mut below) = match_nodes(&node.children, input, &mut found) {
                *params = found;
                below.insert(0, node.clone());
                return Some(below);
            }
            continue;
        }
        let Some(used) = consume(&node.segments, input, &mut found) else {
            continue;
        };
        if used == input.len() {
            *params = found;
            return Some(vec![node.clone()]);
        }
        if let Some(mut below) = match_nodes(&node.children, &input[used..], &mut found) {
            *params = found;
            below.insert(0, node.clone());
            return Some(below);
        }
    }
    None
}

// ---------------------------------------------------------------------------------------
// The router
// ---------------------------------------------------------------------------------------

/// One page of the stack.
#[derive(Clone)]
struct Page {
    /// What identifies it: a page reached again by `go` keeps it, and so keeps its state; one
    /// that was pushed has its own.
    key: u64,
    node: Option<Rc<Node>>,
    state: Rc<GoRouterState>,
    /// Whether it was pushed rather than made by a `go`: what restoring the stack replays.
    pushed: bool,
    /// The shell and branch whose stack it is in, if it is shown inside a shell. The pages of
    /// one shell are always next to each other in the stack, and of one branch.
    shell: Option<(u64, usize)>,
    /// Who waits for what the page is popped with ([`GoRouter::push_for_result`]).
    result: Option<Rc<PopSlot>>,
}

/// Hands a popped page's result (or `None`) to whoever pushed it.
type Deliver = Box<dyn FnOnce(Option<Box<dyn Any>>)>;

/// Builds a page drawn at a slide's progress.
type MakeAt<'a> = Box<dyn Fn(f32) -> Box<dyn Widget> + 'a>;

/// What a page pushed for a result hands back when it leaves the stack.
struct PopSlot {
    value: RefCell<Option<Box<dyn Any>>>,
    deliver: RefCell<Option<Deliver>>,
}

impl PopSlot {
    fn new(deliver: impl FnOnce(Option<Box<dyn Any>>) + 'static) -> Rc<Self> {
        Rc::new(Self {
            value: RefCell::new(None),
            deliver: RefCell::new(Some(Box::new(deliver))),
        })
    }

    /// The page has gone: whoever waits is told, once.
    fn finish(&self) {
        let deliver = self.deliver.borrow_mut().take();
        if let Some(deliver) = deliver {
            let value = self.value.borrow_mut().take();
            deliver(value);
        }
    }
}

/// The stacks of a shell's branches: the one on show is also the run of shell pages in the
/// router's stack.
#[derive(Clone)]
struct ShellStacks {
    current: usize,
    branches: Vec<Vec<Page>>,
}

struct Transition {
    from: Page,
    /// The shell the page left was in, as it was then — what is drawn leaving when the whole
    /// shell goes.
    from_shell: Option<ShellStacks>,
    controller: AnimationController,
    forward: bool,
}

struct PopShared<T> {
    value: RefCell<Option<Option<T>>>,
    waker: RefCell<Option<std::task::Waker>>,
}

/// What a page pushed with [`GoRouter::push_for_result`] is popped with: a future, ready
/// once the page has left the stack, with the value handed to [`GoRouter::pop_with`] —
/// `None` if it went any other way.
#[must_use = "a result does nothing unless awaited"]
pub struct PopResult<T> {
    shared: Rc<PopShared<T>>,
}

impl<T> PopResult<T> {
    /// The answer, if the page has gone already: `Some(answer)`, without waiting.
    pub fn try_take(&self) -> Option<Option<T>> {
        self.shared.value.borrow_mut().take()
    }
}

impl<T> std::future::Future for PopResult<T> {
    type Output = Option<T>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<T>> {
        match self.shared.value.borrow_mut().take() {
            Some(value) => std::task::Poll::Ready(value),
            None => {
                *self.shared.waker.borrow_mut() = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    }
}

impl<T> fmt::Debug for PopResult<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PopResult")
            .field("ready", &self.shared.value.borrow().is_some())
            .finish()
    }
}

/// What a [shell](GoRoute::shell)'s frame is told: which branch is on show, how many there
/// are, and how to show another. Cheap to clone into a handler.
///
/// ```ignore
/// let tabs = shell.clone();
/// BottomNavigationBar::new(shell.current_index(), move |i| tabs.go_branch(i))
/// ```
#[derive(Clone)]
pub struct NavigationShell {
    router: GoRouter,
    shell: u64,
    current: usize,
    count: usize,
}

impl NavigationShell {
    /// The branch on show.
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// How many branches the shell has.
    pub fn branch_count(&self) -> usize {
        self.count
    }

    /// Shows branch `index` **as it was left** — its pages, and what they hold — or from
    /// where it starts, the first time. The branch already on show goes back to its first
    /// page, as a tab chosen again does on phones.
    pub fn go_branch(&self, index: usize) {
        self.router.show_branch(self.shell, index);
    }

    /// The router.
    pub fn router(&self) -> GoRouter {
        self.router.clone()
    }
}

impl fmt::Debug for NavigationShell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NavigationShell")
            .field("current", &self.current)
            .field("count", &self.count)
            .finish()
    }
}

/// What the system's Back would do now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BackAction {
    /// Take the page on top off.
    Page,
    /// Show the first branch of this shell.
    FirstBranch(u64),
    /// Nothing: Back leaves the application.
    Nothing,
}

/// A move of the router, as [`GoRouter::observe`] reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RouteChangeKind {
    /// [`GoRouter::go`], an address from outside, or a redirect: the stack was rebuilt.
    Go,
    /// A page was pushed on top — or came back, through a browser's Forward.
    Push,
    /// The page on top was taken off: a pop, the system's Back, a back gesture.
    Pop,
    /// The page on top was swapped for another ([`GoRouter::replace`]).
    Replace,
    /// Another branch of a shell was shown — a tab switch.
    Branch,
}

/// A move of the router: what [`GoRouter::observe`] is told, for analytics and logs.
///
/// ```
/// use frus_widgets::{text, GoRoute, GoRouter, RouteChangeKind};
/// use std::{cell::RefCell, rc::Rc};
///
/// let router = GoRouter::new(vec![
///     GoRoute::new("/", |_, _| text("home")),
///     GoRoute::new("/about", |_, _| text("about")),
/// ]);
/// let seen = Rc::new(RefCell::new(Vec::new()));
/// let log = seen.clone();
/// let _watch = router.observe(move |change| log.borrow_mut().push((change.kind, change.to.clone())));
///
/// router.push("/about");
/// router.pop();
/// // Watching from before the router started, it is told of the first page too.
/// assert_eq!(
///     *seen.borrow(),
///     [
///         (RouteChangeKind::Go, "/".to_string()),
///         (RouteChangeKind::Push, "/about".to_string()),
///         (RouteChangeKind::Pop, "/".to_string()),
///     ]
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RouteChange {
    /// What kind of move it was.
    pub kind: RouteChangeKind,
    /// The location on top before it, `None` for the first page the router shows.
    pub from: Option<String>,
    /// The location on top after it.
    pub to: String,
    /// The branch of a shell the page on top is in, after it — the tab on show.
    pub branch: Option<usize>,
}

struct Back {
    progress: f32,
    settle: Option<AnimationController>,
    commit: bool,
}

#[derive(Default)]
struct Stack {
    pages: Vec<Page>,
    transition: Option<Transition>,
    back: Option<Back>,
    /// The pages popped off the top, the nearest **last**: what going forward again would
    /// show. They continue `pages` exactly — the first one popped sat at `pages.len()` —
    /// since any move that is not a pop clears them. Kept only up to the configured
    /// number, `keep_popped`.
    forward: Vec<Page>,
}

struct Config {
    nodes: Vec<Rc<Node>>,
    names: HashMap<String, Rc<Node>>,
    initial: String,
    redirect: Option<Redirect>,
    error: Option<RouteBuilder>,
    limit: usize,
    animate: bool,
    /// How many popped pages are kept alive for a return ([`GoRouter::keep_popped_pages`]).
    keep_popped: usize,
    shells: HashMap<u64, Rc<ShellSpec>>,
}

struct Inner {
    config: RefCell<Config>,
    stack: RefCell<Stack>,
    started: Cell<bool>,
    /// Whether a tree has been built from this router yet: until then nobody has seen a page,
    /// so there is nothing to move away from.
    built: Cell<bool>,
    /// Whether the last move swapped the page on top rather than adding or removing one.
    replaced: Cell<bool>,
    /// The transition the theme the router was last built under asks for: what the next
    /// push or pop runs for (milestone 620). The zoom, the reference's fallback, until a
    /// theme has said.
    transitions: Cell<crate::PageTransitionsBuilder>,
    unique: Cell<u64>,
    refresh: RefCell<Option<Subscription>>,
    /// Whether the stack restored for this router has been looked for (milestone 753).
    restoration_checked: Cell<bool>,
    /// The restoration scope of each page at the last build: a page that left the stack
    /// takes its scope's values with it.
    page_scopes: RefCell<Vec<Vec<String>>>,
    /// Each shell's branches, while the shell is in the stack.
    shells: RefCell<HashMap<u64, ShellStacks>>,
    /// Who is told of each move ([`GoRouter::observe`]), and the move they are told of.
    changes: crate::ChangeNotifier,
    last_change: RefCell<Option<RouteChange>>,
}

/// The routes, and the stack of pages showing which of them the person has been through.
///
/// A cheap handle: clone it into a handler and navigate from there.
#[derive(Clone)]
pub struct GoRouter {
    inner: Rc<Inner>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Go,
    Push,
    Replace,
}

fn hash_of(value: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

impl GoRouter {
    /// A router over `routes`, starting at `/`.
    pub fn new(routes: Vec<GoRoute>) -> Self {
        let mut names = HashMap::new();
        let mut shells = HashMap::new();
        let mut building = Building {
            names: &mut names,
            shells: &mut shells,
        };
        let nodes = routes
            .into_iter()
            .map(|route| build_node(route, None, None, &mut building))
            .collect();
        Self {
            inner: Rc::new(Inner {
                config: RefCell::new(Config {
                    nodes,
                    names,
                    initial: "/".to_string(),
                    redirect: None,
                    error: None,
                    limit: 5,
                    animate: true,
                    keep_popped: 0,
                    shells,
                }),
                stack: RefCell::new(Stack::default()),
                started: Cell::new(false),
                built: Cell::new(false),
                transitions: Cell::new(crate::PageTransitionsBuilder::Zoom),
                replaced: Cell::new(false),
                unique: Cell::new(0),
                refresh: RefCell::new(None),
                restoration_checked: Cell::new(false),
                page_scopes: RefCell::new(Vec::new()),
                shells: RefCell::new(HashMap::new()),
                changes: crate::ChangeNotifier::new(),
                last_change: RefCell::new(None),
            }),
        }
    }

    /// Where the router starts. `/` by default.
    pub fn initial_location(self, location: impl Into<String>) -> Self {
        self.inner.config.borrow_mut().initial = location.into();
        self
    }

    /// A redirect for every navigation: it is told where it is heading and may answer with
    /// another location, or `None` to let it through.
    ///
    /// It closes over whatever it needs to decide — a handle to the signed-in state, say.
    pub fn redirect(self, redirect: impl Fn(&GoRouterState) -> Option<String> + 'static) -> Self {
        self.inner.config.borrow_mut().redirect = Some(Rc::new(redirect));
        self
    }

    /// The page shown when no route answers to a location. A plain "not found" page by
    /// default.
    pub fn error_builder<W: Widget + 'static>(
        self,
        builder: impl Fn(&BuildContext, &GoRouterState) -> W + 'static,
    ) -> Self {
        self.inner.config.borrow_mut().error =
            Some(Rc::new(move |cx, state| Box::new(builder(cx, state))));
        self
    }

    /// How many redirects in a row are followed before the router gives up and shows the
    /// error page. Five by default.
    pub fn redirect_limit(self, limit: usize) -> Self {
        self.inner.config.borrow_mut().limit = limit;
        self
    }

    /// Whether a page slides in and out. On by default.
    pub fn animate(self, animate: bool) -> Self {
        self.inner.config.borrow_mut().animate = animate;
        self
    }

    /// Keeps up to `limit` pages that were popped **alive**, the nearest first, so that a
    /// location handed back in through [`open`](Self::open) — a browser's Forward button —
    /// finds its page as it was left: its fields, its scroll, everything its components
    /// hold. Nought by default: a popped page is let go, as a phone's back button expects.
    ///
    /// A kept page is built with the others, not laid out or drawn; it is let go as soon as
    /// the router moves anywhere that is not back — as a browser drops the entries ahead
    /// when a new one is made. The Web shell keeps a few.
    pub fn keep_popped_pages(self, limit: usize) -> Self {
        self.set_keep_popped_pages(limit);
        self
    }

    /// How many popped pages this router keeps alive ([`keep_popped_pages`](Self::keep_popped_pages)).
    pub fn popped_pages_kept(&self) -> usize {
        self.inner.config.borrow().keep_popped
    }

    /// [`keep_popped_pages`](Self::keep_popped_pages), on a router already made.
    pub fn set_keep_popped_pages(&self, limit: usize) {
        self.inner.config.borrow_mut().keep_popped = limit;
        let mut stack = self.inner.stack.borrow_mut();
        let len = stack.forward.len();
        stack.forward.drain(..len.saturating_sub(limit));
    }

    /// Runs the redirects again whenever `listenable` changes — the way a sign-in or sign-out
    /// moves the person without anyone having navigated.
    pub fn refresh_listenable(self, listenable: &impl Listenable) -> Self {
        let weak: Weak<Inner> = Rc::downgrade(&self.inner);
        let subscription = listenable.add_listener(move || {
            if let Some(inner) = weak.upgrade() {
                GoRouter { inner }.refresh();
            }
        });
        *self.inner.refresh.borrow_mut() = Some(subscription);
        self
    }

    // ---- reading -----------------------------------------------------------------------

    /// The location of the page on top.
    pub fn location(&self) -> String {
        self.start();
        self.inner
            .stack
            .borrow()
            .pages
            .last()
            .map(|page| page.state.location.clone())
            .unwrap_or_default()
    }

    /// The state of the page on top: what its location put in it.
    pub fn state(&self) -> Option<GoRouterState> {
        self.start();
        self.inner
            .stack
            .borrow()
            .pages
            .last()
            .map(|page| (*page.state).clone())
    }

    /// The location of every page in the stack, the one at the bottom first and the one on
    /// top last.
    pub fn locations(&self) -> Vec<String> {
        self.start();
        self.inner
            .stack
            .borrow()
            .pages
            .iter()
            .map(|page| page.state.location.clone())
            .collect()
    }

    /// Whether the last move swapped the page on top for another — [`replace`](Self::replace)
    /// — rather than adding a page, removing one or rebuilding the stack. What an address bar
    /// wants to know, to swap its entry too instead of adding one.
    pub fn replaced(&self) -> bool {
        self.inner.replaced.get()
    }

    /// How many pages the stack holds.
    pub fn depth(&self) -> usize {
        self.start();
        self.inner.stack.borrow().pages.len()
    }

    /// Whether [`pop`](Self::pop) would do anything: there is a page beneath the top to go
    /// back to — or, on the first page of a shell's branch, a first branch to go back to
    /// ([`ShellBack`]).
    pub fn can_pop(&self) -> bool {
        self.back_action() != BackAction::Nothing
    }

    /// The branch of a shell the page on top is in — the tab on show — or `None` when the
    /// page on top is not in a shell.
    pub fn current_branch(&self) -> Option<usize> {
        self.start();
        self.inner
            .stack
            .borrow()
            .pages
            .last()
            .and_then(|page| page.shell)
            .map(|(_, branch)| branch)
    }

    /// What the system's Back would do now: the innermost stack first — a branch's own pages
    /// — then the shell's [`ShellBack`], then the pages beneath.
    fn back_action(&self) -> BackAction {
        self.start();
        let stack = self.inner.stack.borrow();
        let pages = &stack.pages;
        let Some(top) = pages.last() else {
            return BackAction::Nothing;
        };
        if let Some((shell, branch)) = top.shell {
            let run = pages
                .iter()
                .filter(|page| page.shell.map(|(s, _)| s) == Some(shell))
                .count();
            if run > 1 {
                return BackAction::Page;
            }
            let back = self
                .inner
                .config
                .borrow()
                .shells
                .get(&shell)
                .map(|spec| spec.back);
            if branch != 0 && back == Some(ShellBack::FirstBranch) {
                return BackAction::FirstBranch(shell);
            }
        }
        if pages.len() > 1 {
            BackAction::Page
        } else {
            BackAction::Nothing
        }
    }

    /// Calls `observer` after every move of the router — a push, a pop, a `go`, a swap, a tab
    /// switch — for as long as the returned [`Subscription`] is kept: what an analytics or a
    /// log of screens listens to. See [`RouteChange`].
    ///
    /// A move that leaves the same page on top is not reported.
    pub fn observe(&self, observer: impl Fn(&RouteChange) + 'static) -> Subscription {
        use crate::Listenable;
        let weak = Rc::downgrade(&self.inner);
        self.inner.changes.add_listener(move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let change = inner.last_change.borrow().clone();
            if let Some(change) = change {
                observer(&change);
            }
        })
    }

    /// [`observe`](Self::observe), for the router's whole life, on a router being made. An
    /// observer added before the router has started is also told of the first page, with
    /// `from: None`.
    ///
    /// ```
    /// use frus_widgets::{text, GoRoute, GoRouter};
    ///
    /// let router = GoRouter::new(vec![GoRoute::new("/", |_, _| text("home"))])
    ///     .observer(|change| log::info!("screen: {}", change.to));
    /// # let _ = router;
    /// ```
    pub fn observer(self, observer: impl Fn(&RouteChange) + 'static) -> Self {
        self.observe(observer).forget();
        self
    }

    /// The location of the route called `name`, with its `:parameters` filled in and `query`
    /// appended. `None` if there is no such route, or a parameter is missing.
    pub fn location_for(
        &self,
        name: &str,
        params: &[(&str, &str)],
        query: &[(&str, &str)],
    ) -> Option<String> {
        let node = self.inner.config.borrow().names.get(name).cloned()?;
        let mut path = String::new();
        for segment in node.full_path.split('/').filter(|s| !s.is_empty()) {
            path.push('/');
            match segment.strip_prefix(':') {
                Some(param) => {
                    let (_, value) = params.iter().find(|(key, _)| *key == param)?;
                    path.push_str(&encode(value));
                }
                None => path.push_str(segment),
            }
        }
        if path.is_empty() {
            path.push('/');
        }
        if !query.is_empty() {
            let pairs: Vec<String> = query
                .iter()
                .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
                .collect();
            path.push('?');
            path.push_str(&pairs.join("&"));
        }
        Some(path)
    }

    // ---- moving ------------------------------------------------------------------------

    /// Goes to `location`: the stack becomes the routes that location is made of.
    pub fn go(&self, location: impl AsRef<str>) {
        self.navigate(location.as_ref(), None, Mode::Go);
    }

    /// [`go`](Self::go), carrying an object the page can read with
    /// [`GoRouterState::extra`].
    pub fn go_with_extra(&self, location: impl AsRef<str>, extra: impl Any) {
        self.navigate(location.as_ref(), Some(Rc::new(extra)), Mode::Go);
    }

    /// Goes to the route called `name`.
    pub fn go_named(&self, name: &str, params: &[(&str, &str)], query: &[(&str, &str)]) {
        match self.location_for(name, params, query) {
            Some(location) => self.go(location),
            None => debug_assert!(false, "no route called `{name}`, or a parameter is missing"),
        }
    }

    /// Pushes the page for `location` on top of what is here.
    pub fn push(&self, location: impl AsRef<str>) {
        self.navigate(location.as_ref(), None, Mode::Push);
    }

    /// [`push`](Self::push), carrying an object the page can read with
    /// [`GoRouterState::extra`].
    pub fn push_with_extra(&self, location: impl AsRef<str>, extra: impl Any) {
        self.navigate(location.as_ref(), Some(Rc::new(extra)), Mode::Push);
    }

    /// Pushes the route called `name`.
    pub fn push_named(&self, name: &str, params: &[(&str, &str)], query: &[(&str, &str)]) {
        match self.location_for(name, params, query) {
            Some(location) => self.push(location),
            None => debug_assert!(false, "no route called `{name}`, or a parameter is missing"),
        }
    }

    /// Pushes the page for `location`, and answers **what it is popped with**: the value
    /// handed to [`pop_with`](Self::pop_with) on the page, or `None` when it went otherwise —
    /// the system's Back, a back gesture, the app bar's arrow, a `go` elsewhere, or a value
    /// that is not a `T`.
    ///
    /// The answer is a future, awaited in a task — an [`async_callback`](BuildContext::async_callback):
    ///
    /// ```ignore
    /// on_tap: cx.async_callback(move |state| {
    ///     let router = router.clone();
    ///     async move {
    ///         let picked: Option<Color> = router.push_for_result("/pick").await;
    ///         if let Some(color) = picked {
    ///             state.update(|s| s.color = color);
    ///         }
    ///     }
    /// })
    /// ```
    ///
    /// A page left in a branch of a [shell](GoRoute::shell) while another branch is shown has
    /// not been popped: the answer waits. [`push_then`](Self::push_then) is the same with a
    /// closure instead of a future.
    pub fn push_for_result<T: 'static>(&self, location: impl AsRef<str>) -> PopResult<T> {
        let shared = Rc::new(PopShared {
            value: RefCell::new(None),
            waker: RefCell::new(None),
        });
        let filled = shared.clone();
        self.push_then(location, move |value: Option<T>| {
            *filled.value.borrow_mut() = Some(value);
            let waker = filled.waker.borrow_mut().take();
            if let Some(waker) = waker {
                waker.wake();
            }
        });
        PopResult { shared }
    }

    /// [`push_for_result`](Self::push_for_result), with `then` called with what the page is
    /// popped with — `None` when it went any other way.
    ///
    /// ```
    /// use frus_widgets::{text, GoRoute, GoRouter};
    /// use std::{cell::Cell, rc::Rc};
    ///
    /// let router = GoRouter::new(vec![
    ///     GoRoute::new("/", |_, _| text("home")),
    ///     GoRoute::new("/pick", |_, _| text("pick a number")),
    /// ]);
    /// let picked = Rc::new(Cell::new(None));
    /// let answer = picked.clone();
    /// router.push_then("/pick", move |value: Option<u32>| answer.set(value));
    ///
    /// router.pop_with(7u32); // on the page, when a number is chosen
    /// assert_eq!(picked.get(), Some(7));
    /// ```
    pub fn push_then<T: 'static>(
        &self,
        location: impl AsRef<str>,
        then: impl FnOnce(Option<T>) + 'static,
    ) {
        let slot = PopSlot::new(move |value| {
            let value = value.and_then(|value| match value.downcast::<T>() {
                Ok(value) => Some(*value),
                Err(_) => {
                    log::warn!(
                        "a page was popped with a value of another type than the {} it was pushed for",
                        std::any::type_name::<T>()
                    );
                    None
                }
            });
            then(value);
        });
        self.navigate_as(
            location.as_ref(),
            None,
            Mode::Push,
            RouteChangeKind::Push,
            Some(slot),
        );
    }

    /// Takes the page on top off, handing `value` to whoever pushed it for a result
    /// ([`push_for_result`](Self::push_for_result)). Whether there was a page to take off: on
    /// the last page, or the first page of a shell's branch, nothing happens and `value` is
    /// dropped.
    pub fn pop_with<T: 'static>(&self, value: T) -> bool {
        if self.back_action() != BackAction::Page {
            return false;
        }
        let slot = self
            .inner
            .stack
            .borrow()
            .pages
            .last()
            .and_then(|page| page.result.clone());
        if let Some(slot) = slot {
            *slot.value.borrow_mut() = Some(Box::new(value));
        }
        self.pop()
    }

    /// Shows branch `index` of the shell on show (see [`NavigationShell::go_branch`]).
    /// Nothing when the page on top is in no shell.
    pub fn go_branch(&self, index: usize) {
        self.start();
        let shell = self
            .inner
            .stack
            .borrow()
            .pages
            .iter()
            .rev()
            .find_map(|page| page.shell)
            .map(|(shell, _)| shell);
        match shell {
            Some(shell) => self.show_branch(shell, index),
            None => debug_assert!(false, "`go_branch`: no shell is on show"),
        }
    }

    /// Shows branch `index` of `shell`: as it was left, or from its start the first time;
    /// the branch on show again goes back to its first page.
    fn show_branch(&self, shell: u64, index: usize) {
        self.start();
        let Some(spec) = self.inner.config.borrow().shells.get(&shell).cloned() else {
            return;
        };
        if index >= spec.initial.len() {
            debug_assert!(false, "the shell has no branch {index}");
            return;
        }
        let current = self.inner.stack.borrow().pages.clone();
        let start = current
            .iter()
            .position(|page| page.shell.map(|(s, _)| s) == Some(shell));
        let active = start.and_then(|at| current[at].shell).map(|(_, b)| b);
        if let (Some(start), Some(active)) = (start, active) {
            if active == index {
                // The tab on show, chosen again: back to its first page, as phones do.
                if current.len() > start + 1 {
                    self.commit(current[..=start].to_vec(), RouteChangeKind::Pop);
                    self.inner.replaced.set(false);
                }
                return;
            }
        }
        // Where the branch was left, or where it starts; a `go` there finds the branch's
        // stack as it was (`pages_for`), and lets the redirects have their say.
        let target = self
            .inner
            .shells
            .borrow()
            .get(&shell)
            .and_then(|stacks| stacks.branches[index].last().cloned())
            .map(|page| page.state.location.clone());
        let location = target.unwrap_or_else(|| spec.initial[index].clone());
        let pages = self.pages_for(&location, None, Mode::Go, &current);
        self.commit(pages, RouteChangeKind::Branch);
        self.inner.replaced.set(false);
    }

    /// Goes to a location that came from **outside** the application: an address typed, a
    /// link followed, the browser's back button. Like [`go`](Self::go), except that a
    /// location the router is already at is left alone, and that before anything has been
    /// built there is no page to slide away from, so the router simply starts there.
    ///
    /// It is what the shell's `FrusApp` hands an address to.
    ///
    /// A location that is a page **beneath** the top goes back to that page, keeping it and
    /// what is under it — a page that was pushed too, which a `go` would have rebuilt from
    /// the location; one that is a page the router [kept](Self::keep_popped_pages) after
    /// going back from it brings it back as it was. Either way only when no redirect sends
    /// the location elsewhere.
    pub fn open(&self, location: impl AsRef<str>) {
        let location = location.as_ref();
        if self.location() == location {
            return;
        }
        if !self.return_to(location) {
            self.go(location);
        }
        if !self.inner.built.get() {
            self.inner.stack.borrow_mut().transition = None;
        }
    }

    /// Goes back, or forward again, to a page the stack holds or kept for `location`.
    /// Whether there was one.
    fn return_to(&self, location: &str) -> bool {
        self.start();
        let (current, forward) = {
            let stack = self.inner.stack.borrow();
            (stack.pages.clone(), stack.forward.clone())
        };
        let below = current[..current.len().saturating_sub(1)]
            .iter()
            .rposition(|page| page.state.location == location);
        let ahead = forward
            .iter()
            .rposition(|page| page.state.location == location);
        if below.is_none() && ahead.is_none() {
            return false;
        }
        // A redirect has its say, as on any navigation: a page it would send elsewhere is not
        // returned to.
        let heading = self.pages_for(location, None, Mode::Go, &current);
        if heading.last().map(|page| page.state.location.as_str()) != Some(location) {
            return false;
        }
        match (below, ahead) {
            (Some(at), _) => self.commit(current[..=at].to_vec(), RouteChangeKind::Pop),
            (None, Some(at)) => {
                // The kept pages from the nearest to this one, back on the stack.
                let mut pages = current;
                pages.extend(forward[at..].iter().rev().cloned());
                let rest = forward[..at].to_vec();
                self.commit(pages, RouteChangeKind::Push);
                self.inner.stack.borrow_mut().forward = rest;
            }
            (None, None) => unreachable!(),
        }
        self.inner.replaced.set(false);
        true
    }

    /// Replaces the page on top with the one for `location`, so going back skips the one it
    /// replaced.
    pub fn replace(&self, location: impl AsRef<str>) {
        self.navigate(location.as_ref(), None, Mode::Replace);
    }

    /// Goes back: removes the page on top. Whether there was anything to go back from: the
    /// last page stays.
    ///
    /// On the first page of a [shell](GoRoute::shell)'s branch that is not the first, it
    /// shows the first branch instead, unless the shell says otherwise ([`ShellBack`]). It is
    /// what the system's Back does.
    pub fn pop(&self) -> bool {
        match self.back_action() {
            BackAction::Nothing => false,
            BackAction::FirstBranch(shell) => {
                self.show_branch(shell, 0);
                true
            }
            BackAction::Page => {
                let pages = {
                    let stack = self.inner.stack.borrow();
                    stack.pages[..stack.pages.len() - 1].to_vec()
                };
                self.commit(pages, RouteChangeKind::Pop);
                self.inner.replaced.set(false);
                true
            }
        }
    }

    /// Asks the redirects again about where the top page is. What a
    /// [`refresh listenable`](Self::refresh_listenable) does when it changes.
    pub fn refresh(&self) {
        self.start();
        let target = self
            .inner
            .stack
            .borrow()
            .pages
            .last()
            .map(|page| (page.state.location.clone(), page.state.extra.clone()));
        if let Some((location, extra)) = target {
            self.navigate(&location, extra, Mode::Go);
        }
    }

    // ---- the machinery -----------------------------------------------------------------

    fn start(&self) {
        if self.inner.started.replace(true) {
            return;
        }
        let initial = self.inner.config.borrow().initial.clone();
        let pages = self.pages_for(&initial, None, Mode::Go, &[]);
        self.install(pages, RouteChangeKind::Go, false);
    }

    fn navigate(&self, location: &str, extra: Option<Rc<dyn Any>>, mode: Mode) {
        let kind = match mode {
            Mode::Go => RouteChangeKind::Go,
            Mode::Push => RouteChangeKind::Push,
            Mode::Replace => RouteChangeKind::Replace,
        };
        self.navigate_as(location, extra, mode, kind, None);
    }

    fn navigate_as(
        &self,
        location: &str,
        extra: Option<Rc<dyn Any>>,
        mode: Mode,
        kind: RouteChangeKind,
        result: Option<Rc<PopSlot>>,
    ) {
        self.start();
        let current = self.inner.stack.borrow().pages.clone();
        let mut pages = self.pages_for(location, extra, mode, &current);
        if let (Some(result), Some(top)) = (result, pages.last_mut()) {
            top.result = Some(result);
        }
        self.commit(pages, kind);
        self.inner.replaced.set(mode == Mode::Replace);
    }

    /// The stack that results from `mode` to `location`, redirects followed.
    fn pages_for(
        &self,
        location: &str,
        extra: Option<Rc<dyn Any>>,
        mode: Mode,
        current: &[Page],
    ) -> Vec<Page> {
        let (nodes, redirect, limit) = {
            let config = self.inner.config.borrow();
            (config.nodes.clone(), config.redirect.clone(), config.limit)
        };
        let mut location = location.to_string();
        let mut seen = 0usize;
        loop {
            let parts = split_location(&location);
            let mut params = HashMap::new();
            let chain = match_nodes(&nodes, &parts.segments, &mut params);
            let query: HashMap<String, String> = parts.query.iter().cloned().collect();
            let leaf = chain.as_ref().and_then(|chain| chain.last());
            let state = GoRouterState {
                location: location.clone(),
                path: leaf.map(|n| n.full_path.clone()).unwrap_or_default(),
                name: leaf.and_then(|n| n.name.clone()),
                params: params.clone(),
                query: query.clone(),
                extra: extra.clone(),
                entering: 1.0,
            };

            // Redirects: the router's own, then each route's, parent first.
            let mut sent = redirect.as_ref().and_then(|r| r(&state));
            if sent.is_none() {
                if let Some(chain) = &chain {
                    sent = chain
                        .iter()
                        .filter_map(|node| node.redirect.as_ref())
                        .find_map(|r| r(&state));
                }
            }
            if let Some(next) = sent.filter(|next| *next != location) {
                seen += 1;
                if seen > limit {
                    let state = GoRouterState {
                        location: location.clone(),
                        path: String::new(),
                        name: None,
                        params: HashMap::new(),
                        query: HashMap::new(),
                        extra: None,
                        entering: 1.0,
                    };
                    return self.error_pages(state, current, mode);
                }
                location = next;
                continue;
            }

            let Some(chain) = chain else {
                return self.error_pages(state, current, mode);
            };
            return match mode {
                Mode::Go => {
                    // One page per route of the chain, each keyed by the part of the location
                    // it answered to, so a page that is still on the way keeps its state. A
                    // shell takes no part of the location and is no page of its own.
                    let mut prefix = String::new();
                    let mut used = 0;
                    let mut pages: Vec<Page> = chain
                        .iter()
                        .enumerate()
                        .filter(|(_, node)| node.spec.is_none())
                        .map(|(at, node)| {
                            let mut scratch = HashMap::new();
                            let took =
                                consume(&node.segments, &parts.segments[used..], &mut scratch)
                                    .unwrap_or(0);
                            for segment in &parts.segments[used..used + took] {
                                prefix.push('/');
                                prefix.push_str(segment);
                            }
                            used += took;
                            let page_state = if at + 1 == chain.len() {
                                state.clone()
                            } else {
                                // A page underneath stands for the part of the location it
                                // answered — `/` under `/users/42` — so going back to it lands
                                // on *its* location, not on where the person had been heading.
                                GoRouterState {
                                    location: if prefix.is_empty() {
                                        "/".to_string()
                                    } else {
                                        prefix.clone()
                                    },
                                    path: node.full_path.clone(),
                                    name: node.name.clone(),
                                    ..state.clone()
                                }
                            };
                            Page {
                                key: hash_of(("go", prefix.clone(), node.full_path.clone())),
                                node: Some(node.clone()),
                                state: Rc::new(page_state),
                                pushed: false,
                                shell: node.member,
                                result: None,
                            }
                        })
                        .collect();
                    // A location that is where a shell's branch was left — a tab shown
                    // again, a browser's Back to it — finds that branch's stack as it was:
                    // its pushed pages, and so their state.
                    if extra.is_none() {
                        if let Some((shell, branch)) = pages.last().and_then(|page| page.shell) {
                            let saved = self
                                .inner
                                .shells
                                .borrow()
                                .get(&shell)
                                .map(|stacks| stacks.branches[branch].clone())
                                .unwrap_or_default();
                            if saved
                                .last()
                                .is_some_and(|page| page.state.location == location)
                            {
                                let start = pages
                                    .iter()
                                    .position(|page| page.shell.is_some())
                                    .unwrap_or(pages.len());
                                pages.truncate(start);
                                pages.extend(saved);
                            }
                        }
                    }
                    pages
                }
                Mode::Push | Mode::Replace => {
                    let mut pages = current.to_vec();
                    if mode == Mode::Replace {
                        pages.pop();
                    }
                    let leaf = chain.last().expect("a match has a route").clone();
                    // A page of a shell's branch goes under the shell when the shell is on
                    // top: onto its branch, which is shown first if it was not. Pushed over
                    // anything else, it covers the whole window.
                    // The shell on show is the one of the page on top *before* a replace
                    // took it off: replacing a branch's only page keeps the new one there.
                    let mut shell = None;
                    if let Some((s, branch)) = leaf.member {
                        if let Some((_, on_show)) = current
                            .last()
                            .and_then(|page| page.shell)
                            .filter(|(top, _)| *top == s)
                        {
                            if on_show != branch {
                                let start = pages
                                    .iter()
                                    .position(|page| page.shell.is_some())
                                    .unwrap_or(pages.len());
                                pages.truncate(start);
                                pages.extend(self.branch_pages(s, branch, current));
                            }
                            shell = if on_show == branch {
                                Some((s, branch))
                            } else {
                                pages
                                    .last()
                                    .and_then(|page| page.shell)
                                    .filter(|(top, b)| *top == s && *b == branch)
                            };
                        }
                    }
                    let unique = self.inner.unique.get() + 1;
                    self.inner.unique.set(unique);
                    pages.push(Page {
                        key: hash_of(("push", unique)),
                        node: Some(leaf),
                        state: Rc::new(state),
                        pushed: true,
                        shell,
                        result: None,
                    });
                    pages
                }
            };
        }
    }

    /// The stack of a shell's branch, to show it: as it was left, or from where it starts.
    /// Only the branch's own pages — nothing, if its start is sent outside it.
    fn branch_pages(&self, shell: u64, branch: usize, current: &[Page]) -> Vec<Page> {
        let saved = self
            .inner
            .shells
            .borrow()
            .get(&shell)
            .map(|stacks| stacks.branches[branch].clone())
            .unwrap_or_default();
        if !saved.is_empty() {
            return saved;
        }
        let Some(initial) = self
            .inner
            .config
            .borrow()
            .shells
            .get(&shell)
            .map(|spec| spec.initial[branch].clone())
        else {
            return Vec::new();
        };
        self.pages_for(&initial, None, Mode::Go, current)
            .into_iter()
            .filter(|page| page.shell == Some((shell, branch)))
            .collect()
    }

    fn error_pages(&self, state: GoRouterState, current: &[Page], mode: Mode) -> Vec<Page> {
        let unique = self.inner.unique.get() + 1;
        self.inner.unique.set(unique);
        let page = Page {
            key: hash_of(("error", unique)),
            node: None,
            state: Rc::new(state),
            pushed: mode != Mode::Go,
            shell: None,
            result: None,
        };
        match mode {
            Mode::Go => vec![page],
            Mode::Push => {
                let mut pages = current.to_vec();
                pages.push(page);
                pages
            }
            Mode::Replace => {
                let mut pages = current.to_vec();
                pages.pop();
                pages.push(page);
                pages
            }
        }
    }

    /// Makes `pages` the stack, starting the transition from the page that was on top.
    fn commit(&self, pages: Vec<Page>, kind: RouteChangeKind) {
        self.install(pages, kind, true);
    }

    /// Makes `pages` the stack — with the transition from the page that was on top when
    /// `slide` says so — and then keeps the shells' branches, settles the results of the
    /// pages that left, and tells the observers.
    fn install(&self, pages: Vec<Page>, kind: RouteChangeKind, slide: bool) {
        let animate = slide && self.inner.config.borrow().animate;
        let keep = self.inner.config.borrow().keep_popped;
        let shells_before = self.inner.shells.borrow().clone();
        let mut stack = self.inner.stack.borrow_mut();
        let before = stack.pages.clone();
        // A move back — the new stack the bottom of the old one — keeps what it took off for
        // a return; any other move leaves nothing ahead to return to.
        let kept = pages.len() < stack.pages.len()
            && pages
                .iter()
                .zip(&stack.pages)
                .all(|(new, old)| new.key == old.key);
        if kept && keep > 0 {
            let popped: Vec<Page> = stack.pages[pages.len()..].iter().rev().cloned().collect();
            stack.forward.extend(popped);
            let len = stack.forward.len();
            stack.forward.drain(..len.saturating_sub(keep));
        } else {
            stack.forward.clear();
        }
        let old_top = stack.pages.last().cloned();
        let new_top = pages.last().cloned();
        // Another branch of the same shell: tabs switch at once, as they do on phones.
        let switches_branch = match (
            old_top.as_ref().and_then(|p| p.shell),
            new_top.as_ref().and_then(|p| p.shell),
        ) {
            (Some((a, x)), Some((b, y))) => a == b && x != y,
            _ => false,
        };
        if switches_branch {
            stack.transition = None;
        }
        if let (true, false, Some(old), Some(new)) = (animate, switches_branch, &old_top, &new_top)
        {
            if old.key != new.key {
                let forward = !stack.pages.iter().any(|page| page.key == new.key);
                // **Linear, over the transition's own duration** (milestone 620): the
                // reference's route animation runs at a constant pace, and each transition
                // shapes it with its own curves. It was a spring, shaped by nobody.
                let mut controller = AnimationController::unit();
                controller.set_value(0.0);
                controller.forward(
                    self.inner.transitions.get().transition_duration(),
                    Curve::Linear,
                );
                stack.transition = Some(Transition {
                    from: old.clone(),
                    from_shell: old
                        .shell
                        .and_then(|(shell, _)| shells_before.get(&shell).cloned()),
                    controller,
                    forward,
                });
            }
        }
        stack.pages = pages;
        stack.back = None;
        drop(stack);
        self.keep_branches();
        request_rebuild();

        // The observers first: what a page's result starts may move the router again, and
        // that move is reported after this one.
        let moved = match (&old_top, &new_top) {
            (Some(old), Some(new)) => old.key != new.key,
            (None, Some(_)) => true,
            _ => false,
        };
        if moved && self.inner.changes.listener_count() > 0 {
            if let Some(new) = &new_top {
                *self.inner.last_change.borrow_mut() = Some(RouteChange {
                    kind,
                    from: old_top.as_ref().map(|page| page.state.location.clone()),
                    to: new.state.location.clone(),
                    branch: new.shell.map(|(_, branch)| branch),
                });
                self.inner.changes.notify_listeners();
            }
        }
        self.settle_results(&before, &shells_before);
    }

    /// Keeps each shell's branches in step with the stack: the run of a shell's pages is its
    /// branch on show; a shell no longer in the stack is let go, with what its branches held.
    fn keep_branches(&self) {
        let pages = self.inner.stack.borrow().pages.clone();
        let specs = self.inner.config.borrow().shells.clone();
        let mut shells = self.inner.shells.borrow_mut();
        let mut present = Vec::new();
        for page in &pages {
            let Some((shell, branch)) = page.shell else {
                continue;
            };
            if !present.contains(&shell) {
                present.push(shell);
                let count = specs.get(&shell).map_or(0, |spec| spec.initial.len());
                let stacks = shells.entry(shell).or_insert_with(|| ShellStacks {
                    current: branch,
                    branches: vec![Vec::new(); count],
                });
                stacks.current = branch;
                stacks.branches[branch].clear();
            }
            if let Some(stacks) = shells.get_mut(&shell) {
                stacks.branches[branch].push(page.clone());
            }
        }
        shells.retain(|shell, _| present.contains(shell));
    }

    /// Tells whoever waits on a page that has left — gone from the stack and from every
    /// branch — what it was popped with.
    fn settle_results(&self, before: &[Page], shells_before: &HashMap<u64, ShellStacks>) {
        let waiting: Vec<&Page> = before
            .iter()
            .chain(
                shells_before
                    .values()
                    .flat_map(|stacks| stacks.branches.iter().flatten()),
            )
            .filter(|page| page.result.is_some())
            .collect();
        if waiting.is_empty() {
            return;
        }
        let alive: Vec<u64> = {
            let stack = self.inner.stack.borrow();
            let shells = self.inner.shells.borrow();
            stack
                .pages
                .iter()
                .chain(
                    shells
                        .values()
                        .flat_map(|stacks| stacks.branches.iter().flatten()),
                )
                .map(|page| page.key)
                .collect()
        };
        let mut done = Vec::new();
        for page in waiting {
            if !alive.contains(&page.key) && !done.contains(&page.key) {
                done.push(page.key);
                if let Some(result) = &page.result {
                    result.finish();
                }
            }
        }
    }

    // ---- driven by the shell -----------------------------------------------------------

    /// Advances the page transition and a back gesture's settling by `dt` seconds. Whether
    /// anything is still moving.
    pub fn tick(&self, dt: f32) -> bool {
        let mut animating = false;
        let mut pop = false;
        {
            let mut stack = self.inner.stack.borrow_mut();
            let arrived = match stack.transition.as_mut() {
                Some(transition) => {
                    if transition.controller.tick(dt) {
                        animating = true;
                        false
                    } else {
                        true
                    }
                }
                None => false,
            };
            if arrived {
                stack.transition = None;
            }
            let mut settled = false;
            if let Some(back) = stack.back.as_mut() {
                if let Some(settle) = back.settle.as_mut() {
                    if settle.tick(dt) {
                        back.progress = settle.value();
                        animating = true;
                    } else {
                        pop = back.commit;
                        settled = true;
                    }
                }
            }
            if settled {
                stack.back = None;
            }
        }
        if pop {
            // The finger has already carried the page away: there is no transition left to
            // play, only the stack to catch up with.
            let pages = {
                let stack = self.inner.stack.borrow();
                (stack.pages.len() > 1).then(|| stack.pages[..stack.pages.len() - 1].to_vec())
            };
            if let Some(pages) = pages {
                self.install(pages, RouteChangeKind::Pop, false);
            }
        }
        animating
    }

    /// Whether a back gesture can start: there is a page to take off — in a shell, one of the
    /// branch's own first. A gesture never switches branches; the system's Back may
    /// ([`can_pop`](Self::can_pop)).
    pub fn can_go_back(&self) -> bool {
        self.back_action() == BackAction::Page
    }

    /// A back gesture is under way, `progress` of the way from `0` to `1`.
    pub fn back_gesture(&self, progress: f32) {
        let mut stack = self.inner.stack.borrow_mut();
        match stack.back.as_mut() {
            Some(back) => back.progress = progress,
            None => {
                stack.back = Some(Back {
                    progress,
                    settle: None,
                    commit: false,
                })
            }
        }
        drop(stack);
        request_rebuild();
    }

    /// The finger lifted, moving at `velocity` (fractions per second): the page goes back or
    /// stays, whichever the momentum and the position say.
    pub fn back_gesture_end(&self, velocity: f32) {
        let deep_enough = self.back_action() == BackAction::Page;
        let mut stack = self.inner.stack.borrow_mut();
        let cupertino = self.inner.transitions.get() == crate::PageTransitionsBuilder::Cupertino;
        if let Some(back) = stack.back.as_mut() {
            let mut settle = AnimationController::unit();
            settle.set_value(back.progress);
            let commit = if cupertino {
                // **The iOS page's own rules** (`route.dart:851`, milestone 621): moving
                // fast enough, its direction decides; otherwise, whether it was past
                // halfway. Then it finishes in 350 ms on its curve, wherever it was.
                let goes = if velocity.abs() >= CUPERTINO_FLING {
                    velocity > 0.0
                } else {
                    back.progress >= 0.5
                };
                let commit = goes && deep_enough;
                settle.animate_to(
                    if commit { 1.0 } else { 0.0 },
                    CUPERTINO_SETTLE,
                    Curve::fast_ease_in_to_slow_ease_out(),
                );
                commit
            } else {
                // Elsewhere the swipe is frus's own: where the finger's momentum would
                // carry the page, and a spring from there.
                let projected = back.progress + velocity * BACK_PROJECT;
                let commit = projected > BACK_COMMIT_POS && deep_enough;
                settle.spring_to(if commit { 1.0 } else { 0.0 }, spring(), velocity);
                commit
            };
            back.commit = commit;
            back.settle = Some(settle);
        }
        drop(stack);
        request_rebuild();
    }

    // ---- restoration -------------------------------------------------------------------

    /// At the first build: the stack restored for this router, if the platform handed one
    /// back, replaces the one it starts with; and from then on the stack is kept
    /// (milestone 753).
    fn restore(&self, cx: &BuildContext) {
        if self.inner.restoration_checked.replace(true) {
            return;
        }
        let restoration = &cx.runtime().restoration;
        if !restoration.is_enabled() || !restoration.restores_router() {
            return;
        }
        let path = cx.restoration_path("router");
        if let Some(saved) = restoration.claim(&path) {
            self.restore_stack(&saved);
        }
        let weak = Rc::downgrade(&self.inner);
        restoration.register(path, move || match weak.upgrade() {
            Some(inner) => crate::restoration::Reading::Value(write_stack(&inner.stack.borrow())),
            None => crate::restoration::Reading::Gone,
        });
    }

    /// Rebuilds the stack `saved` describes: the pages a `go` made, then each one pushed on
    /// top. Left as it is when a link opened the application somewhere else since.
    fn restore_stack(&self, saved: &str) {
        let entries = read_stack(saved);
        let Some(top) = entries.last() else {
            return;
        };
        let here = self.location();
        let initial = self.inner.config.borrow().initial.clone();
        if here != initial && here != top.1 {
            return;
        }
        let base = entries
            .iter()
            .rev()
            .find(|(pushed, _)| !pushed)
            .map(|(_, location)| location.clone())
            .unwrap_or(initial);
        let mut pages = self.pages_for(&base, None, Mode::Go, &[]);
        for (_, location) in entries.iter().filter(|(pushed, _)| *pushed) {
            pages = self.pages_for(location, None, Mode::Push, &pages);
        }
        self.install(pages, RouteChangeKind::Go, false);
        let mut stack = self.inner.stack.borrow_mut();
        stack.transition = None;
        stack.back = None;
        drop(stack);
        self.inner.replaced.set(false);
    }

    // ---- building ----------------------------------------------------------------------

    /// The widget for the router's pages: what an application whose root is a router
    /// builds. It also makes the router reachable from below, with [`BuildContext::router`].
    pub fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        self.start();
        self.restore(cx);
        self.inner.built.set(true);
        let theme = cx.theme();
        self.inner
            .transitions
            .set(theme.page_transitions.builder_for(theme.platform));
        cx.runtime().states.provide(Rc::new(self.clone()));
        let error_builder = self.inner.config.borrow().error.clone();
        // What is on the stack is copied out, so that a page's builder can ask the router
        // things without finding it borrowed.
        let (pages, transition, back, kept) = {
            let stack = self.inner.stack.borrow();
            (
                stack.pages.clone(),
                stack.transition.as_ref().map(|t| {
                    (
                        t.from.clone(),
                        t.from_shell.clone(),
                        t.controller.value(),
                        t.forward,
                    )
                }),
                stack.back.as_ref().map(|b| b.progress),
                stack.forward.clone(),
            )
        };
        let shells = self.inner.shells.borrow().clone();
        let specs = self.inner.config.borrow().shells.clone();
        let depth = pages.len();
        // A page is built told how far in it is: the transition's own number, seen from
        // this page — the one arriving has it as it is, the one leaving has it the other way.
        // A page with one beneath it knows the way back: its app bar grows the arrow
        // (milestone 716). Every page is in a scope, the first one too, so that a page
        // keeps its place in the tree whatever is pushed over it.
        let pop = {
            let router = self.clone();
            crate::Callback::new(move || {
                router.pop();
            })
        };
        // Every page has a restoration scope of its own, named by where it is in the stack
        // and where it leads: ids only have to be unique within a page, and a page that
        // leaves the stack takes its values with it. A page of a shell's branch is named by
        // its place in the branch, which does not change as other tabs are shown.
        let mut named: Vec<(u64, String)> = Vec::new();
        let mut run_start: HashMap<u64, usize> = HashMap::new();
        for (at, page) in pages.iter().enumerate() {
            let name = match page.shell {
                Some((shell, branch)) => {
                    let start = *run_start.entry(shell).or_insert(at);
                    format!(
                        "shell {shell} branch {branch} route {} {}",
                        at - start,
                        page.state.location
                    )
                }
                None => format!("route {at} {}", page.state.location),
            };
            named.push((page.key, name));
        }
        for (shell, stacks) in &shells {
            for (branch, stack) in stacks.branches.iter().enumerate() {
                if branch == stacks.current {
                    continue;
                }
                for (at, page) in stack.iter().enumerate() {
                    named.push((
                        page.key,
                        format!(
                            "shell {shell} branch {branch} route {at} {}",
                            page.state.location
                        ),
                    ));
                }
            }
        }
        let scopes: Vec<(u64, Vec<String>)> = named
            .into_iter()
            .map(|(key, name)| {
                (
                    key,
                    crate::restoration::scope_below(cx.scope(), &name).to_vec(),
                )
            })
            .collect();
        {
            let mut before = self.inner.page_scopes.borrow_mut();
            for gone in before
                .iter()
                .filter(|scope| !scopes.iter().any(|(_, s)| s == *scope))
            {
                cx.runtime().restoration.forget_scope(gone);
            }
            *before = scopes.iter().map(|(_, scope)| scope.clone()).collect();
        }
        let scope_of = |page: &Page| -> crate::restoration::Scope {
            match scopes.iter().find(|(key, _)| *key == page.key) {
                Some((_, scope)) => Rc::from(scope.clone()),
                // The page leaving, after a pop: its scope is already forgotten.
                None => crate::restoration::scope_below(cx.scope(), "route leaving"),
            }
        };
        let make_page = |page: &Page, entering: f32| -> Box<dyn Widget> {
            let _scope = crate::restoration::enter_scope(scope_of(page));
            let state = GoRouterState {
                entering,
                ..(*page.state).clone()
            };
            match &page.node {
                Some(node) => (node.builder)(cx, &state),
                None => match &error_builder {
                    Some(builder) => builder(cx, &state),
                    None => Box::new(crate::Text::new(format!(
                        "Page not found: {}",
                        state.location
                    ))),
                },
            }
        };
        let make = |page: &Page, back: bool, entering: f32| -> Box<dyn Widget> {
            Box::new(crate::ScaffoldScope::new(
                crate::ScaffoldInfo::route(back.then(|| pop.clone())),
                make_page(page, entering),
            ))
        };

        // Which navigator plays the motion under way: a shell's, when both pages are in its
        // branch — the frame stays put and only the branch moves — else the outer one.
        // Between two branches, nothing moves.
        let play = |a: &Page, b: &Page| -> Option<Option<u64>> {
            match (a.shell, b.shell) {
                (Some((s, x)), Some((t, y))) if s == t => (x == y).then_some(Some(s)),
                _ => Some(None),
            }
        };
        let top = pages.last().expect("a router has at least one page");
        let gesture = back
            .filter(|_| depth >= 2)
            .and_then(|progress| Some((progress, play(top, &pages[depth - 2])?)));
        let slide = transition
            .as_ref()
            .and_then(|(from, snapshot, progress, forward)| {
                Some((from, snapshot, *progress, *forward, play(from, top)?))
            });
        let gesture_in = |scope: Option<u64>| {
            gesture
                .filter(|(_, at)| *at == scope)
                .map(|(progress, _)| progress)
        };
        let slide_in = |scope: Option<u64>| {
            slide
                .as_ref()
                .filter(|(.., at)| *at == scope)
                .map(|(from, _, progress, forward, _)| ((*from).clone(), *progress, *forward))
        };

        // A shell: its frame, around a navigator of its own holding every branch's pages —
        // the branch on show, and the others kept, so that what they hold is still there when
        // they are shown again.
        let make_shell = |shell: u64, stacks: &ShellStacks, live: bool, back: bool| {
            let branch = stacks.current;
            let run = &stacks.branches[branch];
            let inner_key = |b: usize, page: &Page| hash_of((b, page.key));
            let mut slots: Vec<Slot> = run
                .iter()
                .enumerate()
                .map(|(at, page)| Slot {
                    key: (at, inner_key(branch, page)),
                    page: page.key,
                    make: Box::new(move |entering| make(page, at > 0, entering)),
                })
                .collect();
            if live {
                let mine = kept
                    .iter()
                    .rev()
                    .filter(|page| page.shell.map(|(s, _)| s) == Some(shell));
                for (back, page) in mine.enumerate() {
                    slots.push(Slot {
                        key: (run.len() + back, inner_key(branch, page)),
                        page: page.key,
                        make: Box::new(move |entering| make(page, true, entering)),
                    });
                }
            }
            for (other, stack) in stacks.branches.iter().enumerate() {
                if other == branch {
                    continue;
                }
                for (at, page) in stack.iter().enumerate() {
                    slots.push(Slot {
                        key: (at, inner_key(other, page)),
                        page: page.key,
                        make: Box::new(move |entering| make(page, at > 0, entering)),
                    });
                }
            }
            let motion = match (
                live.then(|| gesture_in(Some(shell))).flatten(),
                live.then(|| slide_in(Some(shell))).flatten(),
            ) {
                (Some(progress), _) => Motion::Gesture(progress),
                (None, Some((from, progress, forward))) => {
                    let at = match slots.iter().position(|slot| slot.page == from.key) {
                        Some(at) => at,
                        None => {
                            slots.push(Slot {
                                key: (run.len(), inner_key(branch, &from)),
                                page: from.key,
                                make: Box::new(move |entering| make(&from, true, entering)),
                            });
                            slots.len() - 1
                        }
                    };
                    Motion::Slide {
                        from: at,
                        progress,
                        forward,
                    }
                }
                _ => Motion::Still,
            };
            let navigator = assemble(slots, run.len(), motion).expand();
            let handle = NavigationShell {
                router: self.clone(),
                shell,
                current: branch,
                count: stacks.branches.len(),
            };
            let frame = match specs.get(&shell) {
                Some(spec) => (spec.builder)(cx, &handle, Box::new(navigator)),
                None => Box::new(navigator),
            };
            Box::new(crate::ScaffoldScope::new(
                crate::ScaffoldInfo::route(back.then(|| pop.clone())),
                frame,
            )) as Box<dyn Widget>
        };

        // The outer stack: each page not in a shell, and each shell once, for its pages.
        let mut units: Vec<Unit> = Vec::new();
        for (at, page) in pages.iter().enumerate() {
            match page.shell {
                Some((shell, _)) => {
                    if !matches!(units.last(), Some(Unit::Shell(s)) if *s == shell) {
                        units.push(Unit::Shell(shell));
                    }
                }
                None => units.push(Unit::Page(at)),
            }
        }
        let unit_key = |unit: &Unit| match unit {
            Unit::Page(at) => pages[*at].key,
            Unit::Shell(shell) => hash_of(("shell", *shell)),
        };
        let page_unit_key = |page: &Page| match page.shell {
            Some((shell, _)) => hash_of(("shell", shell)),
            None => page.key,
        };
        let mut slots: Vec<Slot> = Vec::new();
        for (at, unit) in units.iter().enumerate() {
            let key = unit_key(unit);
            let make_unit: Box<dyn Fn(f32) -> Box<dyn Widget> + '_> = match *unit {
                Unit::Page(index) => {
                    let page = &pages[index];
                    Box::new(move |entering| make(page, at > 0, entering))
                }
                Unit::Shell(shell) => {
                    let make_shell = &make_shell;
                    let stacks = &shells[&shell];
                    Box::new(move |_| make_shell(shell, stacks, true, at > 0))
                }
            };
            slots.push(Slot {
                key: (at, key),
                page: key,
                make: make_unit,
            });
        }
        // The pages kept for a return are kept in the tree, at the depth they will come back
        // at, so that what their components hold is still there. Those of a shell are kept in
        // the shell's own navigator.
        for (back, page) in kept
            .iter()
            .rev()
            .filter(|page| page.shell.is_none())
            .enumerate()
        {
            slots.push(Slot {
                key: (units.len() + back, page.key),
                page: page.key,
                make: Box::new(move |entering| make(page, true, entering)),
            });
        }
        let motion = match (gesture_in(None), slide_in(None)) {
            (Some(progress), _) if units.len() >= 2 => Motion::Gesture(progress),
            (_, Some((from, progress, forward))) => {
                let key = page_unit_key(&from);
                let at = match slots.iter().position(|slot| slot.page == key) {
                    Some(at) => Some(at),
                    // The page leaving is gone from the stack (a pop took it off the top): it
                    // is drawn as it was, a shell with the branches it had.
                    None => {
                        let make_gone: Option<MakeAt<'_>> = match from.shell {
                            Some((shell, _)) => slide
                                .as_ref()
                                .and_then(|(_, snapshot, ..)| (*snapshot).clone())
                                .map(|stacks| {
                                    let make_shell = &make_shell;
                                    Box::new(move |_| make_shell(shell, &stacks, false, true))
                                        as MakeAt<'_>
                                }),
                            None => Some(Box::new(move |entering| make(&from, true, entering))),
                        };
                        make_gone.map(|make_gone| {
                            slots.push(Slot {
                                key: (units.len(), key),
                                page: key,
                                make: make_gone,
                            });
                            slots.len() - 1
                        })
                    }
                };
                match at {
                    Some(at) => Motion::Slide {
                        from: at,
                        progress,
                        forward,
                    },
                    None => Motion::Still,
                }
            }
            _ => Motion::Still,
        };
        Box::new(assemble(slots, units.len(), motion))
    }
}

/// One page of a navigator being built: its key, the page (or shell) it stands for, and how
/// to build it, told how far in it is.
struct Slot<'a> {
    key: (usize, u64),
    page: u64,
    make: Box<dyn Fn(f32) -> Box<dyn Widget> + 'a>,
}

/// An entry of the outer stack: a page, or a shell standing for all of its pages.
enum Unit {
    Page(usize),
    Shell(u64),
}

/// What a navigator plays.
enum Motion {
    Still,
    /// A back gesture, `progress` of the way: the top page leaving for the one beneath.
    Gesture(f32),
    /// A transition, from the page at `from` to the one on top.
    Slide {
        from: usize,
        progress: f32,
        forward: bool,
    },
}

/// A navigator whose stack is the first `depth` slots — the last of them on top — playing
/// `motion`, with every other slot kept: built, and not drawn.
fn assemble(slots: Vec<Slot>, depth: usize, motion: Motion) -> Navigator {
    let top = depth - 1;
    let (shown, mut navigator) = match motion {
        Motion::Gesture(progress) if depth >= 2 => {
            let below = depth - 2;
            let navigator = Navigator::new(slots[below].key, (slots[below].make)(progress))
                .from(
                    slots[top].key,
                    (slots[top].make)(1.0 - progress),
                    progress,
                    false,
                )
                // The finger drives it: the pages follow linearly.
                .gesture(true);
            ([top, below], navigator)
        }
        Motion::Slide {
            from,
            progress,
            forward,
        } if from != top => {
            // The page leaving is either still in the stack (a push left it one entry down)
            // or gone from it (a pop took it off the top), kept or not.
            let navigator = Navigator::new(slots[top].key, (slots[top].make)(progress)).from(
                slots[from].key,
                (slots[from].make)(1.0 - progress),
                progress,
                forward,
            );
            ([top, from], navigator)
        }
        _ => (
            [top, top],
            Navigator::new(slots[top].key, (slots[top].make)(1.0)),
        ),
    };
    for (at, slot) in slots.iter().enumerate() {
        if !shown.contains(&at) {
            navigator = navigator.retain(slot.key, (slot.make)(1.0));
        }
    }
    navigator
}

/// The stack as a restorable value: a line per page, `g` for one a `go` made and `p` for one
/// pushed, then its location.
fn write_stack(stack: &Stack) -> String {
    stack
        .pages
        .iter()
        .map(|page| {
            format!(
                "{}{}",
                if page.pushed { 'p' } else { 'g' },
                page.state.location
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_stack(saved: &str) -> Vec<(bool, String)> {
    saved
        .lines()
        .filter_map(|line| {
            let mut chars = line.chars();
            let pushed = match chars.next()? {
                'p' => true,
                'g' => false,
                _ => return None,
            };
            Some((pushed, chars.as_str().to_string()))
        })
        .collect()
}

impl BuildContext<'_> {
    /// The application's router. Handlers capture it and navigate from there:
    ///
    /// ```ignore
    /// let router = cx.router();
    /// button("Open", move || router.go("/settings"))
    /// ```
    ///
    /// Panics if this application has no router — one made with `FrusApp::router`.
    pub fn router(&self) -> GoRouter {
        match self.service::<GoRouter>() {
            Some(router) => (*router).clone(),
            None => {
                panic!("there is no router: this application was not made with `FrusApp::router`")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Container;

    fn page(_: &BuildContext, _: &GoRouterState) -> Container {
        Container::new()
    }

    fn routes() -> Vec<GoRoute> {
        vec![
            GoRoute::new("/", page).routes(vec![
                GoRoute::new("users/:id", page)
                    .name("user")
                    .routes(vec![GoRoute::new("posts/:post", page).name("post")]),
                GoRoute::new("files/*", page).name("files"),
            ]),
            GoRoute::new("/settings", page).name("settings"),
        ]
    }

    fn router() -> GoRouter {
        GoRouter::new(routes()).animate(false)
    }

    fn keys(router: &GoRouter) -> Vec<String> {
        router
            .inner
            .stack
            .borrow()
            .pages
            .iter()
            .map(|p| p.state.path.clone())
            .collect()
    }

    #[test]
    fn it_starts_at_the_initial_location() {
        assert_eq!(router().location(), "/");
        assert_eq!(
            GoRouter::new(routes())
                .initial_location("/settings")
                .location(),
            "/settings"
        );
    }

    #[test]
    fn go_makes_the_stack_the_chain_of_routes_the_location_is_made_of() {
        let r = router();
        r.go("/users/42/posts/7");
        assert_eq!(
            keys(&r),
            ["/", "/users/:id", "/users/:id/posts/:post"],
            "home, the user, then the post: something to go back to at each step"
        );
        let state = r.state().unwrap();
        assert_eq!(state.param("id"), Some("42"));
        assert_eq!(state.param("post"), Some("7"));
        assert_eq!(state.path(), "/users/:id/posts/:post");
        assert_eq!(state.name(), Some("post"));
    }

    #[test]
    fn a_page_underneath_stands_for_its_own_location() {
        let r = router();
        r.go("/users/42/posts/7?x=1");
        let locations: Vec<String> = r
            .inner
            .stack
            .borrow()
            .pages
            .iter()
            .map(|p| p.state.location.clone())
            .collect();
        assert_eq!(
            locations,
            ["/", "/users/42", "/users/42/posts/7?x=1"],
            "each page is the part of the location it answered; the top one, all of it"
        );
        r.pop();
        assert_eq!(
            r.location(),
            "/users/42",
            "so going back lands where that page is"
        );
    }

    #[test]
    fn a_page_underneath_knows_the_parameters_too() {
        let r = router();
        r.go("/users/42/posts/7");
        let below = r.inner.stack.borrow().pages[1].state.clone();
        assert_eq!(below.param("id"), Some("42"));
        assert_eq!(below.path(), "/users/:id");
    }

    #[test]
    fn the_query_is_read_and_decoded() {
        let r = router();
        r.go("/users/1?tab=posts&q=a%20b+c&flag");
        let state = r.state().unwrap();
        assert_eq!(state.query("tab"), Some("posts"));
        assert_eq!(state.query("q"), Some("a b c"));
        assert_eq!(state.query("flag"), Some(""));
        assert_eq!(state.location(), "/users/1?tab=posts&q=a%20b+c&flag");
    }

    #[test]
    fn a_parameter_is_percent_decoded() {
        let r = router();
        r.go("/users/a%2Fb");
        assert_eq!(r.state().unwrap().param("id"), Some("a/b"));
    }

    #[test]
    fn a_wildcard_takes_what_is_left() {
        let r = router();
        r.go("/files/a/b/c.txt");
        assert_eq!(r.state().unwrap().param("*"), Some("a/b/c.txt"));
    }

    #[test]
    fn a_location_no_route_answers_to_is_an_error_page() {
        let r = router();
        r.go("/nowhere");
        assert_eq!(r.depth(), 1);
        assert!(r.inner.stack.borrow().pages[0].node.is_none());
        assert_eq!(r.location(), "/nowhere");
    }

    #[test]
    fn push_adds_a_page_and_pop_removes_it() {
        let r = router();
        r.push("/settings");
        assert_eq!((r.depth(), r.location()), (2, "/settings".to_string()));
        r.push("/settings");
        assert_eq!(r.depth(), 3, "the same page may be open twice");
        assert!(r.pop());
        assert!(r.pop());
        assert!(!r.pop(), "the last page stays");
        assert_eq!(r.location(), "/");
    }

    #[test]
    fn replace_swaps_the_top_page() {
        let r = router();
        r.push("/settings");
        r.replace("/users/3");
        assert_eq!(keys(&r), ["/", "/users/:id"]);
    }

    #[test]
    fn going_back_to_a_page_already_underneath_keeps_it() {
        let r = router();
        r.go("/users/1");
        let home = r.inner.stack.borrow().pages[0].key;
        r.go("/settings");
        r.go("/users/1");
        assert_eq!(
            r.inner.stack.borrow().pages[0].key,
            home,
            "the same place, the same page — and so the same state"
        );
    }

    #[test]
    fn a_named_route_is_reached_by_its_name() {
        let r = router();
        assert_eq!(
            r.location_for("post", &[("id", "4 2"), ("post", "9")], &[("x", "a&b")]),
            Some("/users/4%202/posts/9?x=a%26b".to_string())
        );
        assert_eq!(
            r.location_for("settings", &[], &[]),
            Some("/settings".to_string())
        );
        assert_eq!(
            r.location_for("user", &[], &[]),
            None,
            "a parameter is missing"
        );
        assert_eq!(r.location_for("nope", &[], &[]), None);
        r.go_named("user", &[("id", "5")], &[]);
        assert_eq!(r.location(), "/users/5");
        r.push_named("settings", &[], &[]);
        assert_eq!(r.location(), "/settings");
    }

    #[test]
    fn a_redirect_sends_the_navigation_elsewhere() {
        let signed_in = Rc::new(Cell::new(false));
        let gate = signed_in.clone();
        let r = GoRouter::new(vec![
            GoRoute::new("/", page),
            GoRoute::new("/login", page),
            GoRoute::new("/account", page),
        ])
        .animate(false)
        .redirect(move |state| {
            (state.location() == "/account" && !gate.get()).then(|| "/login".to_string())
        });
        r.go("/account");
        assert_eq!(r.location(), "/login");
        signed_in.set(true);
        r.go("/account");
        assert_eq!(r.location(), "/account");
    }

    #[test]
    fn a_route_can_redirect_for_itself() {
        let r = GoRouter::new(vec![
            GoRoute::new("/", page),
            GoRoute::new("/old", page).redirect(|_| Some("/new".to_string())),
            GoRoute::new("/new", page),
        ])
        .animate(false);
        r.go("/old");
        assert_eq!(r.location(), "/new");
    }

    #[test]
    fn a_redirect_loop_ends_in_the_error_page() {
        let r = GoRouter::new(vec![
            GoRoute::new("/", page),
            GoRoute::new("/a", page),
            GoRoute::new("/b", page),
        ])
        .animate(false)
        .redirect(|state| match state.location() {
            "/a" => Some("/b".to_string()),
            "/b" => Some("/a".to_string()),
            _ => None,
        });
        r.go("/a");
        assert!(r.inner.stack.borrow().pages[0].node.is_none());
    }

    #[test]
    fn a_refresh_listenable_runs_the_redirects_again() {
        use crate::ValueNotifier;
        let auth = ValueNotifier::new(true);
        let watched = auth.clone();
        let r = GoRouter::new(vec![
            GoRoute::new("/", page),
            GoRoute::new("/login", page),
            GoRoute::new("/home", page),
        ])
        .animate(false)
        .redirect(move |state| {
            let signed_in = watched.get();
            match (state.location(), signed_in) {
                ("/home", false) => Some("/login".to_string()),
                ("/login", true) => Some("/home".to_string()),
                _ => None,
            }
        })
        .refresh_listenable(&auth);
        r.go("/home");
        assert_eq!(r.location(), "/home");
        auth.set(false);
        assert_eq!(
            r.location(),
            "/login",
            "signing out moves the page with nobody navigating"
        );
    }

    #[test]
    fn extra_travels_with_the_navigation() {
        let r = router();
        r.go_with_extra("/settings", 41u32);
        assert_eq!(r.state().unwrap().extra::<u32>().as_deref(), Some(&41));
        assert!(r.state().unwrap().extra::<String>().is_none());
    }

    #[test]
    fn a_push_slides_in_and_a_pop_slides_out() {
        let r = GoRouter::new(routes());
        r.go("/settings");
        assert!(
            r.inner.stack.borrow().transition.is_some(),
            "a page arrives"
        );
        assert!(r.inner.stack.borrow().transition.as_ref().unwrap().forward);
        let mut frames = 0;
        while r.tick(1.0 / 60.0) {
            frames += 1;
            assert!(frames < 600, "the spring settles");
        }
        assert!(r.inner.stack.borrow().transition.is_none());

        r.push("/users/1");
        r.pop();
        assert!(
            !r.inner.stack.borrow().transition.as_ref().unwrap().forward,
            "going back"
        );
    }

    /// **A push lasts as long as the theme's platform's transition** (milestone 620): the
    /// router learns the transition when it is built, and runs the next one at a constant
    /// pace for that long — 500 ms for iOS's slide, 300 for the desktops' zoom.
    #[test]
    fn a_push_lasts_as_long_as_the_platform_s_transition() {
        let frames = |platform: frus_core::TargetPlatform| {
            // Animated, unlike the helper's router.
            let r = GoRouter::new(routes());
            let runtime = crate::Runtime::default();
            let theme = crate::Theme::default().with_platform(platform);
            let _ = r.build(&BuildContext::for_test(&runtime, &theme));
            r.push("/settings");
            let mut frames = 0;
            while r.tick(0.01) {
                frames += 1;
                assert!(frames < 1000);
            }
            frames
        };
        let slide = frames(frus_core::TargetPlatform::Ios);
        let zoom = frames(frus_core::TargetPlatform::Linux);
        assert!(
            (49..=51).contains(&slide),
            "half a second: {slide} frames of 10 ms"
        );
        assert!(
            (29..=31).contains(&zoom),
            "three tenths: {zoom} frames of 10 ms"
        );
    }

    /// **A back gesture's pages follow the finger**: the navigator it builds says so, so the
    /// transition is drawn linearly rather than on its curves.
    #[test]
    fn a_back_gesture_is_drawn_as_the_finger_moves() {
        let r = router();
        let runtime = crate::Runtime::default();
        let theme = crate::Theme::default();
        r.push("/settings");
        while r.tick(0.05) {}
        r.back_gesture(0.3);
        let tree = r.build(&BuildContext::for_test(&runtime, &theme));
        assert!(tree.navigator_gesture(), "under the finger");
        r.back_gesture_end(0.0);
        while r.tick(0.05) {}
        let tree = r.build(&BuildContext::for_test(&runtime, &theme));
        assert!(!tree.navigator_gesture(), "and not once it has let go");
    }

    /// **An iOS page let go follows the iOS rules** (`route.dart:851`, milestone 621):
    /// moving at a screen width a second or more, its direction decides — a flick back
    /// pops it even from near the start, a flick the other way keeps it even from near
    /// the end; slower, half the way decides. And it takes 350 ms either way.
    #[test]
    fn an_ios_page_let_go_follows_the_ios_rules() {
        let released = |progress: f32, velocity: f32| {
            let r = router();
            let runtime = crate::Runtime::default();
            let theme = crate::Theme::default().with_platform(frus_core::TargetPlatform::Ios);
            let _ = r.build(&BuildContext::for_test(&runtime, &theme));
            r.push("/settings");
            r.back_gesture(progress);
            r.back_gesture_end(velocity);
            let mut frames = 0;
            while r.tick(0.01) {
                frames += 1;
                assert!(frames < 1000);
            }
            (r.depth(), frames)
        };
        assert_eq!(
            released(0.2, 1.2).0,
            1,
            "a flick back pops it from near the start"
        );
        assert_eq!(released(0.8, -1.2).0, 2, "a flick the other way keeps it");
        assert_eq!(released(0.6, 0.5).0, 1, "slow, past halfway: it goes");
        assert_eq!(released(0.4, 0.9).0, 2, "slow, not halfway: it stays");
        let (_, frames) = released(0.6, 0.0);
        assert!(
            (34..=36).contains(&frames),
            "350 ms: {frames} frames of 10 ms"
        );
        let (_, frames) = released(0.1, 0.0);
        assert!(
            (34..=36).contains(&frames),
            "from wherever it was: {frames}"
        );
    }

    #[test]
    fn a_back_gesture_past_halfway_pops_and_a_short_one_does_not() {
        let r = router();
        r.push("/settings");
        assert!(r.can_go_back());
        r.back_gesture(0.2);
        r.back_gesture_end(0.0);
        while r.tick(1.0 / 60.0) {}
        assert_eq!(r.depth(), 2, "let go early: the page comes back");

        r.back_gesture(0.8);
        r.back_gesture_end(0.0);
        while r.tick(1.0 / 60.0) {}
        assert_eq!(r.depth(), 1, "carried far enough: it goes");
    }

    #[test]
    fn a_route_needs_a_slash_at_the_top_and_none_below() {
        let bad_top = std::panic::catch_unwind(|| GoRouter::new(vec![GoRoute::new("nope", page)]));
        assert!(bad_top.is_err());
        let bad_child = std::panic::catch_unwind(|| {
            GoRouter::new(vec![
                GoRoute::new("/", page).routes(vec![GoRoute::new("/x", page)])
            ])
        });
        assert!(bad_child.is_err());
    }
}

#[cfg(test)]
mod animated_tests {
    use super::*;
    use crate::Container;

    fn page(_: &BuildContext, _: &GoRouterState) -> Container {
        Container::new()
    }

    /// The same moves with the slide on — the default — must land where they land with it off.
    #[test]
    fn the_slide_does_not_change_where_the_stack_ends_up() {
        let router = GoRouter::new(vec![
            GoRoute::new("/", page).routes(vec![GoRoute::new("users/:id", page)]),
            GoRoute::new("/settings", page),
        ]);
        router.go("/users/42?tab=posts");
        assert_eq!(router.location(), "/users/42?tab=posts");
        assert!(router.can_pop());
        assert!(router.pop());
        assert_eq!(router.location(), "/");
    }
}

#[cfg(test)]
mod open_tests {
    use super::*;
    use crate::Container;

    fn page(_: &BuildContext, _: &GoRouterState) -> Container {
        Container::new()
    }

    fn router() -> GoRouter {
        GoRouter::new(vec![
            GoRoute::new("/", page).routes(vec![GoRoute::new("users/:id", page)]),
            GoRoute::new("/settings", page),
        ])
    }

    fn sliding(router: &GoRouter) -> bool {
        router.inner.stack.borrow().transition.is_some()
    }

    #[test]
    fn an_address_the_router_is_already_at_changes_nothing() {
        let r = router();
        r.open("/");
        assert_eq!((r.location(), r.depth()), ("/".to_string(), 1));
        assert!(!sliding(&r));
    }

    #[test]
    fn before_anything_is_built_the_router_starts_at_the_address_without_a_slide() {
        let r = router();
        r.open("/users/42");
        assert_eq!(r.location(), "/users/42");
        assert_eq!(r.depth(), 2, "home underneath, as with any `go`");
        assert!(
            !sliding(&r),
            "nobody has seen a page to slide away from: an address is where it opens"
        );
    }

    #[test]
    fn once_a_page_has_been_seen_an_address_slides_to_it_like_any_other_move() {
        let r = router();
        r.inner.built.set(true);
        r.open("/settings");
        assert_eq!(r.location(), "/settings");
        assert!(sliding(&r));
    }

    #[test]
    fn the_locations_of_the_stack_run_from_the_bottom_to_the_top() {
        let r = router();
        r.go("/users/42?tab=posts");
        assert_eq!(r.locations(), ["/", "/users/42?tab=posts"]);
    }

    #[test]
    fn only_a_replace_is_a_swap() {
        let r = router();
        assert!(!r.replaced());
        r.push("/settings");
        assert!(!r.replaced());
        r.replace("/users/1");
        assert!(r.replaced());
        r.go("/settings");
        assert!(!r.replaced(), "a go rebuilds the stack");
        r.push("/users/3");
        r.replace("/users/2");
        assert!(r.replaced());
        assert!(r.pop());
        assert!(!r.replaced(), "and a pop removes a page");
    }

    fn flat() -> GoRouter {
        GoRouter::new(vec![
            GoRoute::new("/", page),
            GoRoute::new("/category/:id", page),
            GoRoute::new("/widget/:id", page),
        ])
        .animate(false)
    }

    fn top_key(router: &GoRouter) -> u64 {
        router.inner.stack.borrow().pages.last().unwrap().key
    }

    /// The browser's Back to a page that was pushed: the page itself, still with what was
    /// beneath it, not one rebuilt from its location alone (milestone 751).
    #[test]
    fn an_address_that_is_a_pushed_page_underneath_goes_back_to_that_page() {
        let r = flat();
        r.push("/category/inputs");
        let category = top_key(&r);
        r.push("/widget/text_field");
        r.open("/category/inputs");
        assert_eq!(r.locations(), ["/", "/category/inputs"]);
        assert!(
            r.can_pop(),
            "home is still beneath: the page keeps its way back"
        );
        assert_eq!(
            top_key(&r),
            category,
            "the same page, and so the same state"
        );
    }

    /// A redirect still has its say about a page beneath.
    #[test]
    fn a_page_underneath_a_redirect_sends_elsewhere_is_not_returned_to() {
        let blocked = Rc::new(Cell::new(false));
        let r = {
            let blocked = blocked.clone();
            flat().redirect(move |state| {
                (blocked.get() && state.location().starts_with("/category"))
                    .then(|| "/".to_string())
            })
        };
        r.push("/category/inputs");
        r.push("/widget/text_field");
        blocked.set(true);
        r.open("/category/inputs");
        assert_eq!(r.locations(), ["/"]);
    }

    /// The browser's Forward to a page it went back from: kept, it is the same page.
    #[test]
    fn a_kept_page_comes_back_as_it_was_left() {
        let r = flat().keep_popped_pages(4);
        r.push("/category/inputs");
        r.push("/widget/text_field");
        let demo = top_key(&r);
        // Back twice, through the browser and through the app bar.
        r.open("/category/inputs");
        assert!(r.pop());
        assert_eq!(r.inner.stack.borrow().forward.len(), 2);
        // Forward twice.
        r.open("/category/inputs");
        r.open("/widget/text_field");
        assert_eq!(
            r.locations(),
            ["/", "/category/inputs", "/widget/text_field"]
        );
        assert_eq!(top_key(&r), demo, "the same page, and so the same state");
        assert!(r.inner.stack.borrow().forward.is_empty());

        // Forward past a page in one move, as a browser's history menu does.
        r.open("/");
        r.open("/widget/text_field");
        assert_eq!(top_key(&r), demo);
    }

    /// A move that is not back drops what was kept, as a browser drops the entries ahead.
    #[test]
    fn a_new_move_lets_the_kept_pages_go() {
        let r = flat().keep_popped_pages(4);
        r.push("/widget/text_field");
        let demo = top_key(&r);
        r.pop();
        r.push("/category/inputs");
        assert!(r.inner.stack.borrow().forward.is_empty());
        r.pop();
        r.open("/widget/text_field");
        assert_ne!(top_key(&r), demo, "a new page");
    }

    /// Without being asked to keep them, a router lets popped pages go: a phone's back
    /// button expects it.
    #[test]
    fn a_router_keeps_no_popped_page_unless_asked() {
        let r = flat();
        r.push("/widget/text_field");
        let demo = top_key(&r);
        r.pop();
        assert!(r.inner.stack.borrow().forward.is_empty());
        r.open("/widget/text_field");
        assert_ne!(top_key(&r), demo);
    }

    /// Only so many: the nearest are the ones kept.
    #[test]
    fn the_nearest_popped_pages_are_the_ones_kept() {
        let r = flat().keep_popped_pages(1);
        r.push("/category/inputs");
        r.push("/widget/text_field");
        let category = r.inner.stack.borrow().pages[1].key;
        r.open("/");
        let forward = r.inner.stack.borrow().forward.clone();
        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].key, category);
    }

    #[test]
    fn an_address_that_is_a_page_underneath_goes_back_to_it() {
        let r = router();
        r.go("/users/42");
        r.inner.built.set(true);
        r.open("/");
        assert_eq!((r.location(), r.depth()), ("/".to_string(), 1));
        let stack = r.inner.stack.borrow();
        assert!(
            !stack.transition.as_ref().unwrap().forward,
            "the browser's back button slides the page out, not in"
        );
    }
}

#[cfg(test)]
mod shell_tests {
    use super::*;
    use crate::Container;
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    fn page(_: &BuildContext, _: &GoRouterState) -> Container {
        Container::new()
    }

    /// Two tabs, each with pages under its first.
    fn tabs() -> GoRoute {
        GoRoute::shell(
            |_, _, body| body,
            vec![
                ShellBranch::new(vec![
                    GoRoute::new("/a", page).routes(vec![GoRoute::new("details", page)])
                ]),
                ShellBranch::new(vec![GoRoute::new("/b", page).routes(vec![
                    GoRoute::new("x", page),
                    GoRoute::new("item/:id", page),
                ])]),
            ],
        )
    }

    /// The tabs, and a page outside them.
    fn router_with(shell: GoRoute) -> GoRouter {
        GoRouter::new(vec![shell, GoRoute::new("/login", page)])
            .initial_location("/a")
            .animate(false)
    }

    fn router() -> GoRouter {
        router_with(tabs())
    }

    /// The key of the page on top. The router builds its first page lazily, so it is
    /// started first: a router nobody has asked anything of has no page yet.
    fn top_key(router: &GoRouter) -> u64 {
        router.start();
        router.inner.stack.borrow().pages.last().unwrap().key
    }

    #[test]
    fn each_tab_keeps_its_own_stack() {
        let r = router();
        assert_eq!(
            (r.location(), r.current_branch()),
            ("/a".to_string(), Some(0))
        );
        r.push("/a/details");
        let details = top_key(&r);
        assert_eq!(r.locations(), ["/a", "/a/details"]);

        r.go_branch(1);
        assert_eq!(r.locations(), ["/b"], "the second tab, from its start");
        assert_eq!(r.current_branch(), Some(1));
        r.push("/b/x");

        r.go_branch(0);
        assert_eq!(
            r.locations(),
            ["/a", "/a/details"],
            "the first tab as it was left"
        );
        assert_eq!(top_key(&r), details, "the same page, and so the same state");

        r.go_branch(1);
        assert_eq!(r.locations(), ["/b", "/b/x"]);
    }

    #[test]
    fn a_go_to_another_tab_keeps_the_first_one() {
        let r = router();
        r.go("/a/details");
        r.go("/b/x");
        assert_eq!(r.current_branch(), Some(1));
        assert_eq!(r.locations(), ["/b", "/b/x"]);
        r.go_branch(0);
        assert_eq!(r.locations(), ["/a", "/a/details"]);
    }

    /// Replacing a tab's only page keeps the new page in the tab: the page replaced was the
    /// shell's whole run, and the shell on show is read from it, not from what lies beneath.
    #[test]
    fn replacing_a_tabs_only_page_keeps_it_in_the_tab() {
        let r = router();
        r.go_branch(1);
        r.replace("/b/x");
        assert_eq!(
            (r.location(), r.current_branch()),
            ("/b/x".to_string(), Some(1))
        );
        assert!(r.pop(), "Back shows the first tab");
        assert_eq!(
            (r.location(), r.current_branch()),
            ("/a".to_string(), Some(0))
        );
    }

    #[test]
    fn choosing_the_tab_on_show_goes_back_to_its_first_page() {
        let r = router();
        let first = top_key(&r);
        r.push("/a/details");
        r.go_branch(0);
        assert_eq!(r.locations(), ["/a"]);
        assert_eq!(top_key(&r), first, "the first page, not a new one");
    }

    #[test]
    fn back_pops_the_tabs_own_pages_then_shows_the_first_tab_then_leaves() {
        let r = router();
        r.push("/a/details");
        r.go_branch(1);
        r.push("/b/x");

        assert!(r.can_pop());
        assert!(r.pop());
        assert_eq!(
            (r.location(), r.current_branch()),
            ("/b".to_string(), Some(1)),
            "the tab's own page first"
        );
        assert!(r.can_pop(), "the first tab is still to go back to");
        assert!(!r.can_go_back(), "but no gesture switches tabs");
        assert!(r.pop());
        assert_eq!(
            (r.location(), r.current_branch()),
            ("/a/details".to_string(), Some(0)),
            "then the first tab, as it was left"
        );
        assert!(r.pop());
        assert_eq!(r.location(), "/a");
        assert!(!r.can_pop(), "the first tab's first page: Back leaves");
        assert!(!r.pop());
    }

    #[test]
    fn a_shell_can_leave_from_any_tab() {
        let r = router_with(tabs().shell_back(ShellBack::Leave));
        r.go("/b");
        assert!(!r.can_pop(), "nothing to pop: Back leaves the application");
        assert!(!r.pop());
        assert_eq!(r.location(), "/b");
    }

    #[test]
    fn a_deep_link_selects_the_tab_and_builds_its_stack() {
        let r = router();
        r.open("/b/item/42");
        assert_eq!(r.current_branch(), Some(1));
        assert_eq!(r.locations(), ["/b", "/b/item/42"]);
        assert_eq!(r.state().unwrap().param("id"), Some("42"));
        r.go_branch(0);
        assert_eq!(r.locations(), ["/a"], "the other tab opens at its start");
        r.go_branch(1);
        assert_eq!(r.locations(), ["/b", "/b/item/42"]);
    }

    #[test]
    fn an_address_back_to_where_a_tab_was_left_finds_its_pages() {
        let r = router();
        r.push("/a/details");
        let details = top_key(&r);
        r.go_branch(1);
        // A browser's Back, to the address the first tab was left at.
        r.open("/a/details");
        assert_eq!(r.current_branch(), Some(0));
        assert_eq!(
            top_key(&r),
            details,
            "the pushed page itself, not one made anew"
        );
    }

    #[test]
    fn a_route_outside_the_shell_covers_it_and_pops_back_to_it() {
        let r = router();
        r.push("/a/details");
        r.push("/login");
        assert_eq!(r.locations(), ["/a", "/a/details", "/login"]);
        assert_eq!(r.current_branch(), None, "over the shell, not in it");
        assert!(r.pop());
        assert_eq!(
            (r.location(), r.current_branch()),
            ("/a/details".to_string(), Some(0))
        );
    }

    #[test]
    fn a_page_of_another_tab_pushed_shows_that_tab_and_lands_on_it() {
        let r = router();
        r.push("/a/details");
        r.push("/b/x");
        assert_eq!(r.current_branch(), Some(1));
        assert_eq!(r.locations(), ["/b", "/b/x"]);
        r.go_branch(0);
        assert_eq!(r.locations(), ["/a", "/a/details"]);
    }

    #[test]
    fn a_go_outside_the_shell_lets_its_tabs_go() {
        let r = router();
        r.push("/a/details");
        r.go_branch(1);
        r.go("/login");
        assert!(
            r.inner.shells.borrow().is_empty(),
            "the shell has left the stack"
        );
        r.go("/b");
        r.go_branch(0);
        assert_eq!(r.locations(), ["/a"], "a shell shown again starts afresh");
    }

    #[test]
    fn a_tab_whose_first_route_has_a_parameter_needs_a_start() {
        let made = std::panic::catch_unwind(|| {
            GoRouter::new(vec![GoRoute::shell(
                |_, _, body| body,
                vec![ShellBranch::new(vec![GoRoute::new("/u/:id", page)])],
            )])
        });
        assert!(made.is_err());
        let r = GoRouter::new(vec![GoRoute::shell(
            |_, _, body| body,
            vec![
                ShellBranch::new(vec![GoRoute::new("/a", page)]),
                ShellBranch::new(vec![GoRoute::new("/u/:id", page)]).initial_location("/u/me"),
            ],
        )])
        .initial_location("/a");
        r.go_branch(1);
        assert_eq!(r.location(), "/u/me");
    }

    #[test]
    fn a_redirect_has_its_say_on_a_tab_switch() {
        let r = GoRouter::new(vec![tabs(), GoRoute::new("/login", page)])
            .initial_location("/a")
            .animate(false)
            .redirect(|state| {
                state
                    .location()
                    .starts_with("/b")
                    .then(|| "/login".to_string())
            });
        r.go_branch(1);
        assert_eq!(r.location(), "/login");
    }

    /// A shell can sit under a route: its tabs' paths are relative, and Back from the first
    /// tab's first page goes to the page beneath the shell.
    #[test]
    fn a_shell_under_a_route_pops_out_to_it() {
        let r = GoRouter::new(vec![GoRoute::new("/", page).routes(vec![GoRoute::shell(
            |_, _, body| body,
            vec![
                ShellBranch::new(vec![GoRoute::new("a", page)]),
                ShellBranch::new(vec![GoRoute::new("b", page)]),
            ],
        )])])
        .animate(false);
        r.go("/b");
        assert_eq!(r.locations(), ["/", "/b"]);
        assert!(r.pop(), "Back shows the first tab");
        assert_eq!(r.locations(), ["/", "/a"]);
        assert!(r.can_go_back(), "and from there a gesture leaves the shell");
        assert!(r.pop());
        assert_eq!(r.locations(), ["/"]);
    }

    // ---- results ---------------------------------------------------------------------------

    fn results() -> GoRouter {
        GoRouter::new(vec![
            GoRoute::new("/", page),
            GoRoute::new("/pick", page),
            GoRoute::new("/other", page),
        ])
        .animate(false)
    }

    fn poll<T>(result: &mut PopResult<T>) -> Poll<Option<T>> {
        let mut cx = Context::from_waker(Waker::noop());
        std::pin::Pin::new(result).poll(&mut cx)
    }

    #[test]
    fn a_page_popped_with_a_value_hands_it_back() {
        let r = results();
        let mut picked = r.push_for_result::<u32>("/pick");
        assert!(poll(&mut picked).is_pending(), "the page is still there");
        assert!(r.pop_with(7u32));
        assert_eq!(poll(&mut picked), Poll::Ready(Some(7)));
        assert_eq!(r.location(), "/");
    }

    #[test]
    fn a_page_gone_any_other_way_hands_back_none() {
        let r = results();
        let mut by_back = r.push_for_result::<u32>("/pick");
        assert!(r.pop(), "the system's Back");
        assert_eq!(poll(&mut by_back), Poll::Ready(None));

        let mut by_gesture = r.push_for_result::<u32>("/pick");
        r.back_gesture(0.9);
        r.back_gesture_end(0.0);
        while r.tick(1.0 / 60.0) {}
        assert_eq!(r.depth(), 1);
        assert_eq!(poll(&mut by_gesture), Poll::Ready(None), "a back gesture");

        let mut by_go = r.push_for_result::<u32>("/pick");
        r.go("/other");
        assert_eq!(poll(&mut by_go), Poll::Ready(None), "a go elsewhere");

        let mut wrong = r.push_for_result::<u32>("/pick");
        r.pop_with("seven");
        assert_eq!(
            poll(&mut wrong),
            Poll::Ready(None),
            "a value of another type"
        );
    }

    #[test]
    fn a_value_is_for_the_page_it_pops_not_one_beneath() {
        let r = results();
        let mut picked = r.push_for_result::<u32>("/pick");
        r.push("/other");
        assert!(r.pop_with(1u32), "pops /other, which nobody waits on");
        assert!(poll(&mut picked).is_pending());
        assert!(r.pop_with(2u32));
        assert_eq!(poll(&mut picked), Poll::Ready(Some(2)));
    }

    #[test]
    fn a_callback_is_told_once() {
        let r = results();
        let told = Rc::new(RefCell::new(Vec::new()));
        let log = told.clone();
        r.push_then("/pick", move |value: Option<String>| {
            log.borrow_mut().push(value)
        });
        r.pop_with("blue".to_string());
        r.pop();
        assert_eq!(*told.borrow(), [Some("blue".to_string())]);
    }

    #[test]
    fn a_page_left_in_another_tab_has_not_been_popped() {
        let r = router();
        let mut picked = r.push_for_result::<u32>("/a/details");
        r.go_branch(1);
        assert!(poll(&mut picked).is_pending(), "still in its tab, waiting");
        r.go_branch(0);
        assert!(r.pop_with(3u32));
        assert_eq!(poll(&mut picked), Poll::Ready(Some(3)));
    }

    #[test]
    fn a_waiting_task_is_woken() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        struct Count(AtomicUsize);
        impl std::task::Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let r = results();
        let mut picked = r.push_for_result::<u32>("/pick");
        let mut cx = Context::from_waker(&waker);
        assert!(std::pin::Pin::new(&mut picked).poll(&mut cx).is_pending());
        r.pop_with(9u32);
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
    }

    // ---- observers -------------------------------------------------------------------------

    type Seen = Rc<RefCell<Vec<RouteChange>>>;

    fn watch(r: &GoRouter) -> (Seen, Subscription) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let subscription = r.observe(move |change| log.borrow_mut().push(change.clone()));
        (seen, subscription)
    }

    fn kinds(seen: &Seen) -> Vec<(RouteChangeKind, String, Option<usize>)> {
        seen.borrow()
            .iter()
            .map(|c| (c.kind, c.to.clone(), c.branch))
            .collect()
    }

    #[test]
    fn every_move_is_reported() {
        use RouteChangeKind::*;
        let r = router();
        r.location();
        let (seen, _watching) = watch(&r);
        r.push("/a/details");
        r.go_branch(1);
        r.replace("/b/x");
        r.pop(); // back to the first tab
        r.pop();
        r.go("/login");
        r.go("/login"); // the same page: nothing
        assert_eq!(
            kinds(&seen),
            [
                (Push, "/a/details".to_string(), Some(0)),
                (Branch, "/b".to_string(), Some(1)),
                (Replace, "/b/x".to_string(), Some(1)),
                (Branch, "/a/details".to_string(), Some(0)),
                (Pop, "/a".to_string(), Some(0)),
                (Go, "/login".to_string(), None),
            ]
        );
        assert_eq!(seen.borrow()[0].from.as_deref(), Some("/a"));
    }

    #[test]
    fn an_observer_stops_with_its_subscription_and_one_set_up_front_sees_the_first_page() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let r = results().observer(move |change| log.borrow_mut().push(change.clone()));
        r.push("/pick");
        assert_eq!(seen.borrow()[0].from, None, "the first page");
        assert_eq!(seen.borrow()[0].to, "/");
        assert_eq!(seen.borrow().len(), 2);

        let (later, watching) = watch(&r);
        drop(watching);
        r.pop();
        assert!(later.borrow().is_empty());
    }

    #[test]
    fn a_gestures_pop_is_reported() {
        let r = results();
        r.push("/pick");
        let (seen, _watching) = watch(&r);
        r.back_gesture(0.9);
        r.back_gesture_end(0.0);
        while r.tick(1.0 / 60.0) {}
        assert_eq!(
            kinds(&seen),
            [(RouteChangeKind::Pop, "/".to_string(), None)]
        );
    }

    // ---- building --------------------------------------------------------------------------

    /// The frame is told the tab on show; the outer navigator holds the shell alone, and the
    /// shell's own navigator every tab's pages: the one on top shown, the others kept.
    #[test]
    fn the_frame_wraps_a_navigator_holding_every_tab() {
        let told = Rc::new(Cell::new(usize::MAX));
        let seen = told.clone();
        let r = GoRouter::new(vec![GoRoute::shell(
            move |_, shell, body| {
                seen.set(shell.current_index());
                body
            },
            vec![
                ShellBranch::new(vec![
                    GoRoute::new("/a", page).routes(vec![GoRoute::new("details", page)])
                ]),
                ShellBranch::new(vec![GoRoute::new("/b", page)]),
            ],
        )])
        .initial_location("/a")
        .animate(false);
        let runtime = crate::Runtime::default();
        let theme = crate::Theme::default();
        r.push("/a/details");
        r.go_branch(1);
        let tree = r.build(&BuildContext::for_test(&runtime, &theme));
        assert_eq!(told.get(), 1);
        assert_eq!(tree.children().len(), 1, "the outer stack: the shell");
        assert_eq!(tree.navigator_retained(), 0);
        // Down to the shell's navigator: the page scope, then the frame (here the body).
        fn find_navigator(widget: &dyn Widget) -> Option<&dyn Widget> {
            if widget.navigator().is_some() {
                return Some(widget);
            }
            widget
                .children()
                .iter()
                .find_map(|child| find_navigator(child.as_ref()))
        }
        let inner = tree
            .children()
            .iter()
            .find_map(|child| find_navigator(child.as_ref()))
            .expect("the shell holds a navigator");
        assert_eq!(
            inner.children().len(),
            3,
            "/b shown, /a and /a/details kept"
        );
        assert_eq!(inner.navigator_retained(), 2);
    }
}
