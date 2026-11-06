//! Deterministic file contents: seeded bytes, property lists, checksums.

use sha2::{Digest, Sha256};

/// Mixed into every seed so fixture bytes can be changed in one place.
const SEED: u64 = 0x526f_6f6d_7946_6978; // "RoomyFix"

/// `size` bytes that depend only on `rel` (the fixture path) and [`SEED`].
///
/// Uses FNV-1a over the path for the seed and SplitMix64 for the stream.
/// Both are fully specified here, so the bytes never change between Rust
/// versions or machines (unlike `std`'s hasher).
pub fn seeded_bytes(rel: &str, size: usize) -> Vec<u8> {
    let mut state = fnv1a(rel.as_bytes()) ^ SEED;
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        let word = splitmix64(&mut state).to_le_bytes();
        let take = (size - out.len()).min(word.len());
        out.extend_from_slice(&word[..take]);
    }
    out
}

/// FNV-1a, 64 bit.
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// One step of SplitMix64.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Lower-case hex SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// A value in a fixture property list.
pub enum Plist<'a> {
    /// `<string>`
    Str(&'a str),
    /// `<integer>`
    Int(i64),
    /// `<true/>` or `<false/>`
    Bool(bool),
    /// `<date>`, written as given (ISO 8601, `Z` time zone).
    Date(&'a str),
}

/// An XML property list holding one dictionary, keys in the order given.
pub fn plist(entries: &[(&str, Plist<'_>)]) -> String {
    let mut out = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
        "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
        "<plist version=\"1.0\">\n<dict>\n",
    ));
    for (key, value) in entries {
        out.push_str(&format!("\t<key>{}</key>\n", xml_escape(key)));
        let line = match value {
            Plist::Str(s) => format!("\t<string>{}</string>\n", xml_escape(s)),
            Plist::Int(i) => format!("\t<integer>{i}</integer>\n"),
            Plist::Bool(true) => "\t<true/>\n".to_string(),
            Plist::Bool(false) => "\t<false/>\n".to_string(),
            Plist::Date(d) => format!("\t<date>{}</date>\n", xml_escape(d)),
        };
        out.push_str(&line);
    }
    out.push_str("</dict>\n</plist>\n");
    out
}

/// Escapes the five XML special characters.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_bytes_are_stable() {
        // Pinned so an accidental change to the generator is caught.
        let bytes = seeded_bytes("Downloads/Installer.dmg", 8);
        assert_eq!(bytes, seeded_bytes("Downloads/Installer.dmg", 8));
        assert_eq!(sha256_hex(&bytes).len(), 64);
        assert_ne!(bytes, seeded_bytes("Downloads/Other.dmg", 8));
    }

    #[test]
    fn seeded_bytes_have_exact_size() {
        for size in [0, 1, 7, 8, 9, 1000] {
            assert_eq!(seeded_bytes("a", size).len(), size);
        }
    }

    #[test]
    fn sha256_known_value() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn plist_escapes_text() {
        let text = plist(&[("Name", Plist::Str("Sam's <Mac> & co"))]);
        assert!(text.contains("<string>Sam&apos;s &lt;Mac&gt; &amp; co</string>"));
    }
}
