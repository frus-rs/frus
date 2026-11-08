use frus::{BuildContext, Container, FontWeight, StatelessWidget, Widget};

use crate::common::{
    colors::{green_tint_strong, grey, light_gray, primary_color, surface},
    ui::{filled, text, FULL},
};

/// A member's 28 px avatar in a project's stack: a white ring keeps the overlap readable.
pub(crate) struct MemberAvatar {
    pub letter: String,
    pub you: bool,
}

impl MemberAvatar {
    pub(crate) fn new(letter: impl Into<String>, you: bool) -> Self {
        Self {
            letter: letter.into(),
            you,
        }
    }
}

impl StatelessWidget for MemberAvatar {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        let (fill, ink) = if self.you {
            (green_tint_strong(), primary_color())
        } else {
            (light_gray(), grey())
        };
        Box::new(
            Container::new()
                .width(28.0)
                .height(28.0)
                .decoration(filled(surface(), FULL))
                .padding(2.0)
                .child(
                    Container::new()
                        .decoration(filled(fill, FULL))
                        .alignment(frus::Alignment::CENTER)
                        .child(text(
                            self.letter.clone(),
                            10.0,
                            12.0,
                            ink,
                            FontWeight::Medium,
                        )),
                ),
        )
    }
}
