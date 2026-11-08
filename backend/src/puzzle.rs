use serde::{Deserialize, Serialize};

pub const MAX_SHORT_TEXT_LENGTH: usize = 60;
pub const MAX_LONG_TEXT_LENGTH: usize = 500;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Puzzle {
    pub word: String,
    pub definition: String,
    pub parts: String,
    pub sentence: String,
    pub category: String,
    pub field: String,
    #[serde(default)]
    pub also_accepted: Vec<String>,
    #[serde(default)]
    pub opposite_meanings: Vec<String>,
    #[serde(default)]
    pub wrong_meanings: Vec<String>,
    #[serde(default)]
    pub inflected_stems: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct InvalidField(pub &'static str);

fn cleaned_text(
    field: &'static str,
    text: &str,
    max_length: usize,
) -> Result<String, InvalidField> {
    let trimmed = text.trim();
    let length = trimmed.chars().count();
    if length == 0 || length > max_length {
        return Err(InvalidField(field));
    }
    Ok(trimmed.to_string())
}

fn cleaned_list(
    field: &'static str,
    texts: &[String],
    max_length: usize,
) -> Result<Vec<String>, InvalidField> {
    let mut cleaned = Vec::new();
    for text in texts {
        cleaned.push(cleaned_text(field, text, max_length)?);
    }
    Ok(cleaned)
}

impl Puzzle {
    pub fn cleaned(&self) -> Result<Puzzle, InvalidField> {
        Ok(Puzzle {
            word: cleaned_text("word", &self.word, MAX_SHORT_TEXT_LENGTH)?,
            definition: cleaned_text("definition", &self.definition, MAX_LONG_TEXT_LENGTH)?,
            parts: cleaned_text("parts", &self.parts, MAX_LONG_TEXT_LENGTH)?,
            sentence: cleaned_text("sentence", &self.sentence, MAX_LONG_TEXT_LENGTH)?,
            category: cleaned_text("category", &self.category, MAX_SHORT_TEXT_LENGTH)?,
            field: cleaned_text("field", &self.field, MAX_SHORT_TEXT_LENGTH)?,
            also_accepted: cleaned_list(
                "also_accepted",
                &self.also_accepted,
                MAX_LONG_TEXT_LENGTH,
            )?,
            opposite_meanings: cleaned_list(
                "opposite_meanings",
                &self.opposite_meanings,
                MAX_LONG_TEXT_LENGTH,
            )?,
            wrong_meanings: cleaned_list(
                "wrong_meanings",
                &self.wrong_meanings,
                MAX_LONG_TEXT_LENGTH,
            )?,
            inflected_stems: cleaned_list(
                "inflected_stems",
                &self.inflected_stems,
                MAX_SHORT_TEXT_LENGTH,
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn puzzle() -> Puzzle {
        Puzzle {
            word: "  sıyırgaç ".to_string(),
            definition: "Tabakta kalan sosu sıyırmak için kullanılan ekmek parçası".to_string(),
            parts: "sıyır- + -gaç".to_string(),
            sentence: "Son lokmayı sıyırgaçla topladı.".to_string(),
            category: "nesne".to_string(),
            field: "yemek".to_string(),
            also_accepted: vec![],
            opposite_meanings: vec![],
            wrong_meanings: vec![],
            inflected_stems: vec![],
        }
    }

    #[test]
    fn text_is_trimmed() {
        assert_eq!(puzzle().cleaned().unwrap().word, "sıyırgaç");
    }

    #[test]
    fn blank_and_oversized_fields_are_refused() {
        let blank = Puzzle {
            definition: "   ".to_string(),
            ..puzzle()
        };
        assert_eq!(blank.cleaned(), Err(InvalidField("definition")));

        let oversized = Puzzle {
            word: "a".repeat(MAX_SHORT_TEXT_LENGTH + 1),
            ..puzzle()
        };
        assert_eq!(oversized.cleaned(), Err(InvalidField("word")));

        let blank_line = Puzzle {
            also_accepted: vec!["  ".to_string()],
            ..puzzle()
        };
        assert_eq!(blank_line.cleaned(), Err(InvalidField("also_accepted")));
    }

    #[test]
    fn length_is_counted_in_letters_not_bytes() {
        let turkish_word = Puzzle {
            word: "ş".repeat(MAX_SHORT_TEXT_LENGTH),
            ..puzzle()
        };
        assert!(turkish_word.cleaned().is_ok());
    }
}
