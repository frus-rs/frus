# {{project-name}}

Widgets for [frus](https://github.com/frus-rs/frus) applications.

```sh
frus test                        # the tests, in tests/
cargo run --example gallery      # the widgets in a window
```

An application uses the package by adding it:

```toml
[dependencies]
{{project-name}} = { path = "../{{project-name}}" }   # or a version, once published
```

```rust
use {{crate_name}}::Tag;

column![Tag::new("Inbox").count(3).into_widget()]
```

Every visual of a widget comes from the application's theme unless it is given:
`Tag::new("Inbox").color(..).text_color(..).radius(4.0)`.

Publishing: `cargo publish` (fill in `repository` in `Cargo.toml` first).
