use super::Key;

// http://elixir.free-electrons.com/linux/latest/source/include/uapi/linux/input-event-codes.h
// into
// sed 's/#define /const /' | sed -E 's/[\t]+/: u32 = /' | awk '{ print $0 ";"; }'
const KEY_ESC: u32 = 1;
const KEY_1: u32 = 2;
const KEY_2: u32 = 3;
const KEY_3: u32 = 4;
const KEY_4: u32 = 5;
const KEY_5: u32 = 6;
const KEY_6: u32 = 7;
const KEY_7: u32 = 8;
const KEY_8: u32 = 9;
const KEY_9: u32 = 10;
const KEY_0: u32 = 11;
const KEY_MINUS: u32 = 12;
const KEY_EQUAL: u32 = 13;
const KEY_BACKSPACE: u32 = 14;
const KEY_TAB: u32 = 15;
const KEY_Q: u32 = 16;
const KEY_W: u32 = 17;
const KEY_E: u32 = 18;
const KEY_R: u32 = 19;
const KEY_T: u32 = 20;
const KEY_Y: u32 = 21;
const KEY_U: u32 = 22;
const KEY_I: u32 = 23;
const KEY_O: u32 = 24;
const KEY_P: u32 = 25;
const KEY_LEFTBRACE: u32 = 26;
const KEY_RIGHTBRACE: u32 = 27;
const KEY_ENTER: u32 = 28;
const KEY_LEFTCTRL: u32 = 29;
const KEY_A: u32 = 30;
const KEY_S: u32 = 31;
const KEY_D: u32 = 32;
const KEY_F: u32 = 33;
const KEY_G: u32 = 34;
const KEY_H: u32 = 35;
const KEY_J: u32 = 36;
const KEY_K: u32 = 37;
const KEY_L: u32 = 38;
const KEY_SEMICOLON: u32 = 39;
const KEY_APOSTROPHE: u32 = 40;
const KEY_GRAVE: u32 = 41;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_BACKSLASH: u32 = 43;
const KEY_Z: u32 = 44;
const KEY_X: u32 = 45;
const KEY_C: u32 = 46;
const KEY_V: u32 = 47;
const KEY_B: u32 = 48;
const KEY_N: u32 = 49;
const KEY_M: u32 = 50;
const KEY_COMMA: u32 = 51;
const KEY_DOT: u32 = 52;
const KEY_SLASH: u32 = 53;
const KEY_RIGHTSHIFT: u32 = 54;
const KEY_KPASTERISK: u32 = 55;
const KEY_LEFTALT: u32 = 56;
const KEY_SPACE: u32 = 57;
const KEY_CAPSLOCK: u32 = 58;
const KEY_F1: u32 = 59;
const KEY_F2: u32 = 60;
const KEY_F3: u32 = 61;
const KEY_F4: u32 = 62;
const KEY_F5: u32 = 63;
const KEY_F6: u32 = 64;
const KEY_F7: u32 = 65;
const KEY_F8: u32 = 66;
const KEY_F9: u32 = 67;
const KEY_F10: u32 = 68;
const KEY_NUMLOCK: u32 = 69;
const KEY_SCROLLLOCK: u32 = 70;
const KEY_KP7: u32 = 71;
const KEY_KP8: u32 = 72;
const KEY_KP9: u32 = 73;
const KEY_KPMINUS: u32 = 74;
const KEY_KP4: u32 = 75;
const KEY_KP5: u32 = 76;
const KEY_KP6: u32 = 77;
const KEY_KPPLUS: u32 = 78;
const KEY_KP1: u32 = 79;
const KEY_KP2: u32 = 80;
const KEY_KP3: u32 = 81;
const KEY_KP0: u32 = 82;
const KEY_KPDOT: u32 = 83;

// const KEY_ZENKAKUHANKAKU: u32 = 85;
const KEY_102ND: u32 = 86;
const KEY_F11: u32 = 87;
const KEY_F12: u32 = 88;
// const KEY_RO: u32 = 89;
// const KEY_KATAKANA: u32 = 90;
// const KEY_HIRAGANA: u32 = 91;
// const KEY_HENKAN: u32 = 92;
// const KEY_KATAKANAHIRAGANA: u32 = 93;
// const KEY_MUHENKAN: u32 = 94;
// const KEY_KPJPCOMMA: u32 = 95;
const KEY_KPENTER: u32 = 96;
const KEY_RIGHTCTRL: u32 = 97;
const KEY_KPSLASH: u32 = 98;
const KEY_SYSRQ: u32 = 99;
const KEY_RIGHTALT: u32 = 100;
// const KEY_LINEFEED: u32 = 101;
const KEY_HOME: u32 = 102;
const KEY_UP: u32 = 103;
const KEY_PAGEUP: u32 = 104;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_END: u32 = 107;
const KEY_DOWN: u32 = 108;
const KEY_PAGEDOWN: u32 = 109;
const KEY_INSERT: u32 = 110;
const KEY_DELETE: u32 = 111;
// const KEY_MACRO: u32 = 112;
 const KEY_MUTE: u32 = 113;
 const KEY_VOLUMEDOWN: u32 = 114;
 const KEY_VOLUMEUP: u32 = 115;
// const KEY_POWER: u32 = 116	/* SC System Power Down */;
// const KEY_KPEQUAL: u32 = 117;
// const KEY_KPPLUSMINUS: u32 = 118;
const KEY_PAUSE: u32 = 119;
// const KEY_SCALE: u32 = 120	/* AL Compiz Scale (Expose) */;

