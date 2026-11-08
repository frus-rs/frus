use frus::{
    Align, BuildContext, Column, Container, Expanded, FontWeight, RichText, Row, StatelessWidget,
    TextSpan, Widget,
};

use crate::{
    common::{
        colors::{black, grey, primary_color},
        ui::{avatar, banner, dot, group, section_heading},
    },
    models::{activity::Activity, member::Member},
};

/// The dashboard's latest changes: the sync state and a list of who did what.
pub(crate) struct RecentActivity {
    pub entries: Vec<(Activity, Member)>,
}

impl StatelessWidget for RecentActivity {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        let rows = self
            .entries
            .iter()
            .map(|(entry, member)| {
                let who = if member.is_you {
                    "You"
                } else {
                    member.first_name()
                };
                let mut sentence = TextSpan::new(format!("{who} "))
                    .size(12.0)
                    .color(black())
                    .weight(FontWeight::SemiBold);
                let action = if member.is_you && entry.action == "assigned" {
                    "were assigned".to_string()
                } else {
                    entry.action.clone()
                };
                sentence = sentence
                    .child(TextSpan::new(format!("{action} ")).weight(FontWeight::SemiBold));
                if !entry.subject.is_empty() {
                    sentence = sentence.child(
                        TextSpan::new(format!("“{}” ", entry.subject)).weight(FontWeight::SemiBold),
                    );
                }
                if !entry.tail.is_empty() {
                    sentence = sentence.child(
                        TextSpan::new(format!("{} ", entry.tail)).weight(FontWeight::SemiBold),
                    );
                }
                sentence = sentence.child(
                    TextSpan::new(format!("· {}", entry.ago))
                        .color(grey())
                        .weight(FontWeight::Regular),
                );
                let row: Box<dyn Widget> = Box::new(
                    Container::new().padding(12.0).child(
                        Row::new()
                            .spacing(12.0)
                            .cross_axis_alignment(Align::Center)
                            .child(avatar(&member.letter(), 28.0, 10.0, member.is_you))
                            .child(Expanded::new(
                                RichText::new(sentence)
                                    .base_style(frus::TextStyle::new(12.0).height(20.0 / 12.0)),
                            )),
                    ),
                );
                row
            })
            .collect();

        Box::new(
            Column::new()
                .spacing(12.0)
                .cross_axis_alignment(Align::Stretch)
                .child(section_heading("Recent activity"))
                .child(banner(
                    dot(primary_color(), 8.0),
                    "All changes synced",
                    8.0,
                    12.0,
                    10.0,
                ))
                .child(group(rows)),
        )
    }
}
