//! Phase 3: менеджер буфера ввода и детектор триггер-слов.
//!
//! Правила (по спецификации + фразы):
//! * Регистронезависимость — все буквы уже приведены к нижнему регистру в layout_map.
//! * Таймаут: пауза между нажатиями > 2.0 с → полная очистка буфера.
//! * Сброс: Enter / Backspace / Tab / Escape / «чистая» пунктуация → очистка.
//! * Цифры (без Shift) — такие же символы слова, как и буквы.
//! * Одиночное слово детектится сразу (суффикс буфера).
//! * Фраза «слово слово» срабатывает СРАЗУ по суффиксу — финальный пробел
//!   не нужен. Пробел внутри фразы — это разделитель слов, который идёт
//!   в буфер, чтобы дотянуть до совпадения хвоста.
//! * После срабатывания буфер очищается → исключено дублирование.
//!
//! Двойной буфер: физическая клавиша даёт символ EN и/или RU раскладки;
//! буква/цифра попадает в буфер только если она буквенная для этого алфавита
//! (запятая клавиша = RU «б» попадёт в RU-буфер, но не замусорит EN-буфер).

use crate::layout_map::MappedKey;
use std::time::{Duration, Instant};

/// Таймаут ввода по спецификации — 2.0 секунды.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(2000);
/// Защитный потолок длины буфера (длиннейшие слова всё равно короче).
const MAX_BUFFER_LEN: usize = 64;

pub struct BufferManager {
    en_buf: String,
    ru_buf: String,
    last_key_at: Option<Instant>,
    timeout: Duration,
    /// Одиночные слова (trigger_id, слово) — срабатывают сразу по суффиксу.
    words: Vec<(String, String)>,
    /// Фразы из 2+ слов (trigger_id, фраза с пробелами) — суффикс-матч, как и слова.
    phrases: Vec<(String, String)>,
    /// Есть ли вообще фразы — определяет семантику пробела.
    has_phrases: bool,
}

impl BufferManager {
    pub fn new(timeout: Duration) -> Self {
        Self {
            en_buf: String::new(),
            ru_buf: String::new(),
            last_key_at: None,
            timeout,
            words: Vec::new(),
            phrases: Vec::new(),
            has_phrases: false,
        }
    }

    /// Перезагрузить словарь: `triggers` = (trigger_id, words).
    /// Элемент с пробелом — это фраза (2+ слов); без пробела — одиночное слово.
    pub fn set_triggers(&mut self, triggers: &[(String, Vec<String>)]) {
        self.words.clear();
        self.phrases.clear();
        for (id, words) in triggers {
            for w in words {
                // Нормализуем пробелы (двойные → одинарные), нижний регистр.
                let norm = w.split_whitespace().collect::<Vec<_>>().join(" ");
                if norm.is_empty() {
                    continue;
                }
                if norm.contains(' ') {
                    self.phrases.push((id.clone(), norm));
                } else {
                    self.words.push((id.clone(), norm));
                }
            }
        }
        self.has_phrases = !self.phrases.is_empty();
        self.clear();
    }

    pub fn clear(&mut self) {
        self.en_buf.clear();
        self.ru_buf.clear();
    }

