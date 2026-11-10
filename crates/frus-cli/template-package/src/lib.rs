//! `{{project-name}}` — widgets for frus applications.
//!
//! An application adds the package and uses its widgets like frus's own:
//!
//! ```ignore
//! use {{crate_name}}::Tag;
//!
//! column![Tag::new("Inbox").count(3).into_widget()]
//! ```
//!
//! Every visual of a widget comes from the theme by default and can be given per widget:
//! `Tag::new("Inbox").color(..).text_color(..)`.

use frus::{text, BuildContext, Color, Container, StatelessWidget, Widget};

/// A small rounded label, with a count after it if there is one: `Inbox · 3`.
#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    label: String,
    count: Option<u32>,
    color: Option<Color>,
    text_color: Option<Color>,
    radius: f32,
    padding: f32,
}

impl Tag {
    /// A tag that says `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Tag {
            label: label.into(),
            count: None,
            color: None,
            text_color: None,
            radius: 12.0,
            padding: 8.0,
        }
    }

    /// A count after the label; a count above 99 is said `99+`.
    pub fn count(mut self, count: u32) -> Self {
        self.count = Some(count);
        self
    }

    /// The fill, instead of the theme's secondary container.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// The text's color, instead of what is legible on the theme's secondary container.
    pub fn text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    /// How round the corners are (default 12).
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// The room around the text (default 8).
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }

    /// What the tag says: `Inbox`, `Inbox · 3`, `Inbox · 99+`.
    pub fn caption(&self) -> String {
        match self.count {
            None => self.label.clone(),
            Some(count) if count > 99 => format!("{} · 99+", self.label),
            Some(count) => format!("{} · {count}", self.label),
        }
    }
}

impl StatelessWidget for Tag {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let scheme = &cx.theme().scheme;
        Box::new(
            Container::new()
                .padding(self.padding)
                .radius(self.radius)
                .color(self.color.unwrap_or(scheme.secondary_container))
                .child(
                    text(self.caption())
                        .color(self.text_color.unwrap_or(scheme.on_secondary_container)),
                ),
        )
    }
}
