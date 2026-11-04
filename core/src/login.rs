//! Reading the device-code banner `codex login --device-auth` prints.
//!
//! The CLI colours its output with ANSI escapes, so every match has to happen
//! on the stripped text. Kept here rather than in the shell so it can be tested
//! against the real banner.

/// Remove ANSI colour/formatting escapes.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// A device code is two groups of capitals or digits, e.g. `S6ZU-7HYU9`.
/// The groups are not a fixed width — the live code had four then five — so
/// this checks the shape rather than a length.
pub fn is_device_code(s: &str) -> bool {
    let (a, b) = match s.split_once('-') {
        Some((a, b)) => (a, b),
        None => return false,
    };
    let ok = |p: &str| {
        (3..=6).contains(&p.len())
            && p.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    };
    ok(a) && ok(b)
}

/// The one-time code and the verification URL, when the banner carries them.
pub fn parse_device_code(raw: &str) -> (Option<String>, Option<String>) {
    let plain = strip_ansi(raw);
    let mut code = None;
    let mut url = None;
    for line in plain.lines() {
        let t = line.trim();
        if url.is_none() {
            if let Some(i) = t.find("https://") {
                let candidate: String = t[i..].chars().take_while(|c| !c.is_whitespace()).collect();
                if candidate.contains("device") {
                    url = Some(candidate);
                }
            }
        }
        if code.is_none() {
            let token = t.split_whitespace().next().unwrap_or("");
            if is_device_code(token) {
                code = Some(token.to_string());
            }
        }
    }
    (code, url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_colour_sequences() {
        assert_eq!(strip_ansi("\x1b[94mok\x1b[0m"), "ok");
    }

    #[test]
    fn recognises_the_code_shape() {
        assert!(is_device_code("S6ZU-7HYU9"));
        assert!(is_device_code("ABCD-EFGH"));
        assert!(!is_device_code("S6ZU"));
        assert!(!is_device_code("s6zu-7hyu9"));
        assert!(!is_device_code("auth.openai.com"));
        // A URL fragment must never be mistaken for a code.
        assert!(!is_device_code("codex/device"));
    }
}
