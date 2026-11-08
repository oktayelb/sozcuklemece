use std::collections::HashMap;
use std::sync::Arc;

use crate::game::{Game, Outcome};
use crate::similarity::PreparedWord;
use crate::stats::{StatsSummary, WordStats};

pub const MIN_PLAYER_ID_LENGTH: usize = 16;
pub const MAX_PLAYER_ID_LENGTH: usize = 64;

pub fn is_valid_player_id(player_id: &str) -> bool {
    let length = player_id.len();
    if !(MIN_PLAYER_ID_LENGTH..=MAX_PLAYER_ID_LENGTH).contains(&length) {
        return false;
    }
    for character in player_id.chars() {
        if !character.is_ascii_alphanumeric() && character != '-' {
            return false;
        }
    }
    true
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerDay {
    Playing { game_id: String },
    Finished { outcome: Outcome },
}

pub struct TodaysPlayers {
    word: Option<Arc<PreparedWord>>,
    days: HashMap<String, PlayerDay>,
    stats: WordStats,
}

impl Default for TodaysPlayers {
    fn default() -> TodaysPlayers {
        TodaysPlayers::new()
    }
}

impl TodaysPlayers {
    pub fn new() -> TodaysPlayers {
        TodaysPlayers {
            word: None,
            days: HashMap::new(),
            stats: WordStats::default(),
        }
    }

    fn is_tracking(&self, word: &Arc<PreparedWord>) -> bool {
        match &self.word {
            Some(tracked_word) => Arc::ptr_eq(tracked_word, word),
            None => false,
        }
    }

    pub fn follow_word(&mut self, word: &Arc<PreparedWord>) {
        if self.is_tracking(word) {
            return;
        }
        self.word = Some(word.clone());
        self.days.clear();
        self.stats = WordStats::default();
    }

    pub fn day_of(&self, player_id: &str) -> Option<PlayerDay> {
        self.days.get(player_id).cloned()
    }

    pub fn record_start(&mut self, game: &Game) {
        self.follow_word(&game.word);
        let day = PlayerDay::Playing {
            game_id: game.id.clone(),
        };
        self.days.insert(game.player_id.clone(), day);
    }

    pub fn record_finish(&mut self, game: &Game) {
        if !game.is_finished() || !self.is_tracking(&game.word) {
            return;
        }
        let expected_day = PlayerDay::Playing {
            game_id: game.id.clone(),
        };
        if self.days.get(&game.player_id) != Some(&expected_day) {
            return;
        }
        let outcome = game.outcome();
        self.stats.add(&outcome);
        self.days
            .insert(game.player_id.clone(), PlayerDay::Finished { outcome });
    }

    pub fn stats_for(&self, outcome: &Outcome) -> StatsSummary {
        self.stats.summary_for(outcome)
    }

    pub fn stats_for_game(&self, game: &Game) -> Option<StatsSummary> {
        if !self.is_tracking(&game.word) {
            return None;
        }
        match self.days.get(&game.player_id) {
            Some(PlayerDay::Finished { outcome }) => Some(self.stats_for(outcome)),
            _ => None,
        }
    }

    pub fn player_count(&self) -> usize {
        self.days.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::puzzle::Puzzle;

    const PLAYER: &str = "0123456789abcdef0123456789abcdef";
    const START: f64 = 1000.0;

    fn prepared_word() -> Arc<PreparedWord> {
        let puzzle = Puzzle {
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
        };
        Arc::new(PreparedWord::new(puzzle, vec![], vec![], vec![]))
    }

    fn started_game(players: &mut TodaysPlayers, word: &Arc<PreparedWord>) -> Game {
        let game = Game::new(word.clone(), PLAYER.to_string(), START);
        players.record_start(&game);
        game
    }

    #[test]
    fn player_ids_must_be_long_random_looking_text() {
        assert!(is_valid_player_id(PLAYER));
        assert!(is_valid_player_id("6f1c2d3e-4b5a-4c7d-8e9f-0a1b2c3d4e5f"));
        assert!(!is_valid_player_id("kisa"));
        assert!(!is_valid_player_id(&"a".repeat(MAX_PLAYER_ID_LENGTH + 1)));
        assert!(!is_valid_player_id("0123456789abcdef 0123456789"));
        assert!(!is_valid_player_id("0123456789abcdefğ"));
    }

    #[test]
    fn started_game_is_remembered() {
        let mut players = TodaysPlayers::new();
        let game = started_game(&mut players, &prepared_word());
        assert_eq!(
            players.day_of(PLAYER),
            Some(PlayerDay::Playing { game_id: game.id })
        );
    }

    #[test]
    fn running_game_is_not_recorded_as_finished() {
        let mut players = TodaysPlayers::new();
        let game = started_game(&mut players, &prepared_word());
        players.record_finish(&game);
        assert!(matches!(
            players.day_of(PLAYER),
            Some(PlayerDay::Playing { .. })
        ));
    }

    #[test]
    fn finished_game_keeps_its_outcome() {
        let mut players = TodaysPlayers::new();
        let mut game = started_game(&mut players, &prepared_word());
        game.finish(START + 10.0, false);

        players.record_finish(&game);

        assert_eq!(
            players.day_of(PLAYER),
            Some(PlayerDay::Finished {
                outcome: game.outcome()
            })
        );
    }

    #[test]
    fn finished_game_is_counted_in_the_stats_once() {
        let mut players = TodaysPlayers::new();
        let mut game = started_game(&mut players, &prepared_word());
        game.finish(START + 10.0, false);

        players.record_finish(&game);
        players.record_finish(&game);

        assert_eq!(players.stats_for_game(&game).unwrap().players, 1);
    }

    #[test]
    fn running_game_has_no_stats() {
        let mut players = TodaysPlayers::new();
        let game = started_game(&mut players, &prepared_word());
        assert_eq!(players.stats_for_game(&game), None);
    }

    #[test]
    fn new_word_forgets_who_played() {
        let mut players = TodaysPlayers::new();
        started_game(&mut players, &prepared_word());

        players.follow_word(&prepared_word());

        assert_eq!(players.day_of(PLAYER), None);
        assert_eq!(players.player_count(), 0);
    }

    #[test]
    fn game_of_an_old_word_is_ignored() {
        let mut players = TodaysPlayers::new();
        let mut old_game = started_game(&mut players, &prepared_word());
        let new_word = prepared_word();
        players.follow_word(&new_word);
        started_game(&mut players, &new_word);
        old_game.finish(START + 10.0, false);

        players.record_finish(&old_game);

        assert!(matches!(
            players.day_of(PLAYER),
            Some(PlayerDay::Playing { .. })
        ));
    }
}
