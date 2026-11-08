use frus::ValueNotifier;

use crate::models::user::User;

/// Who is signed in. Shared by the pages that sign in and out.
#[derive(Clone)]
pub(crate) struct Auth {
    pub user: ValueNotifier<Option<User>>,
}

impl Auth {
    /// Nobody is signed in.
    pub(crate) fn new() -> Self {
        Auth {
            user: ValueNotifier::new(None),
        }
    }

    /// Signs `email` in. The backend is not wired in yet: any valid form signs in.
    pub(crate) fn sign_in(&self, email: String) {
        let name = email.split('@').next().unwrap_or_default().to_string();
        self.user.set(Some(User {
            id: "1".to_string(),
            name,
            email,
        }));
    }

    pub(crate) fn logout(&self) {
        self.user.set(None);
    }
}
