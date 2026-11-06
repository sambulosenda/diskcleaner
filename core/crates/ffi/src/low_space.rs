//! Low-space alerts: decides when the menu bar item should warn that the
//! disk is nearly full.
//!
//! Pure logic. Nothing here touches the filesystem: the app reads free space
//! with [`crate::disk_summary`] and passes the number in. Nothing here ever
//! cleans anything either; an alert only tells the user, and the user opens
//! Roomy to choose what to remove.
//!
//! An alert fires once when free space drops below the threshold. It does
//! not fire again until free space has climbed back above the threshold by a
//! margin, so a disk hovering around the line does not keep notifying.

/// One gigabyte, in the decimal units Finder uses.
const GB: u64 = 1_000_000_000;

/// The smallest threshold the core accepts. Lower values are raised to it.
const MIN_THRESHOLD: u64 = GB;

/// The largest threshold the core accepts. Higher values are lowered to it.
const MAX_THRESHOLD: u64 = 200 * GB;

/// The smallest margin above the threshold before an alert can fire again.
const MIN_MARGIN: u64 = GB;

/// The thresholds offered in Settings, smallest first.
const THRESHOLD_CHOICES: [u64; 4] = [5 * GB, 10 * GB, 20 * GB, 50 * GB];

/// The threshold used until the user picks one.
const DEFAULT_THRESHOLD: u64 = 10 * GB;

/// What the app should do after a free-space reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct LowSpaceCheck {
    /// Free space is below the threshold right now.
    pub is_low: bool,
    /// Show a notification now. True only on the reading where space first
    /// drops below the threshold since it last recovered.
    pub should_alert: bool,
    /// Whether the user has been warned. The app keeps this and passes it
    /// back as `warned` with the next reading.
    pub warned: bool,
    /// The threshold that was used, after raising or lowering it into range.
    pub threshold_bytes: u64,
}

/// Decides whether free space is low and whether to notify.
///
/// `warned` is the value returned by the previous call (false the first
/// time). The threshold is kept between 1 GB and 200 GB.
#[uniffi::export]
pub fn check_low_space(free_bytes: u64, threshold_bytes: u64, warned: bool) -> LowSpaceCheck {
    let threshold = threshold_bytes.clamp(MIN_THRESHOLD, MAX_THRESHOLD);
    let is_low = free_bytes < threshold;
    let (should_alert, warned_after) = if is_low {
        (!warned, true)
    } else if free_bytes >= threshold.saturating_add(rearm_margin(threshold)) {
        (false, false)
    } else {
        (false, warned)
    };
    LowSpaceCheck {
        is_low,
        should_alert,
        warned: warned_after,
        threshold_bytes: threshold,
    }
}

/// The thresholds Settings offers, in bytes, smallest first.
#[uniffi::export]
pub fn low_space_threshold_choices() -> Vec<u64> {
    THRESHOLD_CHOICES.to_vec()
}

/// The threshold used until the user picks one, in bytes.
#[uniffi::export]
pub fn low_space_default_threshold() -> u64 {
    DEFAULT_THRESHOLD
}

/// How far above the threshold free space must climb before the next alert
/// can fire: a tenth of the threshold, and at least 1 GB.
fn rearm_margin(threshold: u64) -> u64 {
    (threshold / 10).max(MIN_MARGIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 10 * GB;

    #[test]
    fn plenty_of_space_is_not_low_and_does_not_alert() {
        let check = check_low_space(100 * GB, T, false);
        assert!(!check.is_low);
        assert!(!check.should_alert);
        assert!(!check.warned);
        assert_eq!(check.threshold_bytes, T);
    }

    #[test]
    fn dropping_below_the_threshold_alerts_once() {
        let first = check_low_space(9 * GB, T, false);
        assert!(first.is_low);
        assert!(first.should_alert);
        assert!(first.warned);

        let second = check_low_space(8 * GB, T, first.warned);
        assert!(second.is_low);
        assert!(
            !second.should_alert,
            "must not notify again while still low"
        );
        assert!(second.warned);
    }

    #[test]
    fn exactly_at_the_threshold_is_not_low() {
        let check = check_low_space(T, T, false);
        assert!(!check.is_low);
        assert!(!check.should_alert);
    }

    #[test]
    fn hovering_just_above_the_threshold_does_not_rearm() {
        let low = check_low_space(T - 1, T, false);
        assert!(low.should_alert);
        // 10.5 GB: above the line, but inside the 1 GB margin.
        let hover = check_low_space(T + GB / 2, T, low.warned);
        assert!(!hover.is_low);
        assert!(hover.warned, "still counts as warned inside the margin");
        let again = check_low_space(T - 1, T, hover.warned);
        assert!(!again.should_alert, "no second alert after hovering");
    }

    #[test]
    fn recovering_past_the_margin_rearms_the_alert() {
        let low = check_low_space(5 * GB, T, false);
        let recovered = check_low_space(T + GB, T, low.warned);
        assert!(!recovered.warned);
        assert!(!recovered.should_alert);
        let low_again = check_low_space(5 * GB, T, recovered.warned);
        assert!(low_again.should_alert);
    }

    #[test]
    fn the_margin_is_a_tenth_of_large_thresholds() {
        let t = 50 * GB;
        let low = check_low_space(t - 1, t, false);
        // 54 GB is inside the 5 GB margin, 55 GB is not.
        assert!(check_low_space(54 * GB, t, low.warned).warned);
        assert!(!check_low_space(55 * GB, t, low.warned).warned);
    }

    #[test]
    fn thresholds_are_kept_in_range() {
        assert_eq!(check_low_space(0, 0, false).threshold_bytes, MIN_THRESHOLD);
        assert_eq!(
            check_low_space(0, u64::MAX, false).threshold_bytes,
            MAX_THRESHOLD
        );
        // A zero threshold cannot switch alerts off by accident: an empty disk is still low.
        assert!(check_low_space(0, 0, false).should_alert);
    }

    #[test]
    fn huge_free_space_does_not_overflow() {
        let check = check_low_space(u64::MAX, MAX_THRESHOLD, true);
        assert!(!check.is_low);
        assert!(!check.warned);
    }

    #[test]
    fn the_default_is_one_of_the_choices_and_choices_are_in_range() {
        let choices = low_space_threshold_choices();
        assert!(choices.contains(&low_space_default_threshold()));
        assert!(choices.windows(2).all(|w| w[0] < w[1]));
        for c in choices {
            assert_eq!(check_low_space(0, c, false).threshold_bytes, c);
        }
    }
}