// const KEY_KPCOMMA: u32 = 121;
// const KEY_HANGEUL: u32 = 122;
// const KEY_HANGUEL: u32 = KEY_HANGEUL;
// const KEY_HANJA: u32 = 123;
// const KEY_YEN: u32 = 124;
const KEY_LEFTMETA: u32 = 125;
const KEY_RIGHTMETA: u32 = 126;
const KEY_COMPOSE: u32 = 127;
const KEY_NEXTSONG: u32 = 163;
const KEY_PLAYPAUSE: u32 = 164;
const KEY_PREVIOUSSONG: u32 = 165;
const KEY_STOPCD: u32 = 166;

pub fn key_convert(code: u32) -> Option<Key> {
    Some(match code {
        KEY_LEFTSHIFT => Key::LShiftKey,
        KEY_RIGHTSHIFT => Key::RShiftKey,
        KEY_LEFTALT => Key::LMenu,
        KEY_RIGHTALT => Key::RMenu,
        KEY_LEFTCTRL => Key::LControlKey,
        KEY_RIGHTCTRL => Key::RControlKey,
        KEY_COMPOSE => Key::Apps,
        KEY_ESC => Key::Escape,
        KEY_0 => Key::D0,
        KEY_1 => Key::D1,
        KEY_2 => Key::D2,
        KEY_3 => Key::D3,
        KEY_4 => Key::D4,
        KEY_5 => Key::D5,
        KEY_6 => Key::D6,
        KEY_7 => Key::D7,
        KEY_8 => Key::D8,
        KEY_9 => Key::D9,
        KEY_MINUS => Key::OemMinus,
        KEY_EQUAL => Key::Oemplus,
        KEY_BACKSPACE => Key::Back,
        KEY_TAB => Key::Tab,
        KEY_A => Key::A,
        KEY_B => Key::B,
        KEY_C => Key::C,
        KEY_D => Key::D,
        KEY_E => Key::E,
        KEY_F => Key::F,
        KEY_G => Key::G,
        KEY_H => Key::H,
        KEY_I => Key::I,
        KEY_J => Key::J,
        KEY_K => Key::K,
        KEY_L => Key::L,
        KEY_M => Key::M,
        KEY_N => Key::N,
        KEY_O => Key::O,
        KEY_P => Key::P,
        KEY_Q => Key::Q,
        KEY_R => Key::R,
        KEY_S => Key::S,
        KEY_T => Key::T,
        KEY_U => Key::U,
        KEY_V => Key::V,
        KEY_W => Key::W,
        KEY_X => Key::X,
        KEY_Y => Key::Y,
        KEY_Z => Key::Z,
        KEY_ENTER =>  Key::Enter,
        KEY_SEMICOLON => Key::Oem1,
        KEY_APOSTROPHE => Key::Oem7,
        KEY_GRAVE => Key::Oem3,
        KEY_BACKSLASH => Key::Oem5,
        KEY_COMMA => Key::Oemcomma,
        KEY_DOT => Key::OemPeriod,
        KEY_SLASH => Key::Oem2,
        KEY_SPACE => Key::Space,
        KEY_CAPSLOCK => Key::CapsLock,
        KEY_F1 => Key::F1,
        KEY_F2 => Key::F2,
        KEY_F3 => Key::F3,
        KEY_F4 => Key::F4,
        KEY_F5 => Key::F5,
        KEY_F6 => Key::F6,
        KEY_F7 => Key::F7,
        KEY_F8 => Key::F8,
        KEY_F9 => Key::F9,
        KEY_F10 => Key::F10,
        KEY_F11 => Key::F11,
        KEY_F12 => Key::F12,
        KEY_NUMLOCK => Key::NumLock,
        KEY_SCROLLLOCK => Key::Scroll,
        KEY_KPSLASH => Key::Divide,
        KEY_KPASTERISK => Key::Multiply,
        KEY_KPMINUS => Key::Subtract,
        KEY_KPPLUS => Key::Add,
        KEY_KPENTER => Key::Separator,
        KEY_KPDOT => Key::Decimal,
        KEY_SYSRQ => Key::PrintScreen,
        KEY_KP0 => Key::NumPad0,
        KEY_KP1 => Key::NumPad1,
        KEY_KP2 => Key::NumPad2,
        KEY_KP3 => Key::NumPad3,
        KEY_KP4 => Key::NumPad4,
        KEY_KP5 => Key::NumPad5,
        KEY_KP6 => Key::NumPad6,
        KEY_KP7 => Key::NumPad7,
        KEY_KP8 => Key::NumPad8,
        KEY_KP9 => Key::NumPad9,
        KEY_102ND => Key::Oem102,
        KEY_HOME => Key::Home,
        KEY_PAGEUP => Key::PageUp,
        KEY_PAGEDOWN => Key::PageDown,
        KEY_END => Key::End,
        KEY_LEFT => Key::Left,
        KEY_UP => Key::Up,
        KEY_DOWN => Key::Down,
        KEY_RIGHT => Key::Right,
        KEY_INSERT => Key::Insert,
        KEY_DELETE => Key::Delete,
        KEY_MUTE => Key::VolumeMute,
        KEY_VOLUMEDOWN => Key::VolumeDown,
        KEY_VOLUMEUP => Key::VolumeUp,
        KEY_PAUSE => Key::Pause,
        // KEY_KPCOMMA => ,
        // KEY_KPEQUAL => Key::Separator,
        KEY_LEFTMETA => Key::LWin,
        KEY_RIGHTMETA => Key::RWin,
        // KEY_POWER => "power",
        KEY_LEFTBRACE => Key::Oem4,
        KEY_RIGHTBRACE => Key::Oem6,
        KEY_NEXTSONG => Key::MediaNextTrack,
        KEY_PLAYPAUSE => Key::MediaPlayPause,
        KEY_PREVIOUSSONG => Key::MediaPreviousTrack,
        KEY_STOPCD => Key::MediaStop,
        _ => return None,
    })
}
