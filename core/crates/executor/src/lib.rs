//! The only crate allowed to modify the filesystem.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert!(!env!("CARGO_PKG_NAME").is_empty());
    }
}
