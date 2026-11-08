use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::puzzle::Puzzle;
use crate::similarity::{Judgement, PreparedWord, WrongTurn, closeness_percent};
use crate::text::{compact_text, normalize_text};

pub const GAME_SECONDS: f64 = 180.0;
pub const GAME_RESULT_GRACE_SECONDS: f64 = 120.0;
pub const BASE_SCORE: f64 = 1000.0;
pub const MAX_TIME_PENALTY: f64 = 500.0;
pub const HINT_PENALTY: i64 = 150;
pub const MIN_GUESS_LENGTH: usize = 3;
pub const MAX_GUESS_LENGTH: usize = 200;
pub const WIN_CLOSENESS: i64 = 100;
pub const GUESS_INTERVAL_SECONDS: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HintKind {
    Parts,
    Sentence,
    Category,
}

pub const HINT_KINDS: [HintKind; 3] = [HintKind::Parts, HintKind::Sentence, HintKind::Category];

impl HintKind {
    pub fn label(self) -> &'static str {
        match self {
            HintKind::Parts => "Kök ve ekler",
            HintKind::Sentence => "Cümlede kullanımı",
            HintKind::Category => "Kategori ve alan",
        }
    }
}

pub fn closeness_label(closeness: i64) -> &'static str {
    if closeness >= WIN_CLOSENESS {
        return "Buldun!";
    }
    if closeness >= 85 {
        return "Çok yakın";
    }
    if closeness >= 60 {
        return "Yakın";
    }
    if closeness >= 30 {
        return "Isınıyor";
    }
    "Uzak"
}

pub fn wrong_turn_note(wrong_turn: WrongTurn) -> &'static str {
    match wrong_turn {
        WrongTurn::Opposite => "Tersini söylüyor gibisin.",
        WrongTurn::OtherMeaning => "Konu yakın, anlam başka.",
    }
}

pub fn repeats_the_word(guess: &str, puzzle: &Puzzle) -> bool {
    let compact_guess = compact_text(guess);
    let mut forbidden_forms = vec![&puzzle.word];
    forbidden_forms.extend(&puzzle.inflected_stems);
    for form in forbidden_forms {
        let compact_form = compact_text(form);
        if !compact_form.is_empty() && compact_guess.contains(&compact_form) {
            return true;
        }
    }
    false
}

pub fn remove_stale_games(games: &mut HashMap<String, Game>, now: f64) -> Vec<Game> {
    let mut stale_ids = Vec::new();
    for (id, game) in games.iter() {
        if game.is_stale(now) {
            stale_ids.push(id.clone());
        }
    }
    let mut removed = Vec::new();
    for id in stale_ids {
        if let Some(mut game) = games.remove(&id) {
            game.finish(now, false);
            removed.push(game);
        }
    }
    removed
}

