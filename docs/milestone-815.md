# Milestone 815 — A conversation, and a questionnaire one step at a time

## Objective

Two sets of widgets that applications keep writing for themselves:

- **A conversation**: `ChatList`, `MessageBubble`, `TypingIndicator` and `ChatComposer`, over
  the application's `ChatMessage`s. The list rests at the newest message and follows the
  conversation as messages arrive, unless the reader scrolled up, in which case they keep their
  place and a button offers to jump back, with how many came in. Bubbles are grouped by sender
  and time, with an avatar, a name, a time, a status (sending, sent, delivered, read, failed with
  a retry), selectable words and a slot for an attachment. Dates go between days, three dots
  say someone is typing, and the composer grows with what is typed, sends only something, and
  sends on Enter on a desktop (Shift+Enter breaks the line).
- **A questionnaire**: `StepForm`, `FormStep` and `StepFormState`. Steps with a title and a
  subtitle, a progress (the numbered steps, a bar or dots), Back and Next, validation that
  refuses Next and shows why, optional steps that can be skipped, a review step, the answers
  kept across the steps, a completion message, and the steps sliding along the reading
  direction over the theme's motion durations.

Both are themable (`theme.widgets.chat`, `theme.widgets.step_form`) and each word they say can
be changed (`ChatStrings`, `StepFormStrings`); both work right to left and in both brightnesses
and say what they are to a screen reader.

## Design

### The conversation's list is a reversed, virtualised list

`ChatList` is a `ListView::builder` built from the far end: index 0 is the newest message, at
the bottom, and the list rests at offset 0 there. A message arriving is a new index 0, so a
reader following the conversation sees it come in without anything being asked of the list.
Only the rows in view are built, each from its message and its two neighbours (whether it starts
a day, starts a run, ends a run): ten thousand messages build the dozen on the screen (tested).
The application gives the messages oldest first, as it appends them, in an `Rc<Vec<_>>`, so a
rebuild copies nothing.

### Keeping the reader's place: `ListView::hold_position`

A reader who scrolled up is not at offset 0, and a new index 0 pushed every row they were
reading up by its own height. This is a property of the list, not of a conversation, so it is a
new option there: `ListView::hold_position(true)`. The list remembers the first keyed row it
showed and where it started; on the next frame it asks `find_child_index` where that row is now.
When it moved by `k` places, `k` unknown extents are put at the start of what the list knows of
its rows (so the measured heights stay with their rows), the `k` new rows are **built and
measured at once** however far from the window they are (at most 64 per frame), and the offset
moves by what moved the row — through the same correction the list already uses to keep a
reader's place when a row above the window changes height (milestone 717). Resting at the start,
nothing moves: what arrives is what the reader is waiting for. Rows removed before the view move
it back by what they were counted at.

### Where "scrolled up" lives

In the application, as a `ChatListState`, like the palette's and the panels' states (milestone
809). The list reports through `on_scroll`, but only when "away from the newest" changes — a
crate-private filter on `ListView` lets a report decline to be one — so a scroll does not
rebuild the view on every frame. Away, it records which message was the newest; the jump
button counts the others' messages after it ("3 new").

The jump is `ChatListState::jump_to_newest`, which the button sends and which an application
also sets after the reader sends a message. It carries a generation: a list built with a new
one asks the runtime (`Runtime::list_to_start`) to glide back to its start at the next frame,
which a finger on the list refuses as it refuses any request. The reports on the way back are
not mistaken for the reader leaving again.

### Bubbles

A `MessageBubble` is assembled from existing widgets under the theme of the place it lands in
(a `Lazy` over a `ThemeBuilder`, as the milestone 786 composites are): a `Container` with
corners named by the reading direction (`BorderRadiusDirectional`, tighter on the sender's side
between two bubbles of a run), a selectable `Text`, the attachment above it, the name over the
first of a run, the avatar beside the last (the author's initials unless the application gives
one), the time and the status under the last, and "Not sent" with a retry button under a failed
one. A bubble reads as one list item — who, when, where it has got to, and what it says —
except a failed one, whose retry button stays a button of its own. The words the label already
says (the text, the name, the time, "Not sent", the initials) are kept out of the tree with
`ExcludeSemantics`, so they are not read a second time; an attachment or an avatar the
application hands in keeps its own voice, since it may be operable.

The `Semantics` node wraps the bubble, not its row: `Semantics` is a transparent box that hugs
its child, and wrapped round the row it left the row as narrow as its bubble, which put the
reader's bubbles at the start side instead of the end, in both directions. The test that should have caught it
compared the reader's bubble with someone else's only, which a bubble 12 px further in passed;
it now checks the half of the screen too.

### The composer, and Shift+Enter

`ChatComposer` is a `TextField` that grows from one line to six, a send button live only when
the draft has something other than spaces, and a slot at the start. On a desktop Enter sends
and Shift+Enter breaks the line; on a phone Enter breaks the line and the button sends.

A field could not tell the two keys apart: `Key::Enter` carried no modifier. `Key` has a new
variant, `LineBreak`, which the desktop shell sends for Shift+Enter; a multi-line field always
breaks the line on it, a single-line one takes it as Enter. `TextField::submit_on_enter` makes a
multi-line field submit on Enter.

