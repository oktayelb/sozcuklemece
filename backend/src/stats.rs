use std::collections::BTreeMap;

use serde::Serialize;

use crate::game::Outcome;

pub const SCORE_BUCKETS: [(i64, i64); 5] = [(0, 0), (1, 399), (400, 599), (600, 799), (800, 1000)];

#[derive(Debug, PartialEq, Serialize)]
pub struct ScoreBucket {
    pub min_score: i64,
    pub max_score: i64,
    pub players: u64,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct StatsSummary {
    pub players: u64,
    pub win_percent: i64,
    pub average_score: i64,
    pub average_guesses: f64,
    pub average_winning_seconds: Option<i64>,
    pub better_than_percent: Option<i64>,
    pub score_buckets: Vec<ScoreBucket>,
}

fn percent(part: u64, whole: u64) -> i64 {
    (part as f64 / whole as f64 * 100.0).round() as i64
}

#[derive(Default)]
pub struct WordStats {
    players: u64,
    wins: u64,
    score_total: i64,
    guess_total: u64,
    winning_seconds_total: i64,
    players_by_score: BTreeMap<i64, u64>,
}

impl WordStats {
    pub fn add(&mut self, outcome: &Outcome) {
        self.players += 1;
        if outcome.won {
            self.wins += 1;
            self.winning_seconds_total += outcome.seconds_used;
        }
        self.score_total += outcome.score;
        self.guess_total += outcome.guess_count as u64;
        *self.players_by_score.entry(outcome.score).or_default() += 1;
    }

    fn players_scoring_between(&self, min_score: i64, max_score: i64) -> u64 {
        let mut count = 0;
        for (_, players) in self.players_by_score.range(min_score..=max_score) {
            count += players;
        }
        count
    }

    fn players_scoring_below(&self, score: i64) -> u64 {
        let mut count = 0;
        for (_, players) in self.players_by_score.range(..score) {
            count += players;
        }
        count
    }

    pub fn summary_for(&self, outcome: &Outcome) -> StatsSummary {
        let players = self.players.max(1);

        let mut average_winning_seconds = None;
        if self.wins > 0 {
            let average = self.winning_seconds_total as f64 / self.wins as f64;
            average_winning_seconds = Some(average.round() as i64);
        }

        let mut better_than_percent = None;
        let other_players = self.players.saturating_sub(1);
        if other_players > 0 {
            let players_below = self.players_scoring_below(outcome.score);
            better_than_percent = Some(percent(players_below, other_players));
        }

        let mut score_buckets = Vec::new();
        for (min_score, max_score) in SCORE_BUCKETS {
            score_buckets.push(ScoreBucket {
                min_score,
                max_score,
                players: self.players_scoring_between(min_score, max_score),
            });
        }

        let average_guesses = self.guess_total as f64 / players as f64;
        StatsSummary {
            players: self.players,
            win_percent: percent(self.wins, players),
            average_score: (self.score_total as f64 / players as f64).round() as i64,
            average_guesses: (average_guesses * 10.0).round() / 10.0,
            average_winning_seconds,
            better_than_percent,
            score_buckets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(won: bool, score: i64, seconds_used: i64, guess_count: u32) -> Outcome {
        Outcome {
            won,
            score,
            seconds_used,
            hints_used: 0,
            guess_count,
            best_closeness: 0,
        }
    }

    fn stats_of(outcomes: &[Outcome]) -> WordStats {
        let mut stats = WordStats::default();
        for outcome in outcomes {
            stats.add(outcome);
        }
        stats
    }

    #[test]
    fn first_player_has_no_one_to_compare_with() {
        let mine = outcome(true, 900, 30, 2);
        let summary = stats_of(&[mine]).summary_for(&mine);
        assert_eq!(summary.players, 1);
        assert_eq!(summary.win_percent, 100);
        assert_eq!(summary.better_than_percent, None);
    }

    #[test]
    fn averages_cover_every_player() {
        let mine = outcome(true, 900, 30, 2);
        let lost = outcome(false, 0, 180, 7);
        let other_win = outcome(true, 600, 90, 3);
        let summary = stats_of(&[mine, lost, other_win]).summary_for(&mine);

        assert_eq!(summary.players, 3);
        assert_eq!(summary.win_percent, 67);
        assert_eq!(summary.average_score, 500);
        assert_eq!(summary.average_guesses, 4.0);
        assert_eq!(summary.average_winning_seconds, Some(60));
    }

    #[test]
    fn nobody_winning_has_no_average_winning_time() {
        let lost = outcome(false, 0, 180, 4);
        let summary = stats_of(&[lost]).summary_for(&lost);
        assert_eq!(summary.average_winning_seconds, None);
        assert_eq!(summary.win_percent, 0);
    }

    #[test]
    fn better_than_counts_only_lower_scores_of_others() {
        let mine = outcome(true, 700, 60, 2);
        let summary = stats_of(&[
            mine,
            outcome(true, 700, 60, 2),
            outcome(false, 0, 180, 5),
            outcome(true, 900, 20, 1),
            outcome(true, 500, 120, 4),
        ])
        .summary_for(&mine);
        assert_eq!(summary.better_than_percent, Some(50));
    }

    #[test]
    fn buckets_split_scores_and_keep_losses_apart() {
        let mine = outcome(true, 1000, 0, 1);
        let summary = stats_of(&[
            mine,
            outcome(false, 0, 180, 5),
            outcome(true, 399, 170, 6),
            outcome(true, 400, 160, 6),
            outcome(true, 800, 40, 2),
        ])
        .summary_for(&mine);

        let counts: Vec<u64> = summary
            .score_buckets
            .iter()
            .map(|bucket| bucket.players)
            .collect();
        assert_eq!(counts, vec![1, 1, 1, 0, 2]);
    }
}
