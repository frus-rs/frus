/// The notifications page: which list is shown.
#[derive(Default)]
pub(crate) struct NotificationsState {
    /// 0 all, 1 unread, 2 mentions.
    pub tab: usize,
    pub all_read: bool,
}
