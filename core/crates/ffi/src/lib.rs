//! UniFFI surface between the Rust core and the SwiftUI app. Never touches the filesystem.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert!(!env!("CARGO_PKG_NAME").is_empty());
    }
}
