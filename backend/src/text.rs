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
    fn compact_text_drops_spaces_and_hyphens() {
        assert_eq!(compact_text("Göz-kulak olmak"), "gözkulakolmak");
    }
}
