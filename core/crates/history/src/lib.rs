//! SQLite log of scans and cleanups. Writes its own database only.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert!(!env!("CARGO_PKG_NAME").is_empty());
    }
}
