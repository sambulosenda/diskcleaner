//! Per-tool analysis (xcode, android, js, docker, everyday). Read only; may run read-only CLI queries.

pub mod apps;
pub mod docker;
pub mod files;
pub mod ios_backup;
pub mod js;
pub mod libraries;
mod process;
pub mod system_data;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert!(!env!("CARGO_PKG_NAME").is_empty());
    }
}