fn category_text(puzzle: &Puzzle) -> String {
    format!("{} · {}", puzzle.category, puzzle.field)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outcome {
    pub won: bool,
    pub score: i64,
    pub seconds_used: i64,
    pub hints_used: usize,
    pub guess_count: u32,
    pub best_closeness: i64,
}

pub fn game_result(puzzle: &Puzzle, outcome: &Outcome) -> FinishResponse {
    FinishResponse {
        word: puzzle.word.clone(),
        definition: puzzle.definition.clone(),
        parts: puzzle.parts.clone(),
        sentence: puzzle.sentence.clone(),
        category: category_text(puzzle),
        won: outcome.won,
        score: outcome.score,
        seconds_used: outcome.seconds_used,
        hints_used: outcome.hints_used,
        guess_count: outcome.guess_count,
        best_closeness: outcome.best_closeness,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RevealedHint {
    pub kind: HintKind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JudgedGuess {
    pub text: String,
    pub closeness: i64,
    pub label: &'static str,
    pub note: Option<&'static str>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct HintResponse {
    pub kind: HintKind,
    pub text: Option<String>,
    pub finished: bool,
    pub remaining_seconds: f64,
    pub score: i64,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct GuessResponse {
    pub closeness: Option<i64>,
    pub message: String,
    pub note: Option<&'static str>,
    pub rejected: bool,
    pub won: bool,
    pub finished: bool,
    pub remaining_seconds: f64,
    pub score: i64,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct FinishResponse {
    pub word: String,
    pub definition: String,
    pub parts: String,
    pub sentence: String,
    pub category: String,
    pub won: bool,
    pub score: i64,
    pub seconds_used: i64,
    pub hints_used: usize,
    pub guess_count: u32,
    pub best_closeness: i64,
}

pub enum GuessCheck {
    TooSoon { seconds_to_wait: f64 },
    Answered(GuessResponse),
    NeedsJudging(Arc<PreparedWord>),
}

pub struct Game {
    pub id: String,
    pub player_id: String,
    pub word: Arc<PreparedWord>,
    started_at: f64,
    used_hints: Vec<HintKind>,
    judged_guesses: Vec<JudgedGuess>,
    guess_count: u32,
    best_closeness: i64,
    finished: bool,
    won: bool,
    final_score: i64,
    seconds_used: f64,
    last_guess_at: Option<f64>,
}

impl Game {
    pub fn new(word: Arc<PreparedWord>, player_id: String, started_at: f64) -> Game {
        Game {
            id: Uuid::new_v4().simple().to_string(),
            player_id,
            word,
            started_at,
            used_hints: Vec::new(),
            judged_guesses: Vec::new(),
            guess_count: 0,
            best_closeness: 0,
            finished: false,
            won: false,
            final_score: 0,
            seconds_used: 0.0,
            last_guess_at: None,
        }
    }

    pub fn puzzle(&self) -> &Puzzle {
        &self.word.puzzle
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn is_stale(&self, now: f64) -> bool {
        now - self.started_at >= GAME_SECONDS + GAME_RESULT_GRACE_SECONDS
    }

    pub fn remaining_seconds(&self, now: f64) -> f64 {
        if self.finished {
            return GAME_SECONDS - self.seconds_used;
        }
        let elapsed = now - self.started_at;
        (GAME_SECONDS - elapsed).max(0.0)
    }

    fn score_if_won_now(&self, now: f64) -> i64 {
        let elapsed_share = 1.0 - self.remaining_seconds(now) / GAME_SECONDS;
        let time_penalty = MAX_TIME_PENALTY * elapsed_share;
        let hint_penalty = (HINT_PENALTY * self.used_hints.len() as i64) as f64;
        (BASE_SCORE - time_penalty - hint_penalty).round_ties_even() as i64
    }

    pub fn current_score(&self, now: f64) -> i64 {
        if self.finished {
            return self.final_score;
        }
        self.score_if_won_now(now)
    }

    pub fn finish(&mut self, now: f64, won: bool) {
        if self.finished {
            return;
        }
        if won {
            self.final_score = self.score_if_won_now(now);
        }
        self.seconds_used = GAME_SECONDS - self.remaining_seconds(now);
        self.won = won;
        self.finished = true;
    }

    pub fn finish_if_time_is_up(&mut self, now: f64) {
        if self.remaining_seconds(now) <= 0.0 {
            self.finish(now, false);
        }
    }

    fn hint_text(&self, kind: HintKind) -> String {
        let puzzle = self.puzzle();
        match kind {
            HintKind::Parts => puzzle.parts.clone(),
            HintKind::Sentence => puzzle.sentence.clone(),
            HintKind::Category => category_text(puzzle),
        }
    }

    pub fn revealed_hints(&self) -> Vec<RevealedHint> {
        let mut revealed = Vec::new();
        for kind in &self.used_hints {
            revealed.push(RevealedHint {
                kind: *kind,
                text: self.hint_text(*kind),
            });
        }
        revealed
    }

    pub fn judged_guesses(&self) -> Vec<JudgedGuess> {
        self.judged_guesses.clone()
    }

    pub fn reveal_hint(&mut self, kind: HintKind, now: f64) -> HintResponse {
        self.finish_if_time_is_up(now);
        let mut text = None;
        if !self.finished {
            text = Some(self.hint_text(kind));
            if !self.used_hints.contains(&kind) {
                self.used_hints.push(kind);
            }
        }
        HintResponse {
            kind,
            text,
            finished: self.finished,
            remaining_seconds: self.remaining_seconds(now),
            score: self.current_score(now),
        }
    }

    fn guess_response(
        &self,
        now: f64,
        message: &str,
        closeness: Option<i64>,
        rejected: bool,
    ) -> GuessResponse {
        GuessResponse {
            closeness,
            message: message.to_string(),
            note: None,
            rejected,
            won: self.won,
            finished: self.finished,
            remaining_seconds: self.remaining_seconds(now),
            score: self.current_score(now),
        }
    }

    fn seconds_until_next_guess(&self, now: f64) -> f64 {
        match self.last_guess_at {
            Some(last_guess_at) => last_guess_at + GUESS_INTERVAL_SECONDS - now,
            None => 0.0,
        }
    }

    pub fn check_guess(&mut self, text: &str, now: f64) -> GuessCheck {
        self.finish_if_time_is_up(now);
        if self.finished {
            return GuessCheck::Answered(self.guess_response(now, "Oyun bitti.", None, false));
        }
        let seconds_to_wait = self.seconds_until_next_guess(now);
        if seconds_to_wait > 0.0 {
            return GuessCheck::TooSoon { seconds_to_wait };
        }
        self.last_guess_at = Some(now);

        if normalize_text(text).chars().count() < MIN_GUESS_LENGTH {
            return GuessCheck::Answered(self.guess_response(
                now,
                "Biraz daha açık yaz.",
                None,
                true,
            ));
        }
        if repeats_the_word(text, self.puzzle()) {
            let message = "Sözcüğün kendisini kullanmadan anlat.";
            return GuessCheck::Answered(self.guess_response(now, message, None, true));
        }
        GuessCheck::NeedsJudging(self.word.clone())
    }

    pub fn record_judged_guess(
        &mut self,
        text: &str,
        judgement: &Judgement,
        now: f64,
    ) -> GuessResponse {
        self.finish_if_time_is_up(now);
        if self.finished {
            return self.guess_response(now, "Oyun bitti.", None, false);
        }

        let closeness;
        let mut note = None;
        if judgement.won {
            closeness = WIN_CLOSENESS;
            self.finish(now, true);
        } else {
            closeness = closeness_percent(judgement.similarity);
            if let Some(wrong_turn) = judgement.wrong_turn {
                note = Some(wrong_turn_note(wrong_turn));
            }
        }
        let label = closeness_label(closeness);
        self.guess_count += 1;
        self.best_closeness = self.best_closeness.max(closeness);
        self.judged_guesses.push(JudgedGuess {
            text: text.to_string(),
            closeness,
            label,
            note,
        });
        let mut response = self.guess_response(now, label, Some(closeness), false);
        response.note = note;
        response
    }

    pub fn outcome(&self) -> Outcome {
        Outcome {
            won: self.won,
            score: self.final_score,
            seconds_used: self.seconds_used.round_ties_even() as i64,
            hints_used: self.used_hints.len(),
            guess_count: self.guess_count,
            best_closeness: self.best_closeness,
        }
    }

    pub fn result(&self) -> FinishResponse {
        game_result(self.puzzle(), &self.outcome())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::similarity::WIN_SIMILARITY;

    const START: f64 = 1000.0;

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
        }
    }

    fn puzzle_with_word(word: &str, inflected_stems: &[&str]) -> Puzzle {
        let mut stems = Vec::new();
        for stem in inflected_stems {
            stems.push(stem.to_string());
        }
        Puzzle {
            word: word.to_string(),
            inflected_stems: stems,
            ..puzzle()
        }
    }

    fn game_started_at(started_at: f64) -> Game {
        Game::new(
            Arc::new(PreparedWord::new(puzzle(), vec![], vec![], vec![])),
            "oyuncu".to_string(),
            started_at,
        )
    }

    fn new_game() -> Game {
        game_started_at(START)
    }

    fn judgement_of(similarity: f64) -> Judgement {
        Judgement {
            similarity,
            won: similarity >= WIN_SIMILARITY,
            wrong_turn: None,
        }
    }

    fn submit(game: &mut Game, text: &str, similarity: f64, now: f64) -> GuessResponse {
        match game.check_guess(text, now) {
            GuessCheck::TooSoon { .. } => panic!("tahmin aralığı beklenmedi"),
            GuessCheck::Answered(response) => response,
            GuessCheck::NeedsJudging(_) => {
                game.record_judged_guess(text, &judgement_of(similarity), now)
            }
        }
    }

    #[test]
    fn full_score_at_first_moment() {
        assert_eq!(new_game().current_score(START), 1000);
    }

    #[test]
    fn score_drops_with_time() {
        let game = new_game();
        assert_eq!(game.current_score(START + 90.0), 750);
        assert_eq!(game.current_score(START + GAME_SECONDS), 500);
    }

    #[test]
    fn each_hint_costs_points_once() {
        let mut game = new_game();
        game.reveal_hint(HintKind::Parts, START);
        game.reveal_hint(HintKind::Parts, START);
        assert_eq!(game.current_score(START), 1000 - HINT_PENALTY);
        game.reveal_hint(HintKind::Sentence, START);
        assert_eq!(game.current_score(START), 1000 - 2 * HINT_PENALTY);
    }

    #[test]
    fn worst_winning_score_stays_positive() {
        let mut game = new_game();
        for kind in HINT_KINDS {
            game.reveal_hint(kind, START);
        }
        assert_eq!(game.current_score(START + GAME_SECONDS), 50);
    }

    #[test]
    fn category_hint_contains_category_and_field() {
        let text = new_game()
            .reveal_hint(HintKind::Category, START)
            .text
            .unwrap();
        assert!(text.contains("nesne"));
        assert!(text.contains("yemek"));
    }

    #[test]
    fn winning_guess_finishes_game_with_score() {
        let mut game = new_game();
        let response = submit(&mut game, "sosu silen ekmek", WIN_SIMILARITY, START + 36.0);
        assert!(response.won);
        assert!(response.finished);
        assert_eq!(response.closeness, Some(100));
        assert_eq!(response.score, 900);
    }

    #[test]
    fn score_is_frozen_after_win() {
        let mut game = new_game();
        submit(&mut game, "sosu silen ekmek", WIN_SIMILARITY, START + 36.0);
        assert_eq!(game.current_score(START + 170.0), 900);
    }

    #[test]
    fn guess_below_threshold_keeps_game_running() {
        let mut game = new_game();
        let response = submit(
            &mut game,
            "sosu silen kaşık",
            WIN_SIMILARITY - 0.01,
            START + 5.0,
        );
        assert!(!response.won);
        assert!(!response.finished);
        assert!(response.closeness.unwrap() < 100);
    }

    #[test]
    fn guess_after_deadline_cannot_win() {
        let mut game = new_game();
        let response = submit(
            &mut game,
            "sosu silen ekmek",
            0.99,
            START + GAME_SECONDS + 1.0,
        );
        assert!(!response.won);
        assert!(response.finished);
        assert_eq!(response.closeness, None);
        assert_eq!(game.result().score, 0);
    }

    #[test]
    fn game_ended_while_its_guess_was_judged_does_not_count_the_guess() {
        let mut game = new_game();
        let GuessCheck::NeedsJudging(_) = game.check_guess("sosu silen ekmek", START) else {
            panic!("tahmin değerlendirilmeliydi");
        };
        game.finish(START + 1.0, false);

        let response = game.record_judged_guess("sosu silen ekmek", &judgement_of(0.99), START);

        assert!(!response.won);
        assert_eq!(game.result().guess_count, 0);
    }

    #[test]
    fn hint_after_deadline_reveals_nothing() {
        let response = new_game().reveal_hint(HintKind::Sentence, START + GAME_SECONDS + 1.0);
        assert_eq!(response.text, None);
        assert!(response.finished);
    }

    #[test]
    fn rejected_guess_is_not_counted() {
        let mut game = new_game();
        let response = submit(&mut game, "SIYIRGAÇ", 0.99, START);
        assert!(response.rejected);
        assert!(!response.finished);
        assert_eq!(game.result().guess_count, 0);
    }

    #[test]
    fn repeating_the_word_is_detected() {
        for guess in [
            "sıyırgaç",
            "SIYIRGAÇ",
            "sıyır gaç",
            "sıyır-gaç",
            "bu bir sıyırgaçtır",
        ] {
            assert!(repeats_the_word(guess, &puzzle()), "{guess}");
        }
    }

    #[test]
    fn explaining_with_the_root_is_allowed() {
        assert!(!repeats_the_word("tabağı sıyırmak", &puzzle()));
    }

    #[test]
    fn inflected_forms_of_the_word_are_detected() {
        let verb_puzzle = puzzle_with_word("evimsemek", &["evimse"]);
        for guess in ["evimsemek", "evimsedim", "evimsemeye başladım", "EVİMSEDİM"] {
            assert!(repeats_the_word(guess, &verb_puzzle), "{guess}");
        }
    }

    #[test]
    fn word_written_with_a_space_or_hyphen_is_detected() {
        let hyphenated = puzzle_with_word("göz-kulak", &[]);
        for guess in ["göz kulak", "gözkulak", "göz-kulak", "GÖZ KULAK olmak"] {
            assert!(repeats_the_word(guess, &hyphenated), "{guess}");
        }
    }

    #[test]
    fn stem_is_matched_whatever_its_case() {
        assert!(repeats_the_word(
            "evimsedim",
            &puzzle_with_word("evimsemek", &["EVİMSE"])
        ));
    }

    #[test]
    fn stem_with_no_letters_does_not_reject_every_guess() {
        assert!(!repeats_the_word(
            "bambaşka bir tahmin",
            &puzzle_with_word("evimsemek", &["-"])
        ));
    }

    #[test]
    fn game_stays_until_its_result_can_no_longer_be_requested() {
        let game = new_game();
        assert!(!game.is_stale(START + GAME_SECONDS + GAME_RESULT_GRACE_SECONDS - 1.0));
        assert!(game.is_stale(START + GAME_SECONDS + GAME_RESULT_GRACE_SECONDS));
    }

    #[test]
    fn only_stale_games_are_removed() {
        let old_game = new_game();
        let young_game = game_started_at(START + 100.0);
        let young_game_id = young_game.id.clone();
        let mut games = HashMap::new();
        games.insert(old_game.id.clone(), old_game);
        games.insert(young_game.id.clone(), young_game);

        let removed =
            remove_stale_games(&mut games, START + GAME_SECONDS + GAME_RESULT_GRACE_SECONDS);

        assert_eq!(removed.len(), 1);
        assert!(removed[0].is_finished());
        let remaining: Vec<&String> = games.keys().collect();
        assert_eq!(remaining, vec![&young_game_id]);
    }

    #[test]
    fn judged_guesses_and_hints_are_remembered() {
        let mut game = new_game();
        game.reveal_hint(HintKind::Parts, START);
        submit(&mut game, "sosu silen kaşık", 0.3, START);

        assert_eq!(game.revealed_hints()[0].text, "sıyır- + -gaç");
        let guesses = game.judged_guesses();
        assert_eq!(guesses.len(), 1);
        assert_eq!(guesses[0].text, "sosu silen kaşık");
        assert_eq!(guesses[0].closeness, closeness_percent(0.3));
    }

    #[test]
    fn too_short_guess_is_rejected() {
        let mut game = new_game();
        assert!(submit(&mut game, "a!", 0.99, START).rejected);
    }

    #[test]
    fn closeness_labels_cover_the_range() {
        assert_eq!(closeness_label(100), "Buldun!");
        assert_eq!(closeness_label(90), "Çok yakın");
        assert_eq!(closeness_label(70), "Yakın");
        assert_eq!(closeness_label(40), "Isınıyor");
        assert_eq!(closeness_label(5), "Uzak");
    }

    #[test]
    fn second_guess_must_wait_for_the_interval() {
        let mut game = new_game();
        submit(&mut game, "sosu silen kaşık", 0.3, START);

        let GuessCheck::TooSoon { seconds_to_wait } =
            game.check_guess("bir başka tahmin", START + 0.5)
        else {
            panic!("tahmin reddedilmeliydi");
        };
        assert_eq!(seconds_to_wait, GUESS_INTERVAL_SECONDS - 0.5);

        let response = submit(
            &mut game,
            "bir başka tahmin",
            0.3,
            START + GUESS_INTERVAL_SECONDS,
        );
        assert!(!response.rejected);
    }

    #[test]
    fn refused_early_guess_does_not_restart_the_wait() {
        let mut game = new_game();
        submit(&mut game, "sosu silen kaşık", 0.3, START);
        game.check_guess("erken tahmin", START + 1.0);

        let response = submit(
            &mut game,
            "bir başka tahmin",
            0.3,
            START + GUESS_INTERVAL_SECONDS,
        );

        assert_eq!(game.result().guess_count, 2);
        assert!(!response.rejected);
    }

    #[test]
    fn too_short_guess_also_starts_the_wait() {
        let mut game = new_game();
        submit(&mut game, "a!", 0.3, START);
        assert!(matches!(
            game.check_guess("bir başka tahmin", START + 1.0),
            GuessCheck::TooSoon { .. }
        ));
    }
}
