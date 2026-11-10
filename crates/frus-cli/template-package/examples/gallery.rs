//! The package's widgets in a window: `cargo run --example gallery`.

use frus::{column, BuildContext, Color, FrusApp, StatelessWidget, Widget};
use {{crate_name}}::Tag;

#[derive(Clone)]
struct Gallery;

impl StatelessWidget for Gallery {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        Box::new(
            column![
                Tag::new("New").into_widget(),
                Tag::new("Inbox").count(3).into_widget(),
                Tag::new("Spam").count(120).into_widget(),
                Tag::new("Custom")
                    .color(Color::rgb(0.13, 0.55, 0.33))
                    .text_color(Color::rgb(1.0, 1.0, 1.0))
                    .radius(4.0)
                    .into_widget(),
            ]
            .gap(12.0)
            .padding(24.0),
        )
    }
}

// The same entry point an application declares; on the desktop it makes `run()`.
frus::main!(FrusApp::new(Gallery).title("{{project-name}} gallery"));

fn main() -> frus::anyhow::Result<()> {
    run()
}
