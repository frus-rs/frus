// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`ThemeExtension`]: an application's or a design system's **own tokens**, carried by
//! the theme beside the framework's (milestone 778).
//!
//! The theme has a slot for every token the framework's widgets read. A brand colour, a
//! card's corner, the spacing scale of a design system have no slot there, and putting
//! them in a global beside the theme means they do not follow a subtree's theme, do not
//! switch with light and dark, and do not fade when the theme does. An extension does all
//! three.

use std::any::{Any, TypeId};
use std::fmt;
use std::sync::Arc;

/// **Tokens of your own**, stored in the [`Theme`](crate::Theme) and read back by type.
///
/// Implement it for a plain struct, store it with
/// [`Theme::with_extension`](crate::Theme::with_extension) and read it with
/// [`Theme::extension`](crate::Theme::extension). [`lerp`](Self::lerp) is how the tokens
/// travel while the theme fades from one to another, between light and dark for instance;
/// by default they switch half way.
///
/// ```
/// use frus_widgets::{Color, Theme, ThemeExtension};
///
/// #[derive(Clone, Debug, PartialEq)]
/// struct Brand {
///     highlight: Color,
///     card_radius: f32,
/// }
///
/// impl ThemeExtension for Brand {
///     fn lerp(&self, other: &Self, t: f32) -> Self {
///         Brand {
///             highlight: self.highlight.lerp(other.highlight, t),
///             card_radius: self.card_radius + (other.card_radius - self.card_radius) * t,
///         }
///     }
/// }
///
/// let theme = Theme::light().with_extension(Brand {
///     highlight: Color::rgb8(255, 120, 0),
///     card_radius: 12.0,
/// });
/// let brand = theme.extension::<Brand>().expect("stored above");
/// assert_eq!(brand.card_radius, 12.0);
/// ```
pub trait ThemeExtension: Clone + PartialEq + fmt::Debug + Send + Sync + 'static {
    /// These tokens `t` of the way to `other` (`0` is `self`, `1` is `other`).
    ///
    /// The default switches at the half: right for tokens that cannot be in between, such
    /// as a font family. Implement it for colours and sizes, so they fade with the rest of
    /// the theme.
    fn lerp(&self, other: &Self, t: f32) -> Self {
        if t < 0.5 {
            self.clone()
        } else {
            other.clone()
        }
    }
}

