//! Per-tool analysis (xcode, android, js, docker, everyday). Read only; may run read-only CLI queries.

pub mod android;
pub mod apps;
pub mod docker;
pub mod ios_backup;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert!(!env!("CARGO_PKG_NAME").is_empty());
    }
}
