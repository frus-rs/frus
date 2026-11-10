//! What an application sees: `cargo run --example demo`.

fn main() {
    match {{crate_name}}::platform_version() {
        Ok(version) => println!("{{project-name}}: {version}"),
        Err(error) => println!("{error}"),
    }
}
