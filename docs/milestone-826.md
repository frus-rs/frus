# Milestone 826 — Translations and routes the compiler checks

## Objective

Issue #223: the two things an application names most often were plain strings, checked only at
run time.

- `frus-l10n` looked messages up by key: a typo in a key, a message a translator left out, an
  argument renamed in one file showed on the screen, in one language, for whoever happened to
  look.
- `GoRouter` matched string patterns: `go("/user/42/setings")` compiled, and a route's
  parameters came back as text to parse.

Rust has procedural macros, so both can be checked when the application compiles, with no
generator to re-run and no generated file to keep in sync. The issue's "done when":

1. the demo uses typed messages and typed routes;
2. removing a key from one `.ftl` file breaks the build with a message naming the key and the
   locale;
3. a test shows that every existing string location still resolves.

All three hold; see "How it was tested".

## Typed messages: `messages!`

```rust
mod words {
    frus_l10n::messages!("../i18n", default = "en", partial = ["de"]);
}
let fr = words::messages("fr");
fr.task_count(3);   // task-count = { $n -> [one] ... *[other] ... }
fr.hello("Ada");    // hello = Bonjour, { $name } !
```

The macro lives in `frus-macros` (where milestone 823 put `include_font!`) and is reached
through a `macro_rules!` in `frus-l10n` that hands it `$crate`, so the generated code names the
runtime crate correctly whether the application depends on `frus-l10n` or on `frus` with the new
`l10n` feature (`frus::l10n::messages!`).

It reads every `<locale>.ftl` of the folder (relative to the file that writes the macro, as
`include_str!` is) with `fluent-syntax`, the parser `fluent-bundle` itself uses, so the macro and
the runtime cannot disagree about what a file says. It writes, where it is called:

- `messages(locale) -> Messages`, negotiated like `Localizer::set_locale` (exact, then the same
  language, then the default). That negotiation is now `Localizer::negotiate`, which `set_locale`
  calls. It also sorts the candidates before taking one, where the old code took whichever the
  hash map listed first.
- `Messages`, one method per message, named after it (`task-count` is `task_count`, a keyword
  such as `type` is `r#type`). Two keys that would make one name are a compile error.
- `localizer()`, a `&'static Localizer` holding every file (each embedded with `include_str!`,
  which is also what makes cargo rebuild when a file changes): the string API keeps working.
- `LOCALES`, `DEFAULT_LOCALE`.

**Argument types.** Fluent arguments are text or numbers, and the files say which only by how
they use them. An argument is a number (`impl frus_l10n::Number`, every integer and float type)
when, in any locale, it selects between variants keyed by a number or a CLDR plural category
other than `other`, or is passed to `NUMBER()`. Any other argument is a `&str`. A message that
refers to another (`{ hello }`) takes that message's arguments too, as Fluent passes them on.
The arguments come in the order the default locale's message first uses them.

**The fallback rule** (the issue asked for one to be stated):

- the default locale (`default = "..."`, `en` when not given) defines the messages and their
  arguments;
- every other locale has every message of the default locale, unless it is listed in
  `partial = [..]`; a message a partial locale lacks is shown in the default locale, which is
  what `Localizer` already did at run time;
- no locale, partial or not, has a message the default locale lacks;
- a translation uses no argument the default locale's message does not give it;
- a Fluent syntax error is a compile error, with the file and the line.

Each broken rule is reported, all of them at once, naming the message, the locale and its file:

```text
error: messages!: the message `add` is missing from the locale `fr` (fr.ftl); every locale has
       every message of the default locale `en`, unless it is listed in `partial = [..]`
error: messages!: the message `hello` in the locale `fr` (fr.ftl) uses the argument `$nom`, which
       the default locale `en` does not give it; did you mean `$name`?
```

### The alternatives

- **A build script generating a module.** It works, but every application would write the same
  `build.rs`, and the output lives in `OUT_DIR` where nobody reads it. A macro call is one line in
  the module that uses it.
- **A derive on a struct naming the folder.** The same thing with more ceremony; the issue
  suggested either, and a function-like macro is the one that needs nothing else.
- **The union of every locale's arguments.** It would turn a misnamed argument into a silent
  extra parameter, which is the bug the issue is about. The default locale decides.
- **Every argument as `impl Into<Arg>`.** It lets a number reach a text argument and the other
  way round; the plural rules then quietly pick `other`. The usage-based types catch that.

## Typed routes: `#[derive(Routes)]`

```rust
#[derive(Routes, Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    #[route("/")]
    Home,
    #[route("/task/:id")]
    Task { id: u64 },
    #[route("/search")]
    Search { q: Option<String> },
}
let router = GoRouter::new(Route::go_routes(|_, state, route| match route { ... }));
router.go(Route::Task { id: 3 }.location());
```

The derive is in `frus-macros` and re-exported beside the new `Routes` trait in `frus-widgets`
(so `frus::Routes` names both: the trait and its derive). It writes `PATTERNS`, `pattern()`,
`path_params()`, `query_params()` and a hidden `from_parts`; the trait provides `location()`
(encoded the way `GoRouter::location_for` already encoded), `from_location()` and `go_routes()`.

- A `:parameter` is the field of that name (`FromStr` to read it, `Display` to write it). Any
  other field is a query parameter and must be an `Option`: a location may always leave a query
  out, so a required one would be a route a deep link could not reach.