    /// Обработать одно нажатие. Возвращает `Some(trigger_id)` при совпадении.
    /// `now` инжектируется снаружи — это делает таймауты полностью тестируемыми.
    pub fn handle_key(&mut self, key: &MappedKey, now: Instant) -> Option<String> {
        match key {
            MappedKey::Letter { en, ru } => {
                // Таймаут: пауза > timeout обнуляет накопленное.
                if self
                    .last_key_at
                    .is_some_and(|t| now.duration_since(t) > self.timeout)
                {
                    self.clear();
                }
                self.last_key_at = Some(now);

                if en.is_alphanumeric() {
                    push_capped(&mut self.en_buf, *en);
                }
                if let Some(ru) = ru {
                    if ru.is_alphanumeric() {
                        push_capped(&mut self.ru_buf, *ru);
                    }
                }

                // Суффикс-матч одиночных слов и фраз — срабатывает СРАЗУ
                // по последней букве, без ожидания финального пробела.
                self.find_match().map(|id| {
                    self.clear();
                    id
                })
            }
            MappedKey::Space => {
                // Таймаут действует и на пробел: пауза > timeout обнуляет буфер,
                // иначе фразу можно было бы «растянуть» на минуты.
                if self
                    .last_key_at
                    .is_some_and(|t| now.duration_since(t) > self.timeout)
                {
                    self.clear();
                }
                self.last_key_at = Some(now);

                // Пробел — внутренний разделитель слов фразы: кладём его в буфер,
                // чтобы хвост («слово слово …») мог дотянуть до совпадения.
                // (Благодаря суффикс-матчу одиночное слово уже сработало раньше,
                // поэтому «висящего» одиночного слова перед пробелом нет.)
                if self.has_phrases {
                    for buf in [&mut self.en_buf, &mut self.ru_buf] {
                        if !buf.is_empty() && !buf.ends_with(' ') {
                            push_capped(buf, ' ');
                        }
                    }
                    // Практически недостижимо (фраза уже сработала на последней
                    // букве и буфер очищен), но для консистентности с веткой
                    // Letter матч тоже очищает буфер.
                    self.find_match().map(|id| {
                        self.clear();
                        id
                    })
                } else {
                    // Фраз нет — пробел просто сбрасывает «висящее» слово.
                    self.clear();
                    None
                }
            }
            MappedKey::Reset => {
                self.clear();
                self.last_key_at = None;
                None
            }
            MappedKey::Ignore => None,
        }
    }

    /// Самое длинное совпадение-суффикс среди одиночных слов и фраз
    /// (длиннейший матч выигрывает, чтобы «бананы» не перехватывалось «анан»).
    fn find_match(&self) -> Option<String> {
        let mut best: Option<&(String, String)> = None;
        for entry in self.words.iter().chain(self.phrases.iter()) {
            let target = &entry.1;
            let hit =
                self.en_buf.ends_with(target.as_str()) || self.ru_buf.ends_with(target.as_str());
            if hit && best.is_none_or(|b| target.len() > b.1.len()) {
                best = Some(entry);
            }
        }
        best.map(|(id, _)| id.clone())
    }

    #[cfg(test)]
    fn buffers(&self) -> (&str, &str) {
        (&self.en_buf, &self.ru_buf)
    }
}

