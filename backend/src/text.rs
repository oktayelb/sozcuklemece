use std::collections::HashMap;

const TURKISH_ONLY_LETTERS: [char; 12] =
    ['ç', 'ğ', 'ı', 'ö', 'ş', 'ü', 'Ç', 'Ğ', 'İ', 'Ö', 'Ş', 'Ü'];
const TURKISH_WORDS: &str = include_str!("../resources/turkish_words.txt");

pub fn turkish_lower(text: &str) -> String {
    text.replace('I', "ı").replace('İ', "i").to_lowercase()
}

fn is_kept_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '-' || character.is_whitespace()
}

pub fn normalize_text(text: &str) -> String {
    let mut kept = String::new();
    for character in turkish_lower(text).chars() {
        if is_kept_character(character) {
            kept.push(character);
        } else {
            kept.push(' ');
        }
    }
    let words: Vec<&str> = kept.split_whitespace().collect();
    words.join(" ")
}

pub fn compact_text(text: &str) -> String {
    normalize_text(text).replace([' ', '-'], "")
}

pub fn fold_turkish_letters(text: &str) -> String {
    let mut folded = String::new();
    for character in text.chars() {
        let plain = match character {
            'ç' => 'c',
            'ğ' => 'g',
            'ı' => 'i',
            'ö' => 'o',
            'ş' => 's',
            'ü' => 'u',
            other => other,
        };
        folded.push(plain);
    }
    folded
}

pub fn has_turkish_letters(text: &str) -> bool {
    text.chars()
        .any(|character| TURKISH_ONLY_LETTERS.contains(&character))
}

pub struct LetterRestorer {
    word_of_plain_spelling: HashMap<String, String>,
}

impl Default for LetterRestorer {
    fn default() -> LetterRestorer {
        LetterRestorer::new()
    }
}

impl LetterRestorer {
    pub fn new() -> LetterRestorer {
        let mut word_of_plain_spelling = HashMap::new();
        for word in TURKISH_WORDS.lines() {
            word_of_plain_spelling.insert(fold_turkish_letters(word), word.to_string());
        }
        LetterRestorer {
            word_of_plain_spelling,
        }
    }

    pub fn normalize_guess(&self, guess: &str) -> String {
        if has_turkish_letters(guess) {
            return normalize_text(guess);
        }
        let normalized = normalize_text(&guess.to_lowercase());
        let mut restored_words = Vec::new();
        for word in normalized.split(' ') {
            match self.word_of_plain_spelling.get(word) {
                Some(turkish_word) => restored_words.push(turkish_word.as_str()),
                None => restored_words.push(word),
            }
        }
        restored_words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_text_follows_turkish_rules() {
        assert_eq!(normalize_text("IŞIK"), "ışık");
        assert_eq!(normalize_text("İÇİN"), "için");
        assert_eq!(
            normalize_text("  Sosu   silen,   ekmek!  "),
            "sosu silen ekmek"
        );
    }

    #[test]
    fn guess_typed_without_turkish_letters_gets_them_back() {
        let restorer = LetterRestorer::new();
        assert_eq!(
            restorer.normalize_guess("Odaya girince neden geldigini unutmak"),
            "odaya girince neden geldiğini unutmak"
        );
    }

    #[test]
    fn capital_i_in_a_plain_guess_is_a_dotted_i() {
        let restorer = LetterRestorer::new();
        assert_eq!(restorer.normalize_guess("Insan"), "insan");
    }

    #[test]
    fn guess_with_turkish_letters_is_left_alone() {
        let restorer = LetterRestorer::new();
        assert_eq!(
            restorer.normalize_guess("Sosu silen ekmek, acı değil"),
            "sosu silen ekmek acı değil"
        );
    }

    #[test]
    fn unknown_words_stay_as_typed() {
        let restorer = LetterRestorer::new();
        assert_eq!(restorer.normalize_guess("qwxz sozcuk"), "qwxz sözcük");
    }

    #[test]
    fn folding_drops_turkish_letters() {
        assert_eq!(fold_turkish_letters("sıyırgaç öğüş"), "siyirgac ogus");
    }

    #[test]
    fn compact_text_drops_spaces_and_hyphens() {
        assert_eq!(compact_text("Göz-kulak olmak"), "gözkulakolmak");
    }
}
