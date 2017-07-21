// we try to parse these as windows keycodes
mod keys;
pub use self::keys::Keys as Key;
mod linux;
mod qcode;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modifier {
    Alt = 0x0001,
    Ctrl = 0x0002,
    Shift = 0x0004,
    Win = 0x0008,
}
const NOREPEAT: u32 = 0x4000;

impl Key {
    fn modifier(&self) -> Option<Modifier> {
        Some(match *self {
            Key::LMenu | Key::RMenu => Modifier::Alt,
            Key::LControlKey | Key::RControlKey => Modifier::Ctrl,
            Key::LShiftKey | Key::RShiftKey => Modifier::Shift,
            Key::LWin | Key::RWin => Modifier::Win,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    modifiers: Vec<Modifier>,
    no_repeat: bool, // FIXME: implement this
    key: Key,
}

impl KeyBinding {
    pub fn new(modifiers: Vec<Modifier>, key: Key, no_repeat: bool) -> KeyBinding {
        KeyBinding { modifiers, no_repeat, key }
    }

    pub fn matches(&self, modifiers: &[Modifier], key: Key) -> bool {
        key == self.key && self.modifiers.iter().all(|x| modifiers.contains(x))
    }

    pub fn to_windows(&self) -> (u32, u32) {
        let base = if self.no_repeat { NOREPEAT } else { 0 };
        (self.modifiers.iter().fold(base, |sum, &x| sum | (x as u32)), self.key as u32)
    }
}

pub struct KeyboardState<'a> {
    modifiers: Vec<Modifier>,
    bindings: &'a [KeyBinding],
}

impl<'a> KeyboardState<'a> {
    pub fn new(bindings: &'a [KeyBinding]) -> KeyboardState {
        KeyboardState {
            modifiers: Vec::new(),
            bindings,
        }
    }

    pub fn input_linux(&mut self, code: u32, down: bool) -> Option<(Vec<usize>, &'static str)> {
        linux::key_convert(code).map(|k| {
            let mut bindings = Vec::new();
            if let Some(m) = k.modifier() {
                if down {
                    if !self.modifiers.contains(&m) {
                        self.modifiers.push(m);
                    }
                } else {
                    if let Some(i) = self.modifiers.iter().position(|&x| x == m) {
                        self.modifiers.swap_remove(i);
                    }
                }
            } else if down {
                bindings.extend(self.bindings.iter().enumerate()
                                .filter(|&(_, b)| b.matches(&self.modifiers, k))
                                .map(|(i, _)| i));
            }

            (bindings, qcode::key_convert(k))
        })
    }
}
