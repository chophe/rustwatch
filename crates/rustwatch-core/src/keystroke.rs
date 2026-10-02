//! Pure key-to-text translation and app-exclusion matching.
//!
//! Both live in core (not the capture crate) for one reason: they are the
//! only two pieces of the capture path that can be tested without a real
//! keyboard, an Accessibility grant, or a running tap. Everything here is a
//! pure function over plain data.

/// Logical key identity, independent of any particular keytap version.
///
/// The capture crate translates `keytap::Key` into this enum, so the
/// translation rules below can be unit-tested without pulling in the tap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogicalKey {
    Letter(char),
    Digit(u8),
    Backtick,
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Space,
    Enter,
    Tab,
    Backspace,
    Escape,
    Numpad(u8),
}

/// Modifier state at the moment a key went down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub caps_lock: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        caps_lock: false,
    };

    /// Shift is effective for letters when either shift is down or Caps Lock
    /// is on. Caps Lock inverts, so shift+caps actually yields lowercase.
    fn letter_is_upper(self) -> bool {
        self.shift ^ self.caps_lock
    }
}

/// Translate a logical key into the text it inserts, honouring modifiers.
///
/// Returns `None` for keys that produce no text (arrows, function keys,
/// bare modifiers). This is the single source of truth for captured text:
/// callers must not re-derive case or punctuation themselves.
pub fn key_to_text(key: LogicalKey, mods: Modifiers) -> Option<String> {
    let ch = match key {
        LogicalKey::Letter(c) => {
            if mods.letter_is_upper() {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            }
        }
        LogicalKey::Digit(d) => {
            // Shift+digit is the US-layout symbol row.
            let shifted = match d {
                0 => ')',
                1 => '!',
                2 => '@',
                3 => '#',
                4 => '$',
                5 => '%',
                6 => '^',
                7 => '&',
                8 => '*',
                9 => '(',
                _ => return None,
            };
            if mods.shift {
                shifted
            } else {
                char::from(b'0' + d)
            }
        }
        LogicalKey::Backtick => pick(mods.shift, '~', '`'),
        LogicalKey::Minus => pick(mods.shift, '_', '-'),
        LogicalKey::Equal => pick(mods.shift, '+', '='),
        LogicalKey::BracketLeft => pick(mods.shift, '{', '['),
        LogicalKey::BracketRight => pick(mods.shift, '}', ']'),
        LogicalKey::Backslash => pick(mods.shift, '|', '\\'),
        LogicalKey::Semicolon => pick(mods.shift, ':', ';'),
        LogicalKey::Quote => pick(mods.shift, '"', '\''),
        LogicalKey::Comma => pick(mods.shift, '<', ','),
        LogicalKey::Period => pick(mods.shift, '>', '.'),
        LogicalKey::Slash => pick(mods.shift, '?', '/'),
        LogicalKey::Space => ' ',
        LogicalKey::Enter => '\n',
        LogicalKey::Tab => '\t',
        LogicalKey::Backspace => '\u{8}',
        LogicalKey::Escape => '\u{1b}',
        LogicalKey::Numpad(d) => char::from(b'0' + d.min(9)),
    };
    Some(ch.to_string())
}

fn pick(shifted: bool, with_shift: char, without: char) -> char {
    if shifted {
        with_shift
    } else {
        without
    }
}

