//! The product's name, in one place, for every user-facing message the core writes.
//!
//! Change it in [`name!`] (and `AppInfo.name` in the Swift app) to rename the product. It is for
//! words only: never build a path, bundle id or storage key from it, or a rename would lose user data.

/// The product's name as a string literal, for `concat!` in constants.
#[macro_export]
macro_rules! name {
    () => {
        "Diskwell"
    };
}

/// The product's name as users see it. Use `product = brand::PRODUCT_NAME` in `#[error]` messages.
pub const PRODUCT_NAME: &str = name!();

#[cfg(test)]
mod tests {
    use super::PRODUCT_NAME;

    #[test]
    fn name_is_one_plain_word() {
        assert!(!PRODUCT_NAME.is_empty());
        assert!(PRODUCT_NAME.chars().all(|c| c.is_ascii_alphabetic()));
    }

    #[test]
    fn macro_and_constant_agree() {
        assert_eq!(concat!(name!(), "!"), format!("{PRODUCT_NAME}!"));
    }
}