### The questionnaire reuses `Steps`, `Form` and `AnimatedSwitcher`

The state is the application's (`StepFormState`): the step showing, every answer under a key,
the steps done and skipped, the errors of the step the reader last tried to leave, and which
way the last move went. Validation is the existing `Form` and `Rule`s, built from the answers
(`FormStep::validate`), and a check of the step as a whole (`FormStep::check`). What Next does is
decided as the form is built, from the answers as they stand: refused with the errors — listed
over the step in an `ErrorSummary` and handed to each field by `StepFormState::error` — or on.

The progress reuses `Steps`, the existing indicator, rather than a second one. It was extended:
skipped steps (a dash), a refused step (an exclamation mark in the error colour), markers that
can be gone back to while the others are inert (`on_tap_where`), and two fixes found on the way
— **it drew its markers left to right in a right-to-left layout**, where the row of hotspots
over them is mirrored, so a press went to the wrong step; and it said nothing to a screen reader
(it reads as a progress now). Under 600 px the progress is a bar (`StepProgress::Auto`), and the
form is never wider than 720 px, centred.

The steps slide with `AnimatedSwitcher::patterned(SharedAxis::X)`, which takes its durations from
the theme's motion tokens and mirrors right to left; a move back plays it backwards.

## How it was tested

Unit tests, through the existing harness (`build_ui` frames, scene primitives, semantics, hits
and their messages, the runtime's scroll offsets):

- `ListView::hold_position`: a row of 70 arriving at the start of a reversed list scrolled
  away from it leaves every row in view where it was on the screen, in that frame and the next
  (counted at the guess of 40 they would all move by 30); at the start nothing moves; a list
  not asked does not hold; rows removed before the view move it back. The offset itself moves
  by a little more than 70: the rows between the start and the view were never measured, are
  counted at the mean of those that were, and the new row raises that mean. The frame that
measures the arrival places its rows with that new mean too; it used the old one, and the
rows in view moved by the difference for every unbuilt row before them, for one frame.
- The conversation end to end: newest at the bottom, days named ("Today", "Yesterday", the date
  with its year when it is another year's), one name and one time per run, runs split by sender
  and by the window, the reader's bubbles at the end side and mirrored right to left, colours
  from the caller, then the theme, then the scheme, a retry sending its message's id, what a
  bubble reads as, 10 000 messages building fewer than 60 rows, the typing row and its label,
  the dots' phases, the "away" reports (only when it changes), the jump not mistaken for
  leaving, the "1 new" button and its message, a new jump generation turning into a glide, and a
  message arriving through a real `ChatList` moving the offset when scrolled up and not at the
  bottom.
- The composer: send only with something to send; Enter sends and Shift+Enter breaks the line in
  a field that submits on Enter; a plain multi-line field still breaks the line; a single-line
  field takes Shift+Enter as Enter; the composer sends on Enter on a desktop theme and not on a
  phone's.
- The questionnaire: an invalid step refuses Next with its errors, which are shown and cleared by
  a new answer; a valid one goes on and is marked done with its answers; an optional step offers
  Skip and Back; a step-wide check refuses; the review shows the answers and Submit completes
  with all of them; without a review the last step completes; the progress follows the width;
  the form is centred in a wide window; the title is a heading; done steps can be revisited from
  the progress and the ones ahead cannot.
- `Steps`: the first marker at the start in either direction, skipped and refused marks, its
  semantics, inert markers out of the tab order.

The demo has two new pages, "Conversation" (three hundred messages, a reply that types back, a
burst of five messages to scroll up against, a failed message to retry) and "Questionnaire".

The golden images and the size of a minimal app were **not** run locally: both run in WSL, which
could not start while the system drive was full. CI checks the size budget.

## Not done

- **Not checked on a device or in a window.** Everything above is unit-tested; the demo pages were
  built but not run on the phone, a desktop window or a browser, so the feel of the anchoring
  during a fling, the glide back and the slide between steps are unverified.
- Items arriving before the rows in view are assumed to arrive **at the start** of the list. A
  message inserted in the middle of the history (an edit that reorders, a late message with an
  older time) is placed correctly but the measured heights between it and the start shift by one
  row until those rows are seen again. At most 64 arriving rows are measured in one frame; more
  are counted at the mean height.
- Rows removed before the view move it back by what they were counted at, which is a guess for
  rows never seen.
- If the reader catches the list during the glide back to the newest message, the jump button
  stays hidden until they reach the bottom once.
- On the web, the composer's field is edited through the browser's element: Enter there inserts
  a line break as it would in any multi-line field; the send button sends. Shift+Enter is only
  told apart in the desktop shell.
- "Today" and "Yesterday" are reckoned from the system clock's day in UTC unless the application
  says which day is today (`ChatList::today`), and times are `HH:MM` unless it formats them
  (`ChatList::format_time`); the words are English unless it gives `ChatStrings` — they are not
  in the localizations tables yet.
- The jump button and a step change are not announced through a live region; the new step's
  title is a heading the reader finds.
- No read receipts per participant, reactions, replies to a message, or swipe-to-reply.
- No golden images: these widgets are assembled from widgets the goldens already cover.
