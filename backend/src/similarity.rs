use std::sync::Arc;

use moka::future::Cache;
use uuid::Uuid;

use crate::encoder::{EncoderError, TeiEncoder};
use crate::entailment::EntailmentChecker;
use crate::puzzle::Puzzle;
use crate::text::{LetterRestorer, normalize_text};

pub const WIN_SIMILARITY: f64 = 0.615;
pub const ZERO_CLOSENESS_SIMILARITY: f64 = 0.15;
pub const HIGHEST_CLOSENESS_WITHOUT_WIN: i64 = 99;
pub const GUESS_CACHE_SIZE: u64 = 1000;
pub const WRONG_TURN_FLOOR: f64 = 0.4;
pub const TEST_GUESS_MARGIN: f64 = 0.02;
pub const HIGHEST_WIN_SIMILARITY: f64 = 0.70;
pub const ENTAILMENT_BAND_FLOOR: f64 = 0.40;
pub const ENTAILMENT_TO_WIN: f64 = 0.90;

pub fn closeness_percent(similarity: f64, win_similarity: f64) -> i64 {
    let similarity_range = win_similarity - ZERO_CLOSENESS_SIMILARITY;
    let percent = ((similarity - ZERO_CLOSENESS_SIMILARITY) / similarity_range * 100.0)
        .round_ties_even() as i64;
    percent.clamp(0, HIGHEST_CLOSENESS_WITHOUT_WIN)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrongTurn {
    Opposite,
    OtherMeaning,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Judgement {
    pub similarity: f64,
    pub won: bool,
    pub won_by_entailment: bool,
    pub wrong_turn: Option<WrongTurn>,
}

fn wrong_turn_of(
    closest_right: f64,
    closest_opposite: f64,
    closest_other_meaning: f64,
) -> Option<WrongTurn> {
    let opposite_leads = closest_opposite > closest_right
        && closest_opposite >= closest_other_meaning
        && closest_opposite >= WRONG_TURN_FLOOR;
    if opposite_leads {
        return Some(WrongTurn::Opposite);
    }
    let other_meaning_leads =
        closest_other_meaning > closest_right && closest_other_meaning >= WRONG_TURN_FLOOR;
    if other_meaning_leads {
        return Some(WrongTurn::OtherMeaning);
    }
    None
}

pub fn win_similarity_for(wrong_test_similarities: &[f64]) -> f64 {
    let mut win_similarity = WIN_SIMILARITY;
    for similarity in wrong_test_similarities {
        win_similarity = win_similarity.max(similarity + TEST_GUESS_MARGIN);
    }
    win_similarity.min(HIGHEST_WIN_SIMILARITY)
}

pub struct PreparedWord {
    pub puzzle: Puzzle,
    right_vectors: Vec<Vec<f32>>,
    opposite_vectors: Vec<Vec<f32>>,
    other_meaning_vectors: Vec<Vec<f32>>,
    win_similarity: f64,
    edition: String,
    right_texts: Vec<String>,
}

impl PreparedWord {
    pub fn new(
        puzzle: Puzzle,
        right_vectors: Vec<Vec<f32>>,
        opposite_vectors: Vec<Vec<f32>>,
        other_meaning_vectors: Vec<Vec<f32>>,
    ) -> PreparedWord {
        PreparedWord {
            puzzle,
            right_vectors,
            opposite_vectors,
            other_meaning_vectors,
            win_similarity: WIN_SIMILARITY,
            edition: Uuid::new_v4().simple().to_string(),
            right_texts: Vec::new(),
        }
    }

    pub fn edition(&self) -> &str {
        &self.edition
    }

    pub fn is_same_edition(&self, other: &PreparedWord) -> bool {
        self.edition == other.edition
    }

    pub fn keep_edition_of(&mut self, earlier: &PreparedWord) {
        self.edition = earlier.edition.clone();
    }

    pub fn win_similarity(&self) -> f64 {
        self.win_similarity
    }

    pub fn closeness_of(&self, similarity: f64) -> i64 {
        closeness_percent(similarity, self.win_similarity)
    }

    pub fn compare(&self, guess_vector: &[f32]) -> Judgement {
        let closest_right = closest_similarity(&self.right_vectors, guess_vector);
        let closest_opposite = closest_similarity(&self.opposite_vectors, guess_vector);
        let closest_other_meaning = closest_similarity(&self.other_meaning_vectors, guess_vector);

        let closest_wrong = closest_opposite.max(closest_other_meaning);
        let mut similarity = closest_right;
        if closest_wrong > closest_right {
            similarity = closest_right - (closest_wrong - closest_right);
        }
        Judgement {
            similarity,
            won: similarity >= self.win_similarity,
            won_by_entailment: false,
            wrong_turn: wrong_turn_of(closest_right, closest_opposite, closest_other_meaning),
        }
    }
}

fn dot_product(first: &[f32], second: &[f32]) -> f32 {
    let mut sum = 0.0;
    for (first_value, second_value) in first.iter().zip(second) {
        sum += first_value * second_value;
    }
    sum
}

fn closest_similarity(vectors: &[Vec<f32>], guess_vector: &[f32]) -> f64 {
    let mut closest = f64::NEG_INFINITY;
    for vector in vectors {
        closest = closest.max(dot_product(vector, guess_vector) as f64);
    }
    closest
}

pub struct MeaningJudge {
    encoder: TeiEncoder,
    guess_vectors: Cache<String, Arc<Vec<f32>>>,
    letters: LetterRestorer,
    entailment: Option<EntailmentChecker>,
}

impl MeaningJudge {
    pub fn new(encoder: TeiEncoder) -> MeaningJudge {
        MeaningJudge {
            encoder,
            guess_vectors: Cache::new(GUESS_CACHE_SIZE),
            letters: LetterRestorer::new(),
            entailment: None,
        }
    }

    pub fn with_entailment(mut self, checker: EntailmentChecker) -> MeaningJudge {
        self.entailment = Some(checker);
        self
    }

    async fn encode_all(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EncoderError> {
        let mut normalized_texts = Vec::new();
        for text in texts {
            normalized_texts.push(normalize_text(text));
        }
        self.encoder.encode(&normalized_texts).await
    }

    async fn encode_guesses(&self, guesses: &[String]) -> Result<Vec<Vec<f32>>, EncoderError> {
        let mut normalized_guesses = Vec::new();
        for guess in guesses {
            normalized_guesses.push(self.letters.normalize_guess(guess));
        }
        self.encoder.encode(&normalized_guesses).await
    }

    async fn encode_guess_without_cache(
        &self,
        normalized_guess: String,
    ) -> Result<Arc<Vec<f32>>, EncoderError> {
        let mut vectors = self.encoder.encode(&[normalized_guess]).await?;
        Ok(Arc::new(vectors.remove(0)))
    }

    async fn encode_guess(&self, normalized_guess: String) -> Result<Arc<Vec<f32>>, EncoderError> {
        let encoding = self.encode_guess_without_cache(normalized_guess.clone());
        match self
            .guess_vectors
            .try_get_with(normalized_guess, encoding)
            .await
        {
            Ok(vector) => Ok(vector),
            Err(shared_error) => Err(shared_error.as_ref().clone()),
        }
    }

    pub fn forget_guesses(&self) {
        self.guess_vectors.invalidate_all();
    }

    pub async fn prepare(&self, puzzle: Puzzle) -> Result<PreparedWord, EncoderError> {
        let mut right_texts = vec![puzzle.definition.clone()];
        right_texts.extend(puzzle.also_accepted.iter().cloned());
        let right_vectors = self.encode_all(&right_texts).await?;
        let mut normalized_right_texts = Vec::new();
        for text in &right_texts {
            normalized_right_texts.push(normalize_text(text));
        }

        let opposite_vectors = self.encode_all(&puzzle.opposite_meanings).await?;
        let other_meaning_vectors = self.encode_all(&puzzle.wrong_meanings).await?;
        let mut word = PreparedWord::new(
            puzzle,
            right_vectors,
            opposite_vectors,
            other_meaning_vectors,
        );
        word.right_texts = normalized_right_texts;

        let wrong_test_vectors = self.encode_guesses(&word.puzzle.wrong_test_guesses).await?;
        let mut wrong_test_similarities = Vec::new();
        for vector in &wrong_test_vectors {
            wrong_test_similarities.push(word.compare(vector).similarity);
        }
        word.win_similarity = win_similarity_for(&wrong_test_similarities);
        Ok(word)
    }

    pub async fn evaluate(
        &self,
        word: &PreparedWord,
        guess: &str,
    ) -> Result<Judgement, EncoderError> {
        let normalized_guess = self.letters.normalize_guess(guess);
        let guess_vector = self.encode_guess(normalized_guess.clone()).await?;
        let mut judgement = word.compare(&guess_vector);
        if !judgement.won
            && judgement.similarity >= ENTAILMENT_BAND_FLOOR
            && self
                .guess_implies_a_right_meaning(word, &normalized_guess)
                .await
        {
            judgement.won = true;
            judgement.won_by_entailment = true;
        }
        Ok(judgement)
    }

    async fn guess_implies_a_right_meaning(&self, word: &PreparedWord, guess: &str) -> bool {
        let Some(checker) = &self.entailment else {
            return false;
        };
        match checker.highest_entailment(guess, &word.right_texts).await {
            Ok(entailment) => entailment >= ENTAILMENT_TO_WIN,
            Err(error) => {
                eprintln!("Çıkarım denetimi yapılamadı, yalnız benzerlik kullanıldı: {error}");
                false
            }
        }
    }

    pub fn normalize_guess(&self, guess: &str) -> String {
        self.letters.normalize_guess(guess)
    }

    pub async fn similarity(&self, word: &PreparedWord, guess: &str) -> Result<f64, EncoderError> {
        Ok(self.evaluate(word, guess).await?.similarity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn puzzle() -> Puzzle {
        Puzzle {
            word: "sıyırgaç".to_string(),
            definition: "Tabakta kalan sosu sıyırmak için kullanılan ekmek parçası".to_string(),
            parts: "sıyır- + -gaç".to_string(),
            sentence: "Son lokmayı sıyırgaçla topladı.".to_string(),
            category: "nesne".to_string(),
            field: "yemek".to_string(),
            also_accepted: vec![],
            opposite_meanings: vec![],
            wrong_meanings: vec![],
            inflected_stems: vec![],
            right_test_guesses: vec![],
            wrong_test_guesses: vec![],
        }
    }

    #[test]
    fn closeness_never_shows_a_win_below_threshold() {
        assert_eq!(
            closeness_percent(WIN_SIMILARITY - 0.0001, WIN_SIMILARITY),
            99
        );
    }

    #[test]
    fn closeness_never_goes_below_zero() {
        assert_eq!(closeness_percent(-0.5, WIN_SIMILARITY), 0);
    }

    #[test]
    fn threshold_stays_when_wrong_test_guesses_score_low() {
        assert_eq!(win_similarity_for(&[0.2, 0.5]), WIN_SIMILARITY);
        assert_eq!(win_similarity_for(&[]), WIN_SIMILARITY);
    }

    #[test]
    fn threshold_rises_above_a_wrong_test_guess_that_would_win() {
        let win_similarity = win_similarity_for(&[0.3, 0.64]);
        assert!((win_similarity - 0.66).abs() < 1e-9);
    }

    #[test]
    fn threshold_never_rises_past_the_ceiling() {
        assert_eq!(win_similarity_for(&[0.95]), HIGHEST_WIN_SIMILARITY);
    }

    #[test]
    fn raised_threshold_decides_the_win() {
        let mut word = PreparedWord::new(puzzle(), vec![vec![1.0, 0.0]], vec![], vec![]);
        word.win_similarity = 0.66;
        assert!(!word.compare(&[0.64, 0.768]).won);
        assert!(word.compare(&[0.8, 0.6]).won);
    }

    #[test]
    fn closest_right_meaning_counts() {
        let word = PreparedWord::new(
            puzzle(),
            vec![vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]],
            vec![],
            vec![],
        );
        assert_eq!(word.compare(&[0.0, 1.0, 0.0]).similarity, 1.0);
    }

    #[test]
    fn being_closer_to_a_wrong_meaning_lowers_the_similarity() {
        let word = PreparedWord::new(puzzle(), vec![vec![1.0, 0.0]], vec![], vec![vec![0.6, 0.8]]);
        let judgement = word.compare(&[0.6, 0.8]);
        assert!((judgement.similarity - 0.2).abs() < 1e-6);
        assert!(!judgement.won);
    }

    #[test]
    fn closest_opposite_is_named() {
        let word = PreparedWord::new(
            puzzle(),
            vec![vec![1.0, 0.0, 0.0]],
            vec![vec![0.0, 1.0, 0.0]],
            vec![vec![0.0, 0.0, 1.0]],
        );
        let judgement = word.compare(&[0.0, 0.9, 0.3]);
        assert_eq!(judgement.wrong_turn, Some(WrongTurn::Opposite));
    }

    #[test]
    fn closest_other_meaning_is_named() {
        let word = PreparedWord::new(
            puzzle(),
            vec![vec![1.0, 0.0, 0.0]],
            vec![vec![0.0, 1.0, 0.0]],
            vec![vec![0.0, 0.0, 1.0]],
        );
        let judgement = word.compare(&[0.0, 0.3, 0.9]);
        assert_eq!(judgement.wrong_turn, Some(WrongTurn::OtherMeaning));
    }

    #[test]
    fn faint_resemblance_to_a_wrong_meaning_is_not_named() {
        let word = PreparedWord::new(puzzle(), vec![vec![1.0, 0.0]], vec![vec![0.0, 1.0]], vec![]);
        let judgement = word.compare(&[0.1, 0.3]);
        assert_eq!(judgement.wrong_turn, None);
    }

    #[test]
    fn guess_closer_to_the_right_meaning_has_no_wrong_turn() {
        let word = PreparedWord::new(puzzle(), vec![vec![1.0, 0.0]], vec![vec![0.0, 1.0]], vec![]);
        let judgement = word.compare(&[0.8, 0.6]);
        assert_eq!(judgement.wrong_turn, None);
    }
}
