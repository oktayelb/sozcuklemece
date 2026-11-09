mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use chrono::{NaiveDate, TimeZone, Utc};
use serde_json::Value;
use tempfile::TempDir;

use sozcuklemece_backend::daily_word::{DailyWordStore, day_after, day_end, game_day};
use sozcuklemece_backend::encoder::TeiEncoder;
use sozcuklemece_backend::game::{
    GAME_RESULT_GRACE_SECONDS, GAME_SECONDS, Game, remove_stale_games,
};
use sozcuklemece_backend::puzzle::Puzzle;
use sozcuklemece_backend::similarity::MeaningJudge;
use sozcuklemece_backend::text::normalize_text;
use support::{FakeTei, FakeTeiMode, first_puzzle, load_sample_puzzles, second_puzzle};

const START: f64 = 1_800_000_000.0;

struct StoreFixture {
    tei: FakeTei,
    judge: Arc<MeaningJudge>,
    store: DailyWordStore,
    directory: TempDir,
}

impl StoreFixture {
    async fn new() -> StoreFixture {
        let tei = FakeTei::start().await;
        let directory = TempDir::new().unwrap();
        let judge = Arc::new(MeaningJudge::new(TeiEncoder::new(&tei.url)));
        let store = DailyWordStore::new(judge.clone(), directory.path().join("data/schedule.json"));
        StoreFixture {
            tei,
            judge,
            store,
            directory,
        }
    }

    fn schedule_file(&self) -> std::path::PathBuf {
        self.directory.path().join("data/schedule.json")
    }

    async fn restarted_store(&self) -> (FakeTei, DailyWordStore) {
        let tei = FakeTei::start().await;
        let judge = Arc::new(MeaningJudge::new(TeiEncoder::new(&tei.url)));
        (tei, DailyWordStore::new(judge, self.schedule_file()))
    }
}

fn midnight_after(now: f64) -> f64 {
    day_end(game_day(now))
}

fn tomorrow_of(now: f64) -> NaiveDate {
    day_after(game_day(now))
}

fn saved_puzzle(schedule_file: &Path, day: NaiveDate) -> Puzzle {
    let saved: Value = serde_json::from_str(&fs::read_to_string(schedule_file).unwrap()).unwrap();
    serde_json::from_value(saved[day.to_string()].clone()).unwrap()
}

#[tokio::test]
async fn there_is_no_word_before_the_first_upload() {
    let fixture = StoreFixture::new().await;
    assert!(fixture.store.current_word(START).is_none());
}

#[tokio::test]
async fn published_word_is_served() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    assert_eq!(
        fixture.store.current_word(START).unwrap().word.puzzle,
        first_puzzle()
    );
}

#[tokio::test]
async fn new_word_replaces_the_old_one_in_memory_and_on_disk() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    fixture
        .store
        .publish(second_puzzle(), game_day(START + 10.0), START + 10.0)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .store
            .current_word(START + 10.0)
            .unwrap()
            .word
            .puzzle,
        second_puzzle()
    );
    assert_eq!(
        saved_puzzle(&fixture.schedule_file(), game_day(START)),
        second_puzzle()
    );
}

#[tokio::test]
async fn definition_is_processed_once_when_published() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let word = fixture.store.current_word(START).unwrap().word;

    for number in 0..5 {
        fixture
            .judge
            .similarity(&word, &format!("tahmin {number}"))
            .await
            .unwrap();
    }

    let normalized_definition = normalize_text(&first_puzzle().definition);
    let times_encoded = fixture
        .tei
        .encoded_texts()
        .iter()
        .filter(|text| **text == normalized_definition)
        .count();
    assert_eq!(times_encoded, 1);
}

#[tokio::test]
async fn guess_processes_only_the_guess() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let word = fixture.store.current_word(START).unwrap().word;
    let texts_after_publishing = fixture.tei.encoded_texts().len();

    fixture
        .judge
        .similarity(&word, "Yalnızca bu tahmin!")
        .await
        .unwrap();

    assert_eq!(
        fixture.tei.encoded_texts()[texts_after_publishing..],
        ["yalnızca bu tahmin".to_string()]
    );
}

#[tokio::test]
async fn word_lasts_until_midnight() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let midnight = midnight_after(START);
    let live = fixture.store.current_word(midnight - 1.0).unwrap();
    assert_eq!(live.expires_at, midnight);
    assert!(fixture.store.current_word(midnight).is_none());
}

#[tokio::test]
async fn past_day_is_removed_from_memory_and_disk() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();

    fixture.store.drop_past_days(midnight_after(START));

    assert!(fixture.store.current_word(START).is_none());
    assert!(!fixture.schedule_file().exists());
}