- Compile errors: a pattern not starting with `/`, with `?`, `#`, `*`, an empty segment or a
  trailing `/`; a parameter with no field; a parameter named twice; a non-`Option` query field; a
  tuple variant; a variant without `#[route]`; two patterns answering the same locations
  (`/a/:x`, `/a/:y`); generics.
- **`go_routes`** builds the `GoRoute` tree from the patterns: each goes under the longest other
  pattern that is a prefix of it, so `/task/:id` is under `/` and Back from it goes home, as the
  hand-written tree did. One builder receives the parsed route: a variant with no page is a
  missing `match` arm.
- **A path parameter that does not parse** (`/task/three` for a `u64`) must not reach a builder
  that has nothing to build from. A route made by `go_routes` carries an `accept` check, which
  the router's matcher now asks after a pattern consumed its segments; a refusal makes the
  location match no route, so the router shows its error page, exactly as for a location that
  does not exist. Routes written by hand have no check and behave as before.
- **String locations keep working**: the router still matches strings, so deep links, the web
  address bar and `go("/task/3")` are unchanged. `GoRouterState::route::<R>()` reads a page's
  route from its state.

Which crate the generated `impl` names: a derive cannot be handed `$crate`. It reads the calling
package's manifest: `::frus_widgets` for `frus-widgets` itself or a package depending on it (the
demo), `::frus` otherwise (an application). `#[routes(crate = "...")]` overrides.

### The alternatives

- **Route constants (`const TASK: &str = "/task/:id"`)**: they fix typos in the pattern, not
  in the parameters, and parse nothing.
- **A typed builder per route (`GoRoute::typed::<R>(...)`)** beside hand-written nesting: it
  repeats the pattern's prefix in two places. Nesting by pattern prefix gives the tree the demo
  already had, from the enum alone.
- **Required query fields.** Refused at compile time instead: see above.

## The demo

- `src/l10n.rs`: `messages!("../i18n", default = "en")` replaces the hand-loaded `Localizer` and
  the `tr`/`tr_n` string lookups; the screens call `text_in(lang).task_count(total)`.
- `src/screens/mod.rs`: `enum Route` with the 28 routes (home, 26 screens, `/task/:id`) and
  `Route::go_routes`, replacing the hand-written tree. Every `push("/settings")`,
  `go(format!("/task/{id}"))`, menu entry and drawer link now names a variant.
- The `shots` tool and the tests still say where to go with strings, which is the point of the
  test below.

## How it was tested

- `frus-macros` unit tests: the analysis of a set of files (a complete set, a missing message, a
  partial locale, an extra message, a misnamed argument with its hint, a syntax error with its
  line, references and `NUMBER()`, name clashes, a missing default file), the pattern checks and
  the manifest detection.
- `frus-l10n/tests/messages.rs`: the macro on a folder of three locales, one partial: typed
  calls, plurals, a keyword key, a reference's arguments, negotiation, the partial fallback, and
  the string API beside it.
- `frus-l10n/tests/compile_fail.rs` (trybuild): `tests/ui/missing_key.rs` (en.ftl has `add`,
  fr.ftl does not) and `tests/ui/misnamed_argument.rs` (`$nom` for `$name`) fail to compile with
  the `.stderr` beside them, which names the key, the locale and the file. This is "done when" 2.
- The `Routes` doctest in `router.rs` and the guide's example: locations, parsing, a refused
  parameter, the nested tree, a string location read back as a route.
- `frus-demo`'s `every_string_location_still_resolves`: each of the 28 string locations the demo
  was reached at before (the deep link's `/task/3` included) opens the route it opened before,
  with home beneath it, reads back as a typed route, and is the location that route writes; every
  route of the enum is reached by one of them; `/task/three` reaches the error page. This is
  "done when" 3. The demo's existing tests, which navigate by string, pass unchanged.
- The full check on rustc 1.100: `cargo fmt --check`, clippy `-D warnings` on the host, Android
  (`aarch64-linux-android`), the web (`wasm32-unknown-unknown`) and `frus` with `l10n`,
  `cargo doc` with `-D warnings`, and `cargo test --workspace` (frus-gpu and frus-test left out,
  as usual), all green. The eight example projects (tests compiled) and the gallery for the web
  build against the branch.

## Not done

- **`scripts/gen_licenses.py` as a build step.** The script reads `cargo tree` and
  `cargo metadata` for the whole dependency graph; a build script would have to run cargo inside
  cargo (a second resolver run, the lock on the target directory, no network in offline builds)
  for every build of the application. That is not small, so it is left: the licence list is still
  regenerated by hand.
- A **new `.ftl` file** added to the folder is only seen when the crate compiles again for
  another reason: cargo tracks the files the macro embeds, not the folder (tracking a directory
  needs an unstable compiler API). Changing or removing a key in an existing file is seen.
- Message **attributes** (`login.placeholder`) and terms get no function of their own; they stay
  reachable through `localizer()`.
- Typed routes have no `*` (splat) segment and no shell routes: `go_routes` makes plain nested
  routes. An application with a shell writes those routes by hand, and can still use the enum for
  `location()` and `from_location()`.
- Arguments are positional: two text arguments of one message can be swapped without an error.
