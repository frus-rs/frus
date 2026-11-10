//! The package's tests: `frus test` (or `cargo test`). They need no window and no GPU.

use {{crate_name}}::Tag;

#[test]
fn a_tag_says_its_label() {
    assert_eq!(Tag::new("New").caption(), "New");
}

#[test]
fn a_count_follows_the_label() {
    assert_eq!(Tag::new("Inbox").count(3).caption(), "Inbox · 3");
}

#[test]
fn a_large_count_is_cut_short() {
    assert_eq!(Tag::new("Spam").count(120).caption(), "Spam · 99+");
}

#[test]
fn every_visual_can_be_given() {
    let plain = Tag::new("A");
    let dressed = Tag::new("A").radius(4.0).padding(2.0);
    assert_ne!(plain, dressed);
}
