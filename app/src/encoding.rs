//! Text that is not UTF-8. Git hands files over as they are, and a file in an old one-byte
//! encoding (Windows-1251, for example) would show as rows of "�". The lines that are not
//! valid UTF-8 are read in the encoding chosen in Settings → Window instead.

use std::sync::atomic::{AtomicU8, Ordering};

/// The encodings to choose from; 0 leaves unknown bytes as "�".
pub const NAMES: [&str; 5] = ["Don't guess (show �)", "Windows-1251 (Cyrillic)", "Windows-1252 (Western)", "CP866 (DOS Cyrillic)", "KOI8-R"];

static FALLBACK: AtomicU8 = AtomicU8::new(0);

pub fn set_fallback(enc: u8) {
    FALLBACK.store(enc.min(NAMES.len() as u8 - 1), Ordering::Relaxed);
}

/// Bytes 0x80–0xBF of Windows-1251 (0xC0–0xFF are А…я in order).
const CP1251: &str = "ЂЃ‚ѓ„…†‡€‰Љ‹ЊЌЋЏђ‘’“”•–—\u{98}™љ›њќћџ\u{A0}ЎўЈ¤Ґ¦§Ё©Є«¬\u{AD}®Ї°±Ііґµ¶·ё№є»јЅѕї";
/// Bytes 0x80–0x9F of Windows-1252 (0xA0–0xFF are the same as in Unicode).
const CP1252: &str = "€\u{81}‚ƒ„…†‡ˆ‰Š‹Œ\u{8D}Ž\u{8F}\u{90}‘’“”•–—˜™š›œ\u{9D}žŸ";
/// Bytes 0xB0–0xDF and 0xF0–0xFF of CP866 (0x80–0xAF are А…п, 0xE0–0xEF are р…я).
const CP866_BOX: &str = "░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀";
const CP866_TAIL: &str = "ЁёЄєЇїЎў°∙·√№¤■\u{A0}";
/// Bytes 0x80–0xFF of KOI8-R.
const KOI8R: &str = "─│┌┐└┘├┤┬┴┼▀▄█▌▐░▒▓⌠■∙√≈≤≥\u{A0}⌡°²·÷═║╒ё╓╔╕╖╗╘╙╚╛╜╝╞╟╠╡Ё╢╣╤╥╦╧╨╩╪╫╬©юабцдефгхийклмнопярстужвьызшэщчъЮАБЦДЕФГХИЙКЛМНОПЯРСТУЖВЬЫЗШЭЩЧЪ";

fn nth(table: &str, k: usize) -> char {
    table.chars().nth(k).unwrap_or('\u{FFFD}')
}

/// One byte of the encoding `enc` (1…4, see [`NAMES`]) as a character.
pub fn byte_to_char(enc: u8, b: u8) -> char {
    if b < 0x80 {
        return b as char;
    }
    let k = b as usize;
    match enc {
        1 if b >= 0xC0 => char::from_u32(0x410 + (k as u32 - 0xC0)).unwrap_or('\u{FFFD}'),
        1 => nth(CP1251, k - 0x80),
        2 if b >= 0xA0 => b as char,
        2 => nth(CP1252, k - 0x80),
        3 if b < 0xB0 => char::from_u32(0x410 + (k as u32 - 0x80)).unwrap_or('\u{FFFD}'),
        3 if b < 0xE0 => nth(CP866_BOX, k - 0xB0),
        3 if b < 0xF0 => char::from_u32(0x440 + (k as u32 - 0xE0)).unwrap_or('\u{FFFD}'),
        3 => nth(CP866_TAIL, k - 0xF0),
        4 => nth(KOI8R, k - 0x80),
        _ => '\u{FFFD}',
    }
}

/// Text from bytes: UTF-8 where it is UTF-8; a line that is not is read in the encoding `enc`
/// (0: its unknown bytes become "�", as before).
pub fn decode_with(enc: u8, bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) if enc == 0 => String::from_utf8_lossy(bytes).into_owned(),
        Err(_) => {
            let mut out = String::with_capacity(bytes.len() + bytes.len() / 4);
            for line in bytes.split_inclusive(|b| *b == b'\n') {
                match std::str::from_utf8(line) {
                    Ok(s) => out.push_str(s),
                    Err(_) => out.extend(line.iter().map(|b| byte_to_char(enc, *b))),
                }
            }
            out
        }
    }
}

/// What git printed, as text (see [`decode_with`]; the encoding is the one from the settings).
pub fn decode(bytes: &[u8]) -> String {
    decode_with(FALLBACK.load(Ordering::Relaxed), bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_whole() {
        assert_eq!((CP1251.chars().count(), CP1252.chars().count(), CP866_BOX.chars().count(), CP866_TAIL.chars().count(), KOI8R.chars().count()), (64, 32, 48, 16, 128));
    }

    #[test]
    fn reads_old_encodings_and_leaves_utf8_alone() {
        // "Привет, мир" in each encoding.
        let cp1251 = [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, 0x2C, 0x20, 0xEC, 0xE8, 0xF0];
        let cp866 = [0x8F, 0xE0, 0xA8, 0xA2, 0xA5, 0xE2, 0x2C, 0x20, 0xAC, 0xA8, 0xE0];
        let koi8 = [0xF0, 0xD2, 0xC9, 0xD7, 0xC5, 0xD4, 0x2C, 0x20, 0xCD, 0xC9, 0xD2];
        assert_eq!(decode_with(1, &cp1251), "Привет, мир");
        assert_eq!(decode_with(3, &cp866), "Привет, мир");
        assert_eq!(decode_with(4, &koi8), "Привет, мир");
        assert_eq!(decode_with(2, &[0x63, 0x61, 0x66, 0xE9, 0x20, 0x80, 0x35]), "café €5");
        assert_eq!(decode_with(1, &[0xB9, 0xA8, 0xB8]), "№Ёё");
        // Without a choice the unknown bytes stay marked.
        assert!(decode_with(0, &cp1251).contains('\u{FFFD}'));
        // UTF-8 is never touched, and in a mixed text only the lines that need it are read anew.
        assert_eq!(decode_with(1, "Привет ✓".as_bytes()), "Привет ✓");
        let mut mixed = b"+ok \xE2\x9C\x93\n-".to_vec();
        mixed.extend_from_slice(&cp1251);
        mixed.extend_from_slice(b"\n context\n");
        assert_eq!(decode_with(1, &mixed), "+ok ✓\n-Привет, мир\n context\n");
    }
}
