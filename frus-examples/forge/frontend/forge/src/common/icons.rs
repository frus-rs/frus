//! The design's icons, drawn as vectors (the Lucide set the design uses).

use frus::{Color, SvgPicture};

/// An icon of the design.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyph {
    Bell,
    Calendar,
    CalendarDays,
    Check,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    CircleCheck,
    Clock,
    Cloud,
    CloudCheck,
    Ellipsis,
    Eye,
    EyeOff,
    Folder,
    FolderKanban,
    Info,
    Laptop,
    LayoutDashboard,
    Lock,
    LogOut,
    Mail,
    Plus,
    Radio,
    Search,
    SearchX,
    Send,
    Settings,
    SlidersHorizontal,
    Smartphone,
    SquareCheck,
    UserPlus,
    Users,
    X,
}

impl Glyph {
    fn source(self) -> &'static [u8] {
        match self {
            Glyph::Bell => include_bytes!("../../assets/icons/svg/bell.svg"),
            Glyph::Calendar => include_bytes!("../../assets/icons/svg/calendar.svg"),
            Glyph::CalendarDays => include_bytes!("../../assets/icons/svg/calendar-days.svg"),
            Glyph::Check => include_bytes!("../../assets/icons/svg/check.svg"),
            Glyph::ChevronDown => include_bytes!("../../assets/icons/svg/chevron-down.svg"),
            Glyph::ChevronLeft => include_bytes!("../../assets/icons/svg/chevron-left.svg"),
            Glyph::ChevronRight => include_bytes!("../../assets/icons/svg/chevron-right.svg"),
            Glyph::CircleCheck => include_bytes!("../../assets/icons/svg/circle-check.svg"),
            Glyph::Clock => include_bytes!("../../assets/icons/svg/clock.svg"),
            Glyph::Cloud => include_bytes!("../../assets/icons/svg/cloud.svg"),
            Glyph::CloudCheck => include_bytes!("../../assets/icons/svg/cloud-check.svg"),
            Glyph::Ellipsis => include_bytes!("../../assets/icons/svg/ellipsis.svg"),
            Glyph::Eye => include_bytes!("../../assets/icons/svg/eye.svg"),
            Glyph::EyeOff => include_bytes!("../../assets/icons/svg/eye-off.svg"),
            Glyph::Folder => include_bytes!("../../assets/icons/svg/folder.svg"),
            Glyph::FolderKanban => include_bytes!("../../assets/icons/svg/folder-kanban.svg"),
            Glyph::Info => include_bytes!("../../assets/icons/svg/info.svg"),
            Glyph::Laptop => include_bytes!("../../assets/icons/svg/laptop.svg"),
            Glyph::LayoutDashboard => include_bytes!("../../assets/icons/svg/layout-dashboard.svg"),
            Glyph::Lock => include_bytes!("../../assets/icons/svg/lock.svg"),
            Glyph::LogOut => include_bytes!("../../assets/icons/svg/log-out.svg"),
            Glyph::Mail => include_bytes!("../../assets/icons/svg/mail.svg"),
            Glyph::Plus => include_bytes!("../../assets/icons/svg/plus.svg"),
            Glyph::Radio => include_bytes!("../../assets/icons/svg/radio.svg"),
            Glyph::Search => include_bytes!("../../assets/icons/svg/search.svg"),
            Glyph::SearchX => include_bytes!("../../assets/icons/svg/search-x.svg"),
            Glyph::Send => include_bytes!("../../assets/icons/svg/send.svg"),
            Glyph::Settings => include_bytes!("../../assets/icons/svg/settings.svg"),
            Glyph::SlidersHorizontal => {
                include_bytes!("../../assets/icons/svg/sliders-horizontal.svg")
            }
            Glyph::Smartphone => include_bytes!("../../assets/icons/svg/smartphone.svg"),
            Glyph::SquareCheck => include_bytes!("../../assets/icons/svg/square-check.svg"),
            Glyph::UserPlus => include_bytes!("../../assets/icons/svg/user-plus.svg"),
            Glyph::Users => include_bytes!("../../assets/icons/svg/users.svg"),
            Glyph::X => include_bytes!("../../assets/icons/svg/x.svg"),
        }
    }
}

/// `glyph`, `size` square, drawn in `color`.
pub(crate) fn icon(glyph: Glyph, size: f32, color: Color) -> SvgPicture {
    SvgPicture::memory(glyph.source())
        .size(size, size)
        .color(color)
}
