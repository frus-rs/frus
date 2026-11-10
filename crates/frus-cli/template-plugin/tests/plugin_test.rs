//! The plugin's tests, on this computer: `frus test`. On a platform it has no side for, it says
//! so rather than pretending to answer.

use {{crate_name}}::{platform_version, PluginError};

#[test]
fn a_platform_without_a_side_says_so() {
    if cfg!(target_os = "android") {
        return;
    }
    match platform_version() {
        Err(PluginError::Unsupported(platform)) => assert!(!platform.is_empty()),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn errors_read_as_sentences() {
    let error = PluginError::Unsupported("Windows");
    assert!(error.to_string().contains("does not work on Windows"));
}