/// The type-erased form a theme stores.
trait Erased: Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn eq_erased(&self, other: &dyn Erased) -> bool;
    fn lerp_erased(&self, other: &dyn Erased, t: f32) -> Arc<dyn Erased>;
    fn debug(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

impl<T: ThemeExtension> Erased for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn eq_erased(&self, other: &dyn Erased) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn lerp_erased(&self, other: &dyn Erased, t: f32) -> Arc<dyn Erased> {
        match other.as_any().downcast_ref::<T>() {
            Some(other) => Arc::new(ThemeExtension::lerp(self, other, t)),
            None => Arc::new(self.clone()),
        }
    }

    fn debug(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// One stored extension: its type, and its value.
type Entry = (TypeId, Arc<dyn Erased>);

/// The extensions a [`Theme`](crate::Theme) carries, one per type.
///
/// Cheap to clone and one pointer wide: a theme is copied often, and an empty set costs
/// nothing at all. Reached through [`Theme::with_extension`](crate::Theme::with_extension)
/// and [`Theme::extension`](crate::Theme::extension).
#[derive(Clone, Default)]
pub struct ThemeExtensions {
    entries: Option<Arc<Vec<Entry>>>,
}

impl ThemeExtensions {
    /// No extensions.
    pub const EMPTY: Self = Self { entries: None };

    /// Stores `value`, replacing any extension of the same type.
    pub fn insert<T: ThemeExtension>(&mut self, value: T) {
        let entries = Arc::make_mut(self.entries.get_or_insert_with(Default::default));
        let id = TypeId::of::<T>();
        let value: Arc<dyn Erased> = Arc::new(value);
        match entries.iter_mut().find(|(known, _)| *known == id) {
            Some(slot) => slot.1 = value,
            None => entries.push((id, value)),
        }
    }

    /// The extension of type `T`, if one is stored.
    pub fn get<T: ThemeExtension>(&self) -> Option<&T> {
        let id = TypeId::of::<T>();
        self.entries
            .as_ref()?
            .iter()
            .find(|(known, _)| *known == id)
            .and_then(|(_, value)| value.as_any().downcast_ref::<T>())
    }

    /// How many extensions are stored.
    pub fn len(&self) -> usize {
        self.entries.as_ref().map_or(0, |entries| entries.len())
    }

    /// Whether none is.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The set `t` of the way to `other`: each type present on both sides is
    /// interpolated with its [`ThemeExtension::lerp`]; one present on a single side is
    /// kept from that side, as the theme's other discrete settings are, until the half.
    pub(crate) fn lerp(&self, other: &Self, t: f32) -> Self {
        if self.is_empty() && other.is_empty() {
            return Self::EMPTY;
        }
        let empty = Vec::new();
        let ours = self.entries.as_deref().unwrap_or(&empty);
        let theirs = other.entries.as_deref().unwrap_or(&empty);
        let mut out: Vec<Entry> = Vec::new();
        for (id, value) in ours {
            match theirs.iter().find(|(known, _)| known == id) {
                Some((_, target)) => out.push((*id, value.lerp_erased(target.as_ref(), t))),
                None if t < 0.5 => out.push((*id, value.clone())),
                None => {}
            }
        }
        if t >= 0.5 {
            for (id, value) in theirs {
                if !ours.iter().any(|(known, _)| known == id) {
                    out.push((*id, value.clone()));
                }
            }
        }
        Self {
            entries: (!out.is_empty()).then(|| Arc::new(out)),
        }
    }
}

impl PartialEq for ThemeExtensions {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        let (Some(ours), Some(theirs)) = (&self.entries, &other.entries) else {
            return true;
        };
        if Arc::ptr_eq(ours, theirs) {
            return true;
        }
        ours.iter().all(|(id, value)| {
            theirs
                .iter()
                .find(|(known, _)| known == id)
                .is_some_and(|(_, target)| value.eq_erased(target.as_ref()))
        })
    }
}

impl fmt::Debug for ThemeExtensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Shown<'a>(&'a dyn Erased);
        impl fmt::Debug for Shown<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.debug(f)
            }
        }
        let mut list = f.debug_list();
        for (_, value) in self.entries.iter().flat_map(|entries| entries.iter()) {
            list.entry(&Shown(value.as_ref()));
        }
        list.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Theme};

    #[derive(Clone, Debug, PartialEq)]
    struct Brand {
        highlight: Color,
        radius: f32,
    }

    impl ThemeExtension for Brand {
        fn lerp(&self, other: &Self, t: f32) -> Self {
            Brand {
                highlight: self.highlight.lerp(other.highlight, t),
                radius: self.radius + (other.radius - self.radius) * t,
            }
        }
    }

    /// One that keeps the default lerp: it switches at the half.
    #[derive(Clone, Debug, PartialEq)]
    struct Family(&'static str);
    impl ThemeExtension for Family {}

    fn brand(radius: f32) -> Brand {
        Brand {
            highlight: Color::rgb8(255, 0, 0),
            radius,
        }
    }

    #[test]
    fn an_extension_is_read_back_by_its_type() {
        let theme = Theme::light()
            .with_extension(brand(12.0))
            .with_extension(Family("Inter"));
        assert_eq!(theme.extension::<Brand>(), Some(&brand(12.0)));
        assert_eq!(theme.extension::<Family>(), Some(&Family("Inter")));
        assert_eq!(Theme::light().extension::<Brand>(), None, "none stored");
    }

    #[test]
    fn storing_a_type_again_replaces_it() {
        let theme = Theme::light()
            .with_extension(brand(4.0))
            .with_extension(brand(8.0));
        assert_eq!(theme.extensions.len(), 1);
        assert_eq!(theme.extension::<Brand>().unwrap().radius, 8.0);
    }

    #[test]
    fn a_copy_of_the_theme_does_not_share_later_changes() {
        let first = Theme::light().with_extension(brand(4.0));
        let second = first.clone().with_extension(brand(8.0));
        assert_eq!(first.extension::<Brand>().unwrap().radius, 4.0);
        assert_eq!(second.extension::<Brand>().unwrap().radius, 8.0);
        assert_ne!(first, second, "themes with different tokens differ");
        assert_eq!(first, first.clone());
    }

    /// **Extensions fade with the theme**: half way between light and dark, a brand
    /// radius of 4 and one of 8 make 6, and a token that cannot be in between switches.
    #[test]
    fn extensions_lerp_when_the_theme_does() {
        let light = Theme::light()
            .with_extension(brand(4.0))
            .with_extension(Family("Inter"));
        let dark = Theme::dark()
            .with_extension(Brand {
                highlight: Color::rgb8(0, 0, 255),
                radius: 8.0,
            })
            .with_extension(Family("Roboto"));
        let half = light.lerp(&dark, 0.5);
        let b = half.extension::<Brand>().unwrap();
        assert_eq!(b.radius, 6.0);
        assert!(
            b.highlight.r > 0.0 && b.highlight.b > 0.0,
            "{:?}",
            b.highlight
        );
        assert_eq!(
            light.lerp(&dark, 0.25).extension::<Family>().unwrap().0,
            "Inter"
        );
        assert_eq!(half.extension::<Family>().unwrap().0, "Roboto");
        assert_eq!(
            light.lerp(&dark, 1.0).extension::<Brand>().unwrap().radius,
            8.0
        );
    }

    /// One only one side has stays until the half, then the other side's word holds.
    #[test]
    fn an_extension_on_one_side_switches_at_the_half() {
        let with = Theme::light().with_extension(brand(4.0));
        let without = Theme::dark();
        assert!(with.lerp(&without, 0.4).extension::<Brand>().is_some());
        assert!(with.lerp(&without, 0.6).extension::<Brand>().is_none());
        assert!(without.lerp(&with, 0.6).extension::<Brand>().is_some());
    }
}
