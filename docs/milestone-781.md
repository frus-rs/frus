# Milestone 781 — Navigation for real apps: tabs with their own stacks, answers, observers

## Objective

The router of milestones 551–563 kept **one** stack of pages. Real applications are built around a
bottom bar whose tabs each keep their own history: open a post in the feed, look something up in
the search tab, come back to the feed and find the post still open, scrolled where it was. They
also push a screen to *ask* something — a colour, a contact — and need the answer back, and they
log every screen for analytics. None of the three could be written with the router as it was.

## What was done

- **Shell routes.** `GoRoute::shell(builder, branches)` is a frame around `ShellBranch`es. The
  builder gets a `NavigationShell` (`current_index()`, `branch_count()`, `go_branch(i)`) and the
  body — the branch's pages — and puts them in a scaffold with a bottom bar, a rail, anything.
  - Each branch keeps its own stack. `go_branch(i)` shows a branch as it was left; choosing the
    branch on show returns to its first page. Branches not on show stay in the tree (built, not
    drawn), so what their components hold survives a tab switch.
  - A page pushed from a branch goes onto that branch, under the frame: its transition plays
    inside the body and the bar does not move. A page of a route outside the shell covers the
    whole frame. A page of another branch pushed from this one shows that branch and lands on it.
    Tabs switch at once, with no transition.
  - Back arbitration: the system's Back pops the branch's own pages first; on the first page of a
    branch that is not the first, it shows the first branch as it was left (`ShellBack::FirstBranch`,
    the default) or leaves (`ShellBack::Leave`); then the pages beneath the shell, then the
    application. The edge swipe (milestone 621) only ever takes a page off — it never switches
    tabs — so `Application` gained `can_handle_back`, which the shell asks for the system's Back
    while `can_go_back` still gates the gesture.
  - Deep links: a location answered by a branch's route selects that branch and builds its stack
    (`/feed/post/7` → the feed tab, with the post on top). A location that is where another branch
    was left — the browser's Back after a tab switch — finds that branch's stack as it was, its
    pushed pages included. Redirects run on tab switches like on any navigation.
  - Routes of a branch are written as anywhere else; a shell may sit under a route (its branches'
    paths are then relative). A branch whose first route has a parameter must say where it starts
    (`ShellBranch::initial_location`), which is checked when the router is made.
- **Typed pop results.** `router.push_for_result::<T>(location)` returns a `PopResult<T>` future
  that resolves to `Some(value)` when the page is popped with `router.pop_with(value)`, and to
  `None` when it goes any other way: the system's Back, a back gesture, the app bar's arrow, a
  `go` elsewhere, or a value of another type (logged as a warning). `push_then(location, |v| ..)`
  is the closure form. A page left in a branch that is not on show has not been popped: the answer
  waits. The future is awaited in a component's `async_callback` (milestone 714); it wakes the task
  when the page goes.
- **Observers.** `router.observe(|change| ..)` returns a `Subscription`; `.observer(..)` on a
  router being made keeps one for its whole life and is also told of the first page. A
  `RouteChange` has `kind` (`Go`, `Push`, `Pop`, `Replace`, `Branch`), `from`, `to` and `branch`.
  A move that leaves the same page on top is not reported.
- `Navigator::expand()`: a navigator that takes its parent's room instead of the window's size —
  what the shell's body is.
- The navigation guide has three new sections: tabs that keep their pages, a screen that answers,
  watching the screens.

## Design

**One stack, with runs.** The router keeps a single stack of pages, as before; a page shown inside
a shell carries the shell and branch it belongs to, and a shell's pages are always one contiguous
run of one branch. The other branches' stacks are kept beside it. Everything that already worked
on the stack — the back gesture, the transitions, the browser's history, restoration, kept pages
for the browser's Forward — keeps working on it unchanged, and the back button's arbitration
falls out of it: the innermost stack's top *is* the stack's top. The alternative, a router per
branch, would have duplicated redirects and history and made `cx.router()` ambiguous.

Building is where the run matters: the outer navigator gets one entry for the whole shell, and
the shell's frame gets a navigator of its own holding every branch's pages (the run on show, the
others retained). Each motion is played by the navigator that holds both of its pages — inside
the shell when both are in the branch, outside when one of them is not, and not at all between
two branches. Every change of the stack goes through one function (`install`), which keeps the
branches, settles the results of pages that left (gone from the stack *and* from every branch),
and tells the observers — observers first, so a move a result's handler makes is reported after
the move that caused it.

**A future, with a closure beside it.** Components already await work in `async_callback` tasks,
so a pushed screen's answer is another thing to await; the closure form serves code that is not
in a task. The value travels as `Box<dyn Any>` and is downcast at the end, so the screen that
answers needs no type parameter.

## Verification

- `frus-widgets` router tests: 26 new (`shell_tests`) — a stack per tab, `go` to another tab
  keeping the first, re-selecting a tab, back arbitration (branch pages, first tab, leave; the
  gesture refusing to switch tabs), `ShellBack::Leave`, a deep link into a nested route with a
  parameter, an address back to where a tab was left finding its pushed page, a route outside the
  shell covering it, a push into another tab, a shell leaving the stack, a branch's start,
  redirects on a tab switch, a shell under a route; typed results (value, Back, gesture, `go`,
  wrong type, a value for a page above, the callback told once, a page left in another tab
  waiting, the task woken); observers (every kind, unsubscribing, the first page, a gesture's
  pop); and the built tree (the outer navigator holding the shell alone, the shell's holding every
  tab). The existing router tests pass unchanged.
- Through `Application::view` (`frus-shell`, `shell_route_tests`): a real `Scaffold` with a
  `BottomNavigationBar`, driven by taps on its labels — a hook's count survives a tab switch, a
  branch page opens above the bar, and the system's Back unwinds the tab, then shows the first
  tab, then leaves. And the gesture and the system's Back answering differently on a tab's first
  page.
- Not run in a real window on any platform: no demo page uses a shell yet (see below).

## Not done

- **Not run for real.** Nothing here was run in a window, on a phone or in a browser. Everything
  above is unit tests and `Application::view` tests driven by synthetic taps and Back events. No
  demo page or example uses `GoRoute::shell` yet. The browser's Back and Forward across tab
  switches are tested only against the history state machine; no headless browser checked them.
- **Tab switches have no transition.** Branches swap at once. A fade between tabs is not offered.
- **No nested shells.** A shell inside a branch of another shell is refused when the router is
  made (an assertion, so a panic at start-up, not an error value).
- **Restoration** saves only the stack on show. After the process is restored, the branches not
  on show start again from their first page.
