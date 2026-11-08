use std::sync::Arc;

use moka::future::Cache;

use crate::encoder::{EncoderError, TeiEncoder};
use crate::puzzle::Puzzle;
use crate::text::normalize_text;

pub const WIN_SIMILARITY: f64 = 0.625;
pub const ZERO_CLOSENESS_SIMILARITY: f64 = 0.15;
pub const HIGHEST_CLOSENESS_WITHOUT_WIN: i64 = 99;
pub const GUESS_CACHE_SIZE: u64 = 1000;

pub fn closeness_percent(similarity: f64) -> i64 {
    let similarity_range = WIN_SIMILARITY - ZERO_CLOSENESS_SIMILARITY;
    let percent = ((similarity - ZERO_CLOSENESS_SIMILARITY) / similarity_range * 100.0)
        .round_ties_even() as i64;
    percent.clamp(0, HIGHEST_CLOSENESS_WITHOUT_WIN)
}

pub struct PreparedWord {
    pub puzzle: Puzzle,
    right_vectors: Vec<Vec<f32>>,
    wrong_vectors: Vec<Vec<f32>>,
}

impl PreparedWord {
    pub fn new(
        puzzle: Puzzle,
        right_vectors: Vec<Vec<f32>>,
        wrong_vectors: Vec<Vec<f32>>,
    ) -> PreparedWord {
        PreparedWord {
            puzzle,
            right_vectors,
            wrong_vectors,
        }
    }

    pub fn similarity_to(&self, guess_vector: &[f32]) -> f64 {
        let closest_right = closest_similarity(&self.right_vectors, guess_vector);
        if self.wrong_vectors.is_empty() {
            return closest_right;
        }
        let closest_wrong = closest_similarity(&self.wrong_vectors, guess_vector);
        let wrong_side_excess = (closest_wrong - closest_right).max(0.0);
        closest_right - wrong_side_excess
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
}

impl MeaningJudge {
    pub fn new(encoder: TeiEncoder) -> MeaningJudge {
        MeaningJudge {
            encoder,
            guess_vectors: Cache::new(GUESS_CACHE_SIZE),
        }
    }

    async fn encode_all(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EncoderError> {
        let mut normalized_texts = Vec::new();
        for text in texts {
            normalized_texts.push(normalize_text(text));
        }
        self.encoder.encode(&normalized_texts).await
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

        let mut wrong_vectors = Vec::new();
        if !puzzle.wrong_meanings.is_empty() {
            wrong_vectors = self.encode_all(&puzzle.wrong_meanings).await?;
        }
        Ok(PreparedWord::new(puzzle, right_vectors, wrong_vectors))
    }

    pub async fn similarity(&self, word: &PreparedWord, guess: &str) -> Result<f64, EncoderError> {
        let guess_vector = self.encode_guess(normalize_text(guess)).await?;
        Ok(word.similarity_to(&guess_vector))
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
            wrong_meanings: vec![],
            inflected_stems: vec![],
        }
    }

    #[test]
    fn closeness_never_shows_a_win_below_threshold() {
        assert_eq!(closeness_percent(WIN_SIMILARITY - 0.0001), 99);
    }

    #[test]
    fn closeness_never_goes_below_zero() {
        assert_eq!(closeness_percent(-0.5), 0);
    }

    #[test]
    fn closest_right_meaning_counts() {
        let word = PreparedWord::new(puzzle(), vec![vec![1.0, 0.0], vec![0.0, 1.0]], vec![]);
        assert_eq!(word.similarity_to(&[0.0, 1.0]), 1.0);
    }

    #[test]
    fn being_closer_to_a_wrong_meaning_lowers_the_similarity() {
        let word = PreparedWord::new(puzzle(), vec![vec![1.0, 0.0]], vec![vec![0.6, 0.8]]);
        let similarity = word.similarity_to(&[0.6, 0.8]);
        assert!((similarity - 0.2).abs() < 1e-6);
    }
}
