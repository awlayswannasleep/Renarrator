//! Phase 2: глобальный перехват физических нажатий клавиатуры.
//!
//! Используем `rdev::listen` — под Windows внутри это низкоуровневый хук
//! `WH_KEYBOARD_LL`, поэтому события приходят даже когда приложение в фоне
//! и без фокуса. `rdev::Key` привязан к scancode, а не к текущей раскладке ОС,
//! что даёт нам независимость от RU/EN.
//!
//! События пересылаются в поток движка через `std::sync::mpsc`
//! (неблокирующая send-семантика идеально ложится на sync-колбэк хука;
//! отдельный tokio-рантайм здесь ничего не даёт — потребитель всё равно
//! выделенный поток).

use crate::layout_map::{map_key, MappedKey};
use rdev::{listen, EventType};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// Сообщения для потока-движка (buffer manager + диспетчер триггеров).
#[derive(Debug)]
pub enum EngineMessage {
    /// Физическое нажатие, уже нормализованное в (en, ru) символы.
    Key(MappedKey),
    /// Горячая перезагрузка настроек (после save_config): полный снапшот.
    Reload {
        triggers: Vec<crate::config::TriggerRule>,
        master_volume: f32,
        allow_overlap: bool,
    },
}

/// Запускает слушатель клавиатуры в отдельном OS-потоке.
///
/// * `tx` — канал в поток-движок.
/// * `paused` — общий флаг паузы; в паузе события просто отбрасываются
///   (буфер не пополняется, а таймаут 2с «добивает» устаревший ввод сам).
pub fn start_keyboard_hook(
    tx: Sender<EngineMessage>,
    paused: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        eprintln!("[hook] global keyboard listener started");
        if let Err(err) = listen(move |event| {
            // Отпускания клавиш и прочие события не интересны.
            if !matches!(event.event_type, EventType::KeyPress(_)) {
                return;
            }
            if paused.load(Ordering::Relaxed) {
                return;
            }
            if let EventType::KeyPress(key) = event.event_type {
                let mapped = map_key(key);
                // Модификаторы/стрелки даже не покидают хук.
                if matches!(mapped, MappedKey::Ignore) {
                    return;
                }
                if tx.send(EngineMessage::Key(mapped)).is_err() {
                    // Движок умер — приложение завершается, просто молчим.
                    eprintln!("[hook] engine channel closed");
                }
            }
        }) {
            eprintln!("[hook] rdev::listen failed: {err:?}");
        }
    })
}
