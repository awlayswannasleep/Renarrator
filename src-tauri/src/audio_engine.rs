//! Phase 4: аудио-движок на rodio 0.22.
//!
//! Архитектура: выделенный поток владеет устройством вывода (`MixerDeviceSink`)
//! и списком активных `Player`ов; команды приходят по `std::sync::mpsc`.
//! Так обходим Send-ограничения аудио-дескрипторов и получаем
//! предсказуемую точку синхронизации всех воспроизведений.
//!
//! * Полифония: `allow_overlap = true` → новые звуки накладываются на текущие;
//!   `false` → всё играющее останавливается перед новым звуком.
//! * Громкость итоговая = `master_volume * sound_volume` (каждая 0.0..=1.0).
//! * Взвешенный случайный выбор: P(i) = weight_i / Σweights (нормализация).

use crate::config::SoundOption;
use rand::Rng;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use std::fs::File;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::{self, JoinHandle};

/// Команды аудио-движку.
#[derive(Debug)]
pub enum AudioCommand {
    /// Сработал триггер: выбрать один звук по весам и проиграть.
    PlayTrigger {
        sounds: Vec<SoundOption>,
        master_volume: f32,
        allow_overlap: bool,
    },
    /// Кнопка «Тест» в UI: проиграть конкретный файл с заданной громкостью.
    TestSound {
        path: String,
        volume: f32,
        master_volume: f32,
    },
    /// Остановить всё воспроизведение.
    StopAll,
}

/// Публичная ручка движка: клонируемый Sender + поток.
#[derive(Clone)]
pub struct AudioEngineHandle {
    tx: Sender<AudioCommand>,
}

impl AudioEngineHandle {
    pub fn send(&self, cmd: AudioCommand) {
        // Если поток умер (нет аудиоустройства) — просто логируем, не падаем.
        if self.tx.send(cmd).is_err() {
            eprintln!("[audio] engine thread is not running");
        }
    }
}

/// Запускает аудио-поток и возвращает ручку для команд.
pub fn start_audio_engine() -> (AudioEngineHandle, JoinHandle<()>) {
    let (tx, rx) = std::sync::mpsc::channel::<AudioCommand>();
    let thread = thread::spawn(move || engine_loop(rx));
    (AudioEngineHandle { tx }, thread)
}

fn engine_loop(rx: Receiver<AudioCommand>) {
    // Устройство вывода открываем один раз на всю жизнь потока.
    let sink_handle = match DeviceSinkBuilder::open_default_sink() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[audio] cannot open default audio device: {e}");
            // Дренируем канал, чтобы отправители не видели обрыв.
            while rx.recv().is_ok() {}
            return;
        }
    };
    eprintln!("[audio] engine started");

    let mut active: Vec<Player> = Vec::new();
    let mut rng = rand::rng();

    while let Ok(cmd) = rx.recv() {
        // Вычищаем доигравшие плееры (Drop у Player останавливает звук).
        active.retain(|p| !p.empty());
        match cmd {
            AudioCommand::PlayTrigger {
                sounds,
                master_volume,
                allow_overlap,
            } => {
                if let Some(sound) = pick_weighted(&sounds, &mut rng) {
                    if !allow_overlap {
                        stop_all(&mut active);
                    }
                    play_one(&sink_handle, &mut active, &sound.path, sound.volume * master_volume);
                } else {
                    eprintln!("[audio] trigger fired but no playable sounds configured");
                }
            }
            AudioCommand::TestSound {
                path,
                volume,
                master_volume,
            } => {
                // Тест не прерывает текущее воспроизведение — это предпрослушка.
                play_one(&sink_handle, &mut active, &path, volume * master_volume);
            }
            AudioCommand::StopAll => stop_all(&mut active),
        }
    }
}

fn stop_all(active: &mut Vec<Player>) {
    for p in active.iter() {
        p.stop();
    }
    active.clear();
}

fn play_one(handle: &MixerDeviceSink, active: &mut Vec<Player>, path: &str, volume: f32) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[audio] cannot open '{path}': {e}");
            return;
        }
    };
    match Decoder::try_from(file) {
        Ok(source) => {
            let player = Player::connect_new(handle.mixer());
            player.set_volume(volume.clamp(0.0, 1.0));
            player.append(source);
            active.push(player);
        }
        Err(e) => eprintln!("[audio] cannot decode '{path}': {e}"),
    }
}

/// Взвешенный случайный выбор одного звука (нормализация весов в 100%).
/// Чистая функция с инжектируемым RNG — полностью тестируема.
///
/// Краевые случаи: пустой список → `None`; все веса нулевые → первый звук
/// (детерминированный fallback вместо «ничего не играть»).
pub fn pick_weighted<'a, R: Rng + ?Sized>(
    sounds: &'a [SoundOption],
    rng: &mut R,
) -> Option<&'a SoundOption> {
    if sounds.is_empty() {
        return None;
    }
    let total: u64 = sounds.iter().map(|s| u64::from(s.weight)).sum();
    if total == 0 {
        return sounds.first();
    }
    let mut roll = rng.random_range(0..total);
    for s in sounds {
        let w = u64::from(s.weight);
        if roll < w {
            return Some(s);
        }
        roll -= w;
    }
    sounds.last()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    fn snd(path: &str, weight: u32) -> SoundOption {
        SoundOption {
            path: path.to_string(),
            volume: 1.0,
            weight,
        }
    }

    #[test]
    fn empty_list_returns_none() {
        let mut rng = SmallRng::seed_from_u64(1);
        assert!(pick_weighted(&[], &mut rng).is_none());
    }

    #[test]
    fn single_sound_always_picked() {
        let mut rng = SmallRng::seed_from_u64(2);
        let sounds = vec![snd("a.mp3", 5)];
        for _ in 0..50 {
            assert_eq!(pick_weighted(&sounds, &mut rng).unwrap().path, "a.mp3");
        }
    }

    #[test]
    fn zero_weight_never_picked() {
        let mut rng = SmallRng::seed_from_u64(3);
        let sounds = vec![snd("never.mp3", 0), snd("always.mp3", 10)];
        for _ in 0..200 {
            assert_eq!(pick_weighted(&sounds, &mut rng).unwrap().path, "always.mp3");
        }
    }

    #[test]
    fn all_zero_weights_falls_back_to_first() {
        let mut rng = SmallRng::seed_from_u64(4);
        let sounds = vec![snd("a.mp3", 0), snd("b.mp3", 0)];
        assert_eq!(pick_weighted(&sounds, &mut rng).unwrap().path, "a.mp3");
    }

    #[test]
    fn distribution_matches_normalized_weights() {
        // 60 / 30 / 10 → нормализация в 60% / 30% / 10%.
        let mut rng = SmallRng::seed_from_u64(42);
        let sounds = vec![snd("a.mp3", 60), snd("b.mp3", 30), snd("c.mp3", 10)];
        let n = 20_000;
        let mut counts = [0usize; 3];
        for _ in 0..n {
            let picked = pick_weighted(&sounds, &mut rng).unwrap();
            let idx = sounds
                .iter()
                .position(|s| s.path == picked.path)
                .expect("picked sound must come from the list");
            counts[idx] += 1;
        }
        let p = |i: usize| counts[i] as f64 / n as f64;
        assert!((p(0) - 0.60).abs() < 0.03, "p(0)={}", p(0));
        assert!((p(1) - 0.30).abs() < 0.03, "p(1)={}", p(1));
        assert!((p(2) - 0.10).abs() < 0.02, "p(2)={}", p(2));
    }
}
