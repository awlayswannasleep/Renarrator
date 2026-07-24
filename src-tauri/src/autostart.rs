//! Phase 6: автозагрузка при старте Windows.
//!
//! Классический способ: значение в
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! Не требует прав администратора (ветка текущего пользователя).

use std::env;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Renarrator";

/// Прописать текущий exe в автозагрузку (путь в кавычках — на случай пробелов).
pub fn enable() -> std::io::Result<()> {
    let exe = env::current_exe()?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_SUBKEY)?;
    let quoted = format!("\"{}\"", exe.display());
    key.set_value(VALUE_NAME, &quoted)
}

/// Убрать из автозагрузки (отсутствие значения — не ошибка).
pub fn disable() -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_SUBKEY)?;
    match key.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn is_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_SUBKEY)
        .and_then(|k| k.get_value::<String, _>(VALUE_NAME))
        .is_ok()
}

pub fn set_enabled(enabled: bool) -> std::io::Result<()> {
    if enabled {
        enable()
    } else {
        disable()
    }
}
