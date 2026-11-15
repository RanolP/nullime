//! Virtual key -> engine key (docs/architecture.md "키 모델"). Pure so it can be
//! tested without TSF.

use nullime_engine::{Key, KeyEvent};

const VK_BACK: u16 = 0x08;
const VK_RETURN: u16 = 0x0D;
const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12;
const VK_CAPITAL: u16 = 0x14;
const VK_SPACE: u16 = 0x20;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_LWIN: u16 = 0x5B;
const VK_RWIN: u16 = 0x5C;
const VK_LSHIFT: u16 = 0xA0;
const VK_RMENU: u16 = 0xA5;
const VK_A: u16 = b'A' as u16;
const VK_Z: u16 = b'Z' as u16;
const VK_0: u16 = b'0' as u16;
const VK_9: u16 = b'9' as u16;

/// US-QWERTY `[unshifted, shifted]` for the OEM punctuation keys.
const OEM: [(u16, [u8; 2]); 11] = [
    (0xBA, *b";:"),
    (0xBB, *b"=+"),
    (0xBC, *b",<"),
    (0xBD, *b"-_"),
    (0xBE, *b".>"),
    (0xBF, *b"/?"),
    (0xC0, *b"`~"),
    (0xDB, *b"[{"),
    (0xDC, *b"\\|"),
    (0xDD, *b"]}"),
    (0xDE, *b"'\""),
];

const SHIFTED_DIGITS: &[u8; 10] = b")!@#$%^&*(";

/// `None` for a bare modifier key (Shift, Ctrl, Alt, Win, Caps Lock): pressing
/// Shift on its way to Shift+Q must not flush the composition as `Other` would.
pub fn map_key(vk: u16, shift: bool, ctrl: bool, alt: bool) -> Option<KeyEvent> {
    let key = match vk {
        VK_SHIFT | VK_CONTROL | VK_MENU | VK_CAPITAL | VK_LWIN | VK_RWIN => return None,
        VK_LSHIFT..=VK_RMENU => return None,
        VK_A..=VK_Z => Key::Char(vk as u8 + 32),
        VK_0..=VK_9 => {
            let d = vk as u8 - b'0';
            Key::Char(if shift { SHIFTED_DIGITS[d as usize] } else { b'0' + d })
        }
        VK_SPACE => Key::Space,
        VK_RETURN => Key::Enter,
        VK_BACK => Key::Backspace,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        _ => match OEM.iter().find(|(code, _)| *code == vk) {
            Some((_, pair)) => Key::Char(pair[shift as usize]),
            None => Key::Other,
        },
    };
    Some(KeyEvent { key, shift, modifier: ctrl || alt })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Catches a layout table that hands the engine the unshifted digit or an
    // uppercase letter, which would break 두벌식 쌍자음 and D13 passthrough.
    #[test]
    fn maps_virtual_keys_to_us_qwerty_engine_keys() {
        let ev = |key, shift, modifier| Some(KeyEvent { key, shift, modifier });
        assert_eq!(map_key(b'1' as u16, true, false, false), ev(Key::Char(b'!'), true, false));
        assert_eq!(map_key(b'1' as u16, false, false, false), ev(Key::Char(b'1'), false, false));
        assert_eq!(map_key(b'Q' as u16, true, false, false), ev(Key::Char(b'q'), true, false));
        assert_eq!(map_key(0xBE, false, false, false), ev(Key::Char(b'.'), false, false));
        assert_eq!(map_key(0xDE, true, false, false), ev(Key::Char(b'"'), true, false));
        assert_eq!(map_key(b'C' as u16, false, true, false), ev(Key::Char(b'c'), false, true));
        assert_eq!(map_key(VK_SPACE, false, false, false), ev(Key::Space, false, false));
        assert_eq!(map_key(VK_LEFT, false, false, false), ev(Key::Left, false, false));
        assert_eq!(map_key(0x09, false, false, false), ev(Key::Other, false, false));
        assert_eq!(map_key(VK_SHIFT, true, false, false), None);
    }
}
