# frus-macros

The compile-time macros of frus:

- `include_font!` embeds a font file the way `include_bytes!` does, and lets `frus build` put in
  a copy cut down to the characters the application can show, which is most of a megabyte less
  per face.
- `messages!` reads an application's Fluent files when it compiles and writes one function per
  message, its arguments typed. A message missing from one language, or a translation using an
  argument the default language does not give, is a compile error naming the message and the
  language.
- `#[derive(Routes)]` makes an enum the application's routes: their patterns, the location of a
  route and the route of a location, its parameters parsed into the variant's fields.

**Layer:** Foundations. A procedural macro crate.

## Example

```rust,ignore
frus::main!({
    frus::fonts::add_font(frus::fonts::include_font!("../assets/fonts/Inter-Regular.ttf").to_vec());
    frus::fonts::set_default_family("Inter");
    app()
});
```

Any build other than `frus build` in release embeds the file as it is.

## Part of frus

Applications do not name this crate: the `frus` facade re-exports the macros as
`frus::fonts::include_font!`, `frus::Routes` and `frus::l10n::messages!` (feature `l10n`; also
`frus_l10n::messages!`). See the [text guide](https://github.com/frus-rs/frus/blob/master/docs/guide/text.md).
