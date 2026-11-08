//! The design's colours.

use frus::Color;

/// The brand green: buttons, links, active tabs (`#00C950`).
pub(crate) fn primary_color() -> Color {
    Color::hex("#00C950")
}

/// The text on a green button (`#F0FDF4`).
pub(crate) fn white() -> Color {
    Color::hex("#F0FDF4")
}

/// Headings and body text (`#09090B`).
pub(crate) fn black() -> Color {
    Color::hex("#09090B")
}

/// Secondary text and icons (`#71717B`).
pub(crate) fn grey() -> Color {
    Color::hex("#71717B")
}

/// Borders and dividers (`#E4E4E7`).
pub(crate) fn gray() -> Color {
    Color::hex("#E4E4E7")
}

/// Grey fills: tiles, chips, avatars (`#F4F4F5`).
pub(crate) fn light_gray() -> Color {
    Color::hex("#F4F4F5")
}

/// A page's background.
pub(crate) fn surface() -> Color {
    Color::hex("#FFFFFF")
}

/// The green at 10 % over white: banners, selected chips, the signed-in member's avatar.
///
/// Opaque, so that it is the design's colour exactly: every place it is used sits on white.
pub(crate) fn green_tint() -> Color {
    Color::hex("#E6FAEE")
}

/// The green at 15 % over white: the signed-in member's small avatars.
pub(crate) fn green_tint_strong() -> Color {
    Color::hex("#D9F7E5")
}

/// The red at 10 % over white: the block that signs out everywhere.
pub(crate) fn red_wash() -> Color {
    Color::hex("#FDE6E7")
}

/// Destructive actions (`#E7000B`).
pub(crate) fn red() -> Color {
    Color::hex("#E7000B")
}

/// The fill behind a destructive block (`#FFE2E2`).
pub(crate) fn red_tint() -> Color {
    Color::hex("#FFE2E2")
}

/// The text of the "High" priority badge (`#C10007`).
pub(crate) fn red_text() -> Color {
    Color::hex("#C10007")
}

/// Medium priority (`#F0B100`).
pub(crate) fn amber() -> Color {
    Color::hex("#F0B100")
}

/// Low priority (`#009689`).
pub(crate) fn teal() -> Color {
    Color::hex("#009689")
}
