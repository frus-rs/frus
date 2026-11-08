use crate::models::{activity::Activity, member::Member};

/// "Sarah moved Router to Review": what happened, in words.
///
/// `named` says the signed-in member by name ("Kokou assigned …") rather than "You".
pub(crate) fn sentence(entry: &Activity, member: &Member, named: bool) -> String {
    let who = if member.is_you && !named {
        "You".to_string()
    } else {
        member.first_name().to_string()
    };
    let mut words = vec![who, entry.action.clone()];
    if !entry.subject.is_empty() {
        words.push(entry.subject.clone());
    }
    if !entry.tail.is_empty() {
        words.push(entry.tail.clone());
    }
    words.join(" ")
}
