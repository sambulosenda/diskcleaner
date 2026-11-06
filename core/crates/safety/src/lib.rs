//! Path validation and protected paths. Reads metadata only.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert!(!env!("CARGO_PKG_NAME").is_empty());
    }
}