/// Case-insensitive substring match against the exclusion list.
///
/// Substring rather than exact match because macOS reports app names
/// inconsistently ("1Password" vs "1Password 8" vs "1Password – Browser").
/// An empty list excludes nothing; blank entries are ignored.
pub fn is_excluded(name: &str, exclude_apps: &[String]) -> bool {
    if exclude_apps.is_empty() {
        return false;
    }
    let name = name.to_lowercase();
    exclude_apps.iter().any(|ex| {
        let ex = ex.trim();
        !ex.is_empty() && name.contains(&ex.to_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::{is_excluded, key_to_text, LogicalKey, Modifiers};

    const NONE: Modifiers = Modifiers::NONE;
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        caps_lock: false,
    };
    const CAPS: Modifiers = Modifiers {
        shift: false,
        caps_lock: true,
    };

    fn t(key: LogicalKey, mods: Modifiers) -> String {
        key_to_text(key, mods).expect("expected text")
    }

    #[test]
    fn lowercase_letters_are_unchanged() {
        assert_eq!(t(LogicalKey::Letter('a'), NONE), "a");
        assert_eq!(t(LogicalKey::Letter('z'), NONE), "z");
        assert_eq!(t(LogicalKey::Letter('q'), NONE), "q");
    }

    #[test]
    fn shift_uppercases_letters() {
        assert_eq!(t(LogicalKey::Letter('h'), SHIFT), "H");
        assert_eq!(t(LogicalKey::Letter('e'), SHIFT), "E");
        // Regression: the old table returned "h" regardless of shift.
        assert_eq!(t(LogicalKey::Letter('h'), NONE), "h");
    }

    #[test]
    fn caps_lock_uppercases_letters() {
        assert_eq!(t(LogicalKey::Letter('a'), CAPS), "A");
    }

    #[test]
    fn shift_and_caps_together_yield_lowercase() {
        let both = Modifiers {
            shift: true,
            caps_lock: true,
        };
        assert_eq!(t(LogicalKey::Letter('a'), both), "a");
    }

    #[test]
    fn unshifted_punctuation_produces_text() {
        // Regression: these all returned None before, so no TextDelta was sent.
        assert_eq!(t(LogicalKey::Period, NONE), ".");
        assert_eq!(t(LogicalKey::Comma, NONE), ",");
        assert_eq!(t(LogicalKey::Slash, NONE), "/");
        assert_eq!(t(LogicalKey::Quote, NONE), "'");
        assert_eq!(t(LogicalKey::Semicolon, NONE), ";");
        assert_eq!(t(LogicalKey::Minus, NONE), "-");
        assert_eq!(t(LogicalKey::Equal, NONE), "=");
        assert_eq!(t(LogicalKey::BracketLeft, NONE), "[");
        assert_eq!(t(LogicalKey::BracketRight, NONE), "]");
        assert_eq!(t(LogicalKey::Backslash, NONE), "\\");
        assert_eq!(t(LogicalKey::Backtick, NONE), "`");
    }

    #[test]
    fn shifted_punctuation_produces_symbols() {
        assert_eq!(t(LogicalKey::Period, SHIFT), ">");
        assert_eq!(t(LogicalKey::Comma, SHIFT), "<");
        assert_eq!(t(LogicalKey::Slash, SHIFT), "?");
        assert_eq!(t(LogicalKey::Quote, SHIFT), "\"");
        assert_eq!(t(LogicalKey::Semicolon, SHIFT), ":");
        assert_eq!(t(LogicalKey::Minus, SHIFT), "_");
        assert_eq!(t(LogicalKey::Equal, SHIFT), "+");
        assert_eq!(t(LogicalKey::BracketLeft, SHIFT), "{");
        assert_eq!(t(LogicalKey::BracketRight, SHIFT), "}");
        assert_eq!(t(LogicalKey::Backslash, SHIFT), "|");
        assert_eq!(t(LogicalKey::Backtick, SHIFT), "~");
    }

    #[test]
    fn digits_and_shifted_digits() {
        assert_eq!(t(LogicalKey::Digit(0), NONE), "0");
        assert_eq!(t(LogicalKey::Digit(7), NONE), "7");
        assert_eq!(t(LogicalKey::Digit(1), SHIFT), "!");
        assert_eq!(t(LogicalKey::Digit(9), SHIFT), "(");
        assert_eq!(t(LogicalKey::Digit(0), SHIFT), ")");
    }

    #[test]
    fn whitespace_and_editing_keys() {
        assert_eq!(t(LogicalKey::Space, NONE), " ");
        assert_eq!(t(LogicalKey::Enter, NONE), "\n");
        assert_eq!(t(LogicalKey::Tab, NONE), "\t");
        assert_eq!(t(LogicalKey::Backspace, NONE), "\u{8}");
        assert_eq!(t(LogicalKey::Escape, NONE), "\u{1b}");
    }

    #[test]
    fn reconstructs_a_realistic_sentence() {
        // "Fix the bug, please?" — shift applied only where it belongs.
        let keys: Vec<(LogicalKey, bool)> = vec![
            // "Fix" — only F is capitalised.
            (LogicalKey::Letter('f'), true),
            (LogicalKey::Letter('i'), false),
            (LogicalKey::Letter('x'), false),
            (LogicalKey::Space, false),
            (LogicalKey::Letter('t'), false),
            (LogicalKey::Letter('h'), false),
            (LogicalKey::Letter('e'), false),
            (LogicalKey::Space, false),
            (LogicalKey::Letter('b'), false),
            (LogicalKey::Letter('u'), false),
            (LogicalKey::Letter('g'), false),
            (LogicalKey::Comma, false),
            (LogicalKey::Space, false),
            (LogicalKey::Letter('p'), false),
            (LogicalKey::Letter('l'), false),
            (LogicalKey::Letter('e'), false),
            (LogicalKey::Letter('a'), false),
            (LogicalKey::Letter('s'), false),
            (LogicalKey::Letter('e'), false),
            // "?" is shift+/ — unshifted "/" would produce a literal slash.
            (LogicalKey::Slash, true),
        ];

        let mut out = String::new();
        for (key, shift) in keys {
            out.push_str(&key_to_text(key, if shift { SHIFT } else { NONE }).expect("text"));
        }
        assert_eq!(out, "Fix the bug, please?");
    }

    #[test]
    fn preserves_a_url() {
        let url = "https://example.com/a/b?x=1";
        let keys: Vec<(LogicalKey, bool)> = vec![
            (LogicalKey::Letter('h'), false),
            (LogicalKey::Letter('t'), false),
            (LogicalKey::Letter('t'), false),
            (LogicalKey::Letter('p'), false),
            (LogicalKey::Letter('s'), false),
            // "https://" — both colons come from shift+semicolon.
            (LogicalKey::Semicolon, true),
            (LogicalKey::Slash, false),
            (LogicalKey::Slash, false),
            (LogicalKey::Letter('e'), false),
            (LogicalKey::Letter('x'), false),
            (LogicalKey::Letter('a'), false),
            (LogicalKey::Letter('m'), false),
            (LogicalKey::Letter('p'), false),
            (LogicalKey::Letter('l'), false),
            (LogicalKey::Letter('e'), false),
            (LogicalKey::Period, false),
            (LogicalKey::Letter('c'), false),
            (LogicalKey::Letter('o'), false),
            (LogicalKey::Letter('m'), false),
            (LogicalKey::Slash, false),
            (LogicalKey::Letter('a'), false),
            (LogicalKey::Slash, false),
            (LogicalKey::Letter('b'), false),
            (LogicalKey::Slash, true),
            (LogicalKey::Letter('x'), false),
            (LogicalKey::Equal, false),
            (LogicalKey::Digit(1), false),
        ];

        let mut out = String::new();
        for (key, shift) in keys {
            out.push_str(&key_to_text(key, if shift { SHIFT } else { NONE }).expect("text"));
        }
        assert_eq!(out, url);
    }

    #[test]
    fn text_producing_keys_emit_exactly_one_char() {
        for key in [
            LogicalKey::Space,
            LogicalKey::Enter,
            LogicalKey::Tab,
            LogicalKey::Period,
            LogicalKey::Slash,
            LogicalKey::Digit(5),
        ] {
            let s = key_to_text(key, NONE).unwrap();
            assert_eq!(s.chars().count(), 1, "{key:?} produced {s:?}");
        }
    }

    // --- is_excluded ---

    fn ex(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn default_config_excludes_1password() {
        let exclude = ex(&["1Password", "Keychain Access"]);
        assert!(is_excluded("1Password", &exclude));
        assert!(is_excluded("Keychain Access", &exclude));
    }

    #[test]
    fn exclusion_is_substring_so_versioned_names_match() {
        let exclude = ex(&["1Password"]);
        assert!(is_excluded("1Password 8", &exclude));
        assert!(is_excluded("1Password – Browser", &exclude));
        assert!(is_excluded("1Password 7 Beta", &exclude));
    }

    #[test]
    fn exclusion_is_case_insensitive() {
        let exclude = ex(&["1Password", "Keychain Access"]);
        assert!(is_excluded("1PASSWORD", &exclude));
        assert!(is_excluded("1password", &exclude));
        assert!(is_excluded("KEYCHAIN ACCESS", &exclude));
    }

    #[test]
    fn non_excluded_apps_pass_through() {
        let exclude = ex(&["1Password", "Keychain Access"]);
        assert!(!is_excluded("Safari", &exclude));
        assert!(!is_excluded("Terminal", &exclude));
        assert!(!is_excluded("Cursor", &exclude));
    }

    #[test]
    fn empty_exclusion_list_excludes_nothing() {
        assert!(!is_excluded("1Password", &[]));
        assert!(!is_excluded("anything", &[]));
    }

    #[test]
    fn blank_entries_do_not_exclude_everything() {
        // A stray "" in config must not silently disable all capture.
        let exclude = ex(&["", "   "]);
        assert!(!is_excluded("Safari", &exclude));
        assert!(!is_excluded("1Password", &exclude));
    }

    #[test]
    fn whitespace_in_config_entries_is_tolerated() {
        let exclude = ex(&[" 1Password "]);
        assert!(is_excluded("1Password", &exclude));
    }
}