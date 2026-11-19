# frus-macros

The compile-time macros of frus. Today there is one, `include_font!`: it embeds a font file the
way `include_bytes!` does, and lets `frus build` put in a copy cut down to the characters the
application can show, which is most of a megabyte less per face.

**Layer:** Foundations. A procedural macro with no dependencies.

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

Applications do not name this crate: the `frus` facade re-exports the macro as
`frus::fonts::include_font!`. See the [text guide](https://github.com/frus-rs/frus/blob/master/docs/guide/text.md).
