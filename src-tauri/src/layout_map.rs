//! Phase 2/3: отображение физических клавиш (rdev::Key, привязан к scancode)
//! в символы базовых раскладок EN (QWERTY) и RU (ЙЦУКЕН).
//!
//! Идея: одна физическая клавиша = один и тот же scancode независимо от
//! текущей раскладки ОС. Мы сами раскладываем её в пару символов:
//! `(en_char, ru_char)`. Движок ведёт два параллельных буфера, поэтому
//! слово «банан» срабатывает и при печати в RU-, и слово «banan» — в EN-раскладке.

use rdev::Key;

/// Результат интерпретации одного физического нажатия.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedKey {
    /// Клавиша несёт букву или цифру хотя бы в одной раскладке.
    /// `en` — символ QWERTY, `ru` — символ ЙЦУКЕН (если есть).
    Letter { en: char, ru: Option<char> },
    /// Пробел — разделитель слов внутри фразы (фраза срабатывает сама,
    /// суффикс-матчем на последней букве). Выделен отдельно от Reset: см. BufferManager.
    Space,
    /// Клавиша-сброс буфера: Enter, Backspace, Tab, Escape
    /// и «чистая» пунктуация (минус, равно, слеши).
    Reset,
    /// Клавиша, не влияющая на буфер (модификаторы, стрелки, F-клавиши и т.п.).
    Ignore,
}

/// Маппинг физической клавиши в пару символов (EN, RU).
/// Все буквы возвращаются в нижнем регистре (регистронезависимый детект).
pub fn map_key(key: Key) -> MappedKey {
    use MappedKey::Letter as L;
    match key {
        // --- Буквенный ряд: EN QWERTY + RU ЙЦУКЕН ---
        Key::KeyQ => L { en: 'q', ru: Some('й') },
        Key::KeyW => L { en: 'w', ru: Some('ц') },
        Key::KeyE => L { en: 'e', ru: Some('у') },
        Key::KeyR => L { en: 'r', ru: Some('к') },
        Key::KeyT => L { en: 't', ru: Some('е') },
        Key::KeyY => L { en: 'y', ru: Some('н') },
        Key::KeyU => L { en: 'u', ru: Some('г') },
        Key::KeyI => L { en: 'i', ru: Some('ш') },
        Key::KeyO => L { en: 'o', ru: Some('щ') },
        Key::KeyP => L { en: 'p', ru: Some('з') },
        Key::KeyA => L { en: 'a', ru: Some('ф') },
        Key::KeyS => L { en: 's', ru: Some('ы') },
        Key::KeyD => L { en: 'd', ru: Some('в') },
        Key::KeyF => L { en: 'f', ru: Some('а') },
        Key::KeyG => L { en: 'g', ru: Some('п') },
        Key::KeyH => L { en: 'h', ru: Some('р') },
        Key::KeyJ => L { en: 'j', ru: Some('о') },
        Key::KeyK => L { en: 'k', ru: Some('л') },
        Key::KeyL => L { en: 'l', ru: Some('д') },
        Key::KeyZ => L { en: 'z', ru: Some('я') },
        Key::KeyX => L { en: 'x', ru: Some('ч') },
        Key::KeyC => L { en: 'c', ru: Some('с') },
        Key::KeyV => L { en: 'v', ru: Some('м') },
        Key::KeyB => L { en: 'b', ru: Some('и') },
        Key::KeyN => L { en: 'n', ru: Some('т') },
        Key::KeyM => L { en: 'm', ru: Some('ь') },
        // Клавиши пунктуации, которые в ЙЦУКЕН являются БУКВАМИ.
        // EN-символ небуквенный — в EN-буфер не попадёт (см. BufferManager).
        Key::SemiColon => L { en: ';', ru: Some('ж') },
        Key::Quote => L { en: '\'', ru: Some('э') },
        Key::LeftBracket => L { en: '[', ru: Some('х') },
        Key::RightBracket => L { en: ']', ru: Some('ъ') },
        Key::Comma => L { en: ',', ru: Some('б') },
        Key::Dot => L { en: '.', ru: Some('ю') },
        Key::BackQuote => L { en: '`', ru: Some('ё') },

        // --- Цифры верхнего ряда: могут входить в слова и фразы ---
        // (без Shift, поэтому RU-спецсимволы «!\"№;…» сюда не попадают).
        Key::Num0 => L { en: '0', ru: Some('0') },
        Key::Num1 => L { en: '1', ru: Some('1') },
        Key::Num2 => L { en: '2', ru: Some('2') },
        Key::Num3 => L { en: '3', ru: Some('3') },
        Key::Num4 => L { en: '4', ru: Some('4') },
        Key::Num5 => L { en: '5', ru: Some('5') },
        Key::Num6 => L { en: '6', ru: Some('6') },
        Key::Num7 => L { en: '7', ru: Some('7') },
        Key::Num8 => L { en: '8', ru: Some('8') },
        Key::Num9 => L { en: '9', ru: Some('9') },

        // --- Пробел: разделитель слов / коммит фразы (не сброс!) ---
        Key::Space => MappedKey::Space,

        // --- Клавиши мгновенного сброса буфера ---
        // (пунктуация, не являющаяся буквами ни в одной раскладке)
        Key::Return
        | Key::Backspace
        | Key::Tab
        | Key::Escape
        | Key::Slash // en '/', ru '.' — пунктуация в обеих раскладках
        | Key::Minus
        | Key::Equal
        | Key::BackSlash => MappedKey::Reset,

        // Модификаторы, стрелки, F1-F12, numpad и всё прочее — игнорируем:
        // буфер не трогаем, таймер не сбрасываем.
        _ => MappedKey::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_lowercase_and_dual() {
        assert_eq!(
            map_key(Key::KeyF),
            MappedKey::Letter { en: 'f', ru: Some('а') }
        );
        assert_eq!(
            map_key(Key::KeyQ),
            MappedKey::Letter { en: 'q', ru: Some('й') }
        );
    }

    #[test]
    fn ru_letter_on_punctuation_key() {
        // «б» живёт на физической клавише запятой.
        assert_eq!(
            map_key(Key::Comma),
            MappedKey::Letter { en: ',', ru: Some('б') }
        );
        assert_eq!(
            map_key(Key::Dot),
            MappedKey::Letter { en: '.', ru: Some('ю') }
        );
    }

    #[test]
    fn reset_keys() {
        for k in [Key::Return, Key::Backspace, Key::Tab, Key::Escape, Key::Slash] {
            assert_eq!(map_key(k), MappedKey::Reset, "{k:?} must reset");
        }
    }

    #[test]
    fn digits_are_letters_in_both_layouts() {
        for (k, d) in [
            (Key::Num0, '0'),
            (Key::Num5, '5'),
            (Key::Num9, '9'),
        ] {
            assert_eq!(map_key(k), MappedKey::Letter { en: d, ru: Some(d) });
        }
    }

    #[test]
    fn space_is_separator_not_reset() {
        assert_eq!(map_key(Key::Space), MappedKey::Space);
    }

    #[test]
    fn modifiers_are_ignored() {
        for k in [Key::ShiftLeft, Key::ControlRight, Key::Alt, Key::F5, Key::UpArrow] {
            assert_eq!(map_key(k), MappedKey::Ignore, "{k:?} must be ignored");
        }
    }
}
