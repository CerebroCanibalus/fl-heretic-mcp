#!/usr/bin/env python3
"""Arregla la API de keybd_event en save_as.rs (windows 0.58)."""
import io

PATH = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-fl\src\save_as.rs"
s = io.open(PATH, encoding="utf-8").read()

# 1. imports: quitar KEYBD_EVENT_KEYUP
s = s.replace(
    "        keybd_event, KEYBD_EVENT_FLAGS, KEYBD_EVENT_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_MENU,\n        VK_S, VK_SHIFT,",
    "        keybd_event, KEYBD_EVENT_FLAGS, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_S, VK_SHIFT,",
)

# 2. anadir constantes y helper vk
anchor = """        SetForegroundWindow, ShowWindow, SW_RESTORE, WM_COMMAND, WM_GETTEXT, WM_SETTEXT,
    };
"""
s = s.replace(anchor, anchor + """
    /// `KEYEVENTF_KEYUP`. La crate no lo exporta con nombre, asi que va
    /// literal: es 0x0001 en la API de Win32.
    const KEYUP: KEYBD_EVENT_FLAGS = KEYBD_EVENT_FLAGS(1);
    const NONE: KEYBD_EVENT_FLAGS = KEYBD_EVENT_FLAGS(0);

    /// El vk de `VIRTUAL_KEY` a `u8`, que es lo que pide `keybd_event`.
    fn vk(k: VIRTUAL_KEY) -> u8 {
        k.0 as u8
    }
""", 1)

# 3. focus(): keybd_event con u8 y las constantes
s = s.replace(
    """            let none = KEYBD_EVENT_FLAGS(0);
            keybd_event(VK_MENU, 0, none, 0);
            keybd_event(VK_MENU, 0, KEYBD_EVENT_KEYUP, 0);""",
    """            keybd_event(vk(VK_MENU), 0, NONE, 0);
            keybd_event(vk(VK_MENU), 0, KEYUP, 0);""",
)

# 4. hotkey()
s = s.replace(
    """        let none = KEYBD_EVENT_FLAGS(0);
        unsafe {
            for &k in keys {
                keybd_event(k, 0, none, 0);
            }
            for &k in keys.iter().rev() {
                keybd_event(k, 0, KEYBD_EVENT_KEYUP, 0);
            }
        }""",
    """        unsafe {
            for &k in keys {
                keybd_event(vk(k), 0, NONE, 0);
            }
            for &k in keys.iter().rev() {
                keybd_event(vk(k), 0, KEYUP, 0);
            }
        }""",
)

io.open(PATH, "w", encoding="utf-8", newline="\n").write(s)
print("save_as.rs parcheado")
