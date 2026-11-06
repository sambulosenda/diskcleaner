//! Per-tool analysis (xcode, android, js, docker, everyday). Read only; may run read-only CLI queries.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert!(!env!("CARGO_PKG_NAME").is_empty());
    }
}