/// Добавить символ, удерживая длину буфера <= MAX_BUFFER_LEN (UTF-8 безопасно).
fn push_capped(buf: &mut String, c: char) {
    buf.push(c);
    let len = buf.chars().count();
    if len > MAX_BUFFER_LEN {
        let cut = buf
            .char_indices()
            .nth(len - MAX_BUFFER_LEN)
            .map(|(i, _)| i)
            .unwrap_or(buf.len());
        buf.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout_map::map_key;
    use rdev::Key;

    const T: Duration = Duration::from_millis(2000);

    fn manager(words: &[(&str, &[&str])]) -> BufferManager {
        let mut m = BufferManager::new(T);
        let owned: Vec<(String, Vec<String>)> = words
            .iter()
            .map(|(id, ws)| (id.to_string(), ws.iter().map(|w| w.to_string()).collect()))
            .collect();
        m.set_triggers(&owned);
        m
    }

    fn letter(en: char, ru: Option<char>) -> MappedKey {
        MappedKey::Letter { en, ru }
    }

    fn type_en(m: &mut BufferManager, text: &str, t0: Instant) -> Option<String> {
        let mut out = None;
        for (i, c) in text.chars().enumerate() {
            out = m.handle_key(&letter(c, None), t0 + Duration::from_millis(100 * i as u64));
        }
        out
    }

    /// Набрать строку, где пробел — реальный MappedKey::Space (для фраз).
    fn type_phrase(m: &mut BufferManager, text: &str, t0: Instant) -> Option<String> {
        let mut out = None;
        for (i, c) in text.chars().enumerate() {
            let key = if c == ' ' {
                MappedKey::Space
            } else {
                letter(c, None)
            };
            out = m.handle_key(&key, t0 + Duration::from_millis(100 * i as u64));
        }
        out
    }

    fn press_space(m: &mut BufferManager, at: Instant) -> Option<String> {
        m.handle_key(&MappedKey::Space, at)
    }

    #[test]
    fn detects_en_word_once_and_clears() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["banan"])]);
        assert_eq!(type_en(&mut m, "banan", t0), Some("trg".into()));
        // Буфер очищен: повторное срабатывание без повторного ввода невозможно.
        assert_eq!(m.buffers(), ("", ""));
        assert_eq!(m.handle_key(&letter('a', None), t0 + T), None);
    }

    #[test]
    fn detects_ru_word_by_physical_keys() {
        let t0 = Instant::now();
        // «банан» физически: Comma(б) F(а) Y(н) F(а) Y(н)
        let mut m = manager(&[("trg", &["банан"])]);
        let keys = [Key::Comma, Key::KeyF, Key::KeyY, Key::KeyF, Key::KeyY];
        let mut hit = None;
        for (i, k) in keys.iter().enumerate() {
            hit = m.handle_key(&map_key(*k), t0 + Duration::from_millis(80 * i as u64));
        }
        assert_eq!(hit, Some("trg".into()));
    }

    #[test]
    fn timeout_clears_buffer() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["banan"])]);
        type_en(&mut m, "ban", t0);
        assert_eq!(m.buffers().0, "ban");
        // Пауза 2.5с → следующее нажатие начинает с чистого буфера.
        let t1 = t0 + Duration::from_millis(2500);
        assert_eq!(m.handle_key(&letter('a', None), t1), None);
        assert_eq!(m.buffers().0, "a");
    }

    #[test]
    fn reset_keys_clear_instantly() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["banan"])]);
        type_en(&mut m, "bana", t0);
        assert_eq!(
            m.handle_key(&MappedKey::Reset, t0 + Duration::from_millis(400)),
            None
        );
        assert_eq!(m.buffers(), ("", ""));
        // После сброса «nan» уже не достраивается до «banan».
        type_en(&mut m, "nan", t0 + Duration::from_millis(500));
        assert_eq!(m.buffers().0, "nan");
    }

    #[test]
    fn suffix_match_inside_longer_input() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["banan"])]);
        assert_eq!(type_en(&mut m, "xxbanan", t0), Some("trg".into()));
    }

    #[test]
    fn longest_word_wins() {
        let t0 = Instant::now();
        let mut m = manager(&[("short", &["анан"]), ("long", &["банан"])]);
        let keys = [Key::Comma, Key::KeyF, Key::KeyY, Key::KeyF, Key::KeyY];
        let mut hit = None;
        for (i, k) in keys.iter().enumerate() {
            hit = m.handle_key(&map_key(*k), t0 + Duration::from_millis(80 * i as u64));
        }
        assert_eq!(hit, Some("long".into()));
    }

    #[test]
    fn no_match_for_partial_word() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["banana"])]);
        assert_eq!(type_en(&mut m, "banan", t0), None);
    }

    #[test]
    fn ignore_keys_do_not_touch_buffer_or_timer() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["ban"])]);
        type_en(&mut m, "ba", t0);
        // Ignore между буквами не сбрасывает и не продлевает жизнь буфера.
        m.handle_key(&MappedKey::Ignore, t0 + Duration::from_millis(300));
        // 2.2с от последней буквы → таймаут уже сработал, «ban» не собирается.
        let hit = m.handle_key(&letter('n', None), t0 + Duration::from_millis(2200));
        assert_eq!(hit, None);
        assert_eq!(m.buffers().0, "n");
    }

    // --------------------- Фразы (слово + пробел + слово) ---------------------

    #[test]
    fn phrase_fires_on_last_letter_without_trailing_space() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["banana apple"])]);
        // Фраза срабатывает СРАЗУ на последней букве — финальный пробел не нужен.
        assert_eq!(type_phrase(&mut m, "banana apple", t0), Some("trg".into()));
        // Буфер очищен — повторного срабатывания нет.
        assert_eq!(m.buffers(), ("", ""));
    }

    #[test]
    fn phrase_ru_fires_on_last_letter() {
        let t0 = Instant::now();
        // «банан яблоко» физически. Пробел — MappedKey::Space.
        let mut m = manager(&[("trg", &["банан яблоко"])]);
        // б а н а н
        let w1 = [Key::Comma, Key::KeyF, Key::KeyY, Key::KeyF, Key::KeyY];
        for (i, k) in w1.iter().enumerate() {
            m.handle_key(&map_key(*k), t0 + Duration::from_millis(80 * i as u64));
        }
        m.handle_key(&MappedKey::Space, t0 + Duration::from_millis(500));
        // я б л о к о — фраза срабатывает на последней букве «о».
        let w2 = [
            Key::KeyZ, // я
            Key::Comma, // б
            Key::KeyK, // л
            Key::KeyJ, // о
            Key::KeyR, // к
            Key::KeyJ, // о
        ];
        let last = w2.len() - 1;
        for (i, k) in w2.iter().enumerate() {
            let hit = m.handle_key(&map_key(*k), t0 + Duration::from_millis(600 + 80 * i as u64));
            if i < last {
                assert_eq!(hit, None, "прематурное срабатывание на {i}-й букве");
            } else {
                assert_eq!(hit, Some("trg".into()), "фраза должна сработать на последней букве");
            }
        }
    }

    #[test]
    fn phrase_suffix_inside_longer_input() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["banana apple"])]);
        // «xx banana apple» — суффикс-матч: сработает на последней букве.
        assert_eq!(type_phrase(&mut m, "xxbanana apple", t0), Some("trg".into()));
    }

    #[test]
    fn single_space_after_word_does_not_commit_phrase() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["banana apple"])]);
        // Только первое слово + пробел → ещё не фраза, срабатывания нет.
        assert_eq!(type_phrase(&mut m, "banana ", t0), None);
        // Буфер держит «banana » (пробел как разделитель).
        assert_eq!(m.buffers().0, "banana ");
    }

    #[test]
    fn space_respects_timeout() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["banana apple"])]);
        type_phrase(&mut m, "banana ", t0);
        // Пауза 3с → буфер сброшен; пробел теперь начинает с чистого листа.
        assert_eq!(
            press_space(&mut m, t0 + Duration::from_millis(3000)),
            None
        );
        assert_eq!(m.buffers().0, "");
    }

    #[test]
    fn single_words_still_work_when_phrases_present() {
        let t0 = Instant::now();
        // И слово, и фраза у разных триггеров.
        let mut m = manager(&[("w", &["boom"]), ("p", &["red alert"])]);
        assert_eq!(type_en(&mut m, "boom", t0), Some("w".into()));
        assert_eq!(type_phrase(&mut m, "red alert", t0 + Duration::from_millis(600)), Some("p".into()));
    }

    // --------------------- Цифры в словах и фразах --------------------

    #[test]
    fn digits_trigger_word() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["123"])]);
        assert_eq!(type_en(&mut m, "123", t0), Some("trg".into()));
    }

    #[test]
    fn digits_mixed_with_letters() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["abc123"])]);
        assert_eq!(type_en(&mut m, "abc123", t0), Some("trg".into()));
    }

    #[test]
    fn digits_in_phrase() {
        let t0 = Instant::now();
        let mut m = manager(&[("trg", &["phase 2 fire"])]);
        assert_eq!(type_phrase(&mut m, "phase 2 fire", t0), Some("trg".into()));
    }
}