#[tokio::test]
async fn word_scheduled_for_tomorrow_waits_until_midnight() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    fixture
        .store
        .publish(second_puzzle(), tomorrow_of(START), START)
        .await
        .unwrap();
    let midnight = midnight_after(START);

    let before = fixture.store.current_word(midnight - 1.0).unwrap();
    let after = fixture.store.current_word(midnight).unwrap();

    assert_eq!(before.word.puzzle, first_puzzle());
    assert_eq!(after.word.puzzle, second_puzzle());
    assert_eq!(after.expires_at, midnight_after(midnight));
}

#[tokio::test]
async fn word_scheduled_days_ahead_is_prepared_the_day_before() {
    let fixture = StoreFixture::new().await;
    let third_day = tomorrow_of(midnight_after(START));
    fixture
        .store
        .publish(second_puzzle(), third_day, START)
        .await
        .unwrap();
    let day_before = midnight_after(START) + 60.0;
    fixture
        .store
        .prepare_upcoming_words(day_before)
        .await
        .unwrap();
    let encoded_before_midnight = fixture.tei.encoded_texts().len();

    let live = fixture
        .store
        .current_word(midnight_after(day_before))
        .unwrap();

    assert_eq!(live.word.puzzle, second_puzzle());
    assert_eq!(fixture.tei.encoded_texts().len(), encoded_before_midnight);
}

#[tokio::test]
async fn day_without_a_word_has_no_game() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(second_puzzle(), tomorrow_of(START), START)
        .await
        .unwrap();
    assert!(fixture.store.current_word(START).is_none());
}

#[tokio::test]
async fn past_day_and_far_future_are_refused() {
    let fixture = StoreFixture::new().await;
    let yesterday = game_day(START).pred_opt().unwrap();
    let far_future = game_day(START) + chrono::Days::new(91);
    assert!(
        fixture
            .store
            .publish(first_puzzle(), yesterday, START)
            .await
            .is_err()
    );
    assert!(
        fixture
            .store
            .publish(first_puzzle(), far_future, START)
            .await
            .is_err()
    );
    assert!(!fixture.schedule_file().exists());
}

#[tokio::test]
async fn removed_day_is_gone_from_memory_and_disk() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    fixture
        .store
        .publish(second_puzzle(), tomorrow_of(START), START)
        .await
        .unwrap();

    assert!(fixture.store.remove(tomorrow_of(START)).unwrap());

    assert_eq!(fixture.store.scheduled_words(START).len(), 1);
    let (_restarted_tei, restarted) = fixture.restarted_store().await;
    restarted.load_schedule(START).await.unwrap();
    assert_eq!(restarted.scheduled_words(START).len(), 1);
    assert!(fixture.store.current_word(midnight_after(START)).is_none());
}

#[tokio::test]
async fn saved_word_is_processed_again_after_a_restart() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let (restarted_tei, restarted) = fixture.restarted_store().await;

    restarted.load_schedule(START + 100.0).await.unwrap();

    assert_eq!(
        restarted.current_word(START + 100.0).unwrap().word.puzzle,
        first_puzzle()
    );
    assert!(
        restarted_tei
            .encoded_texts()
            .contains(&normalize_text(&first_puzzle().definition))
    );
}

#[tokio::test]
async fn schedule_survives_a_restart() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    fixture
        .store
        .publish(second_puzzle(), tomorrow_of(START), START)
        .await
        .unwrap();
    let (_restarted_tei, restarted) = fixture.restarted_store().await;

    restarted.load_schedule(START + 100.0).await.unwrap();

    let midnight = midnight_after(START);
    assert_eq!(
        restarted.current_word(midnight - 1.0).unwrap().word.puzzle,
        first_puzzle()
    );
    assert_eq!(
        restarted.current_word(midnight).unwrap().word.puzzle,
        second_puzzle()
    );
}

#[tokio::test]
async fn word_whose_day_passed_while_stopped_is_deleted_without_being_processed() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let (restarted_tei, restarted) = fixture.restarted_store().await;
    let midnight = midnight_after(START);

    restarted.load_schedule(midnight).await.unwrap();

    assert!(restarted_tei.encoded_texts().is_empty());
    assert!(restarted.current_word(midnight).is_none());
    assert!(!fixture.schedule_file().exists());
}

#[tokio::test]
async fn start_without_a_saved_word_leaves_the_store_empty() {
    let fixture = StoreFixture::new().await;
    fixture.store.load_schedule(START).await.unwrap();
    assert!(fixture.store.current_word(START).is_none());
}

#[tokio::test]
async fn failed_upload_keeps_the_live_word() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    fixture.tei.set_mode(FakeTeiMode::Broken);

    assert!(
        fixture
            .store
            .publish(second_puzzle(), game_day(START + 10.0), START + 10.0)
            .await
            .is_err()
    );

    assert_eq!(
        fixture
            .store
            .current_word(START + 10.0)
            .unwrap()
            .word
            .puzzle,
        first_puzzle()
    );
    assert_eq!(
        saved_puzzle(&fixture.schedule_file(), game_day(START)),
        first_puzzle()
    );
}

#[tokio::test]
async fn word_without_wrong_meanings_can_be_judged() {
    let fixture = StoreFixture::new().await;
    let puzzle = Puzzle {
        also_accepted: vec![],
        wrong_meanings: vec![],
        ..first_puzzle()
    };
    fixture
        .store
        .publish(puzzle.clone(), game_day(START), START)
        .await
        .unwrap();
    let word = fixture.store.current_word(START).unwrap().word;
    let similarity = fixture
        .judge
        .similarity(&word, &puzzle.definition)
        .await
        .unwrap();
    assert!((similarity - 1.0).abs() < 1e-6);
}

#[tokio::test]
async fn old_word_stays_for_a_running_game_then_leaves_memory() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let game = Game::new(
        fixture.store.current_word(START).unwrap().word,
        "0123456789abcdef".to_string(),
        START,
    );
    let old_word = Arc::downgrade(&game.word);
    let mut games = std::collections::HashMap::new();
    games.insert(game.id.clone(), game);

    fixture
        .store
        .publish(second_puzzle(), game_day(START + 10.0), START + 10.0)
        .await
        .unwrap();
    assert!(old_word.upgrade().is_some());

    remove_stale_games(&mut games, START + GAME_SECONDS + GAME_RESULT_GRACE_SECONDS);
    assert!(old_word.upgrade().is_none());
}

#[test]
fn sample_puzzles_fit_the_upload_form() {
    for puzzle in load_sample_puzzles() {
        assert_eq!(puzzle.cleaned().unwrap(), puzzle);
    }
}

#[test]
fn new_day_starts_at_midnight_in_turkey() {
    let before_midnight = Utc
        .with_ymd_and_hms(2026, 11, 6, 20, 59, 0)
        .unwrap()
        .timestamp() as f64;
    let after_midnight = Utc
        .with_ymd_and_hms(2026, 11, 6, 21, 1, 0)
        .unwrap()
        .timestamp() as f64;
    assert_eq!(
        game_day(before_midnight),
        NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
    );
    assert_eq!(
        game_day(after_midnight),
        NaiveDate::from_ymd_opt(2026, 11, 7).unwrap()
    );
}

fn times_encoded(tei: &FakeTei, normalized_text: &str) -> usize {
    let encoded_texts = tei.encoded_texts();
    encoded_texts
        .iter()
        .filter(|text| *text == normalized_text)
        .count()
}

#[tokio::test]
async fn repeated_guess_is_encoded_once() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let word = fixture.store.current_word(START).unwrap().word;

    fixture
        .judge
        .similarity(&word, "aynı tahmin")
        .await
        .unwrap();
    fixture
        .judge
        .similarity(&word, "AYNI tahmin!")
        .await
        .unwrap();

    assert_eq!(times_encoded(&fixture.tei, "aynı tahmin"), 1);
}

#[tokio::test]
async fn same_guess_sent_at_the_same_moment_is_encoded_once() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let word = fixture.store.current_word(START).unwrap().word;

    let (first, second, third) = tokio::join!(
        fixture.judge.similarity(&word, "aynı anda gelen tahmin"),
        fixture.judge.similarity(&word, "aynı anda gelen tahmin"),
        fixture.judge.similarity(&word, "aynı anda gelen tahmin"),
    );

    assert_eq!(first.unwrap(), second.unwrap());
    assert!(third.is_ok());
    assert_eq!(times_encoded(&fixture.tei, "aynı anda gelen tahmin"), 1);
}

#[tokio::test]
async fn publishing_forgets_cached_guesses() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let old_word = fixture.store.current_word(START).unwrap().word;
    fixture
        .judge
        .similarity(&old_word, "eski sözcük için tahmin")
        .await
        .unwrap();

    fixture
        .store
        .publish(second_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let new_word = fixture.store.current_word(START).unwrap().word;
    fixture
        .judge
        .similarity(&new_word, "eski sözcük için tahmin")
        .await
        .unwrap();

    assert_eq!(times_encoded(&fixture.tei, "eski sözcük için tahmin"), 2);
}

#[tokio::test]
async fn new_day_forgets_cached_guesses() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let word = fixture.store.current_word(START).unwrap().word;
    fixture.judge.similarity(&word, "bir tahmin").await.unwrap();

    fixture.store.drop_past_days(midnight_after(START));
    fixture.judge.similarity(&word, "bir tahmin").await.unwrap();

    assert_eq!(times_encoded(&fixture.tei, "bir tahmin"), 2);
}

#[tokio::test]
async fn failed_encoding_is_not_cached() {
    let fixture = StoreFixture::new().await;
    fixture
        .store
        .publish(first_puzzle(), game_day(START), START)
        .await
        .unwrap();
    let word = fixture.store.current_word(START).unwrap().word;
    fixture.tei.set_mode(FakeTeiMode::Overloaded);
    assert!(fixture.judge.similarity(&word, "bir tahmin").await.is_err());

    fixture.tei.set_mode(FakeTeiMode::Working);

    assert!(fixture.judge.similarity(&word, "bir tahmin").await.is_ok());
}
