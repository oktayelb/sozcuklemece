use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Days, NaiveDate, SecondsFormat, TimeZone};
use chrono_tz::Europe::Istanbul;
use serde::Deserialize;

use crate::encoder::EncoderError;
use crate::puzzle::{MAX_LIST_LENGTH, Puzzle};
use crate::similarity::{MeaningJudge, PreparedWord};

pub const MAX_DAYS_AHEAD: u64 = 90;

fn moment(unix_seconds: f64) -> DateTime<chrono_tz::Tz> {
    let milliseconds = (unix_seconds * 1000.0).round() as i64;
    DateTime::from_timestamp_millis(milliseconds)
        .expect("zaman damgası geçerli aralıkta olmalı")
        .with_timezone(&Istanbul)
}

pub fn game_day(unix_seconds: f64) -> NaiveDate {
    moment(unix_seconds).date_naive()
}

pub fn moment_in_game_timezone(unix_seconds: f64) -> String {
    moment(unix_seconds).to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn day_after(day: NaiveDate) -> NaiveDate {
    day.succ_opt().expect("takvimin sonuna gelinmedi")
}

pub fn day_end(day: NaiveDate) -> f64 {
    let next_midnight = day_after(day)
        .and_hms_opt(0, 0, 0)
        .expect("gece yarısı geçerli bir saat");
    let start_of_next_day = Istanbul
        .from_local_datetime(&next_midnight)
        .earliest()
        .expect("Türkiye saatinde her gün gece yarısıyla başlar");
    start_of_next_day.timestamp() as f64
}

pub fn last_day_to_schedule(today: NaiveDate) -> NaiveDate {
    today
        .checked_add_days(Days::new(MAX_DAYS_AHEAD))
        .expect("takvimin sonuna gelinmedi")
}

#[derive(Debug)]
pub enum StoreError {
    Encoder(EncoderError),
    File(io::Error),
    ListFull,
    DayOutOfRange,
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StoreError::Encoder(error) => write!(formatter, "{error}"),
            StoreError::File(error) => write!(formatter, "sözcük takvimi dosyası: {error}"),
            StoreError::ListFull => write!(formatter, "liste dolu"),
            StoreError::DayOutOfRange => write!(formatter, "gün geçmişte ya da çok ileride"),
        }
    }
}

impl From<EncoderError> for StoreError {
    fn from(error: EncoderError) -> StoreError {
        StoreError::Encoder(error)
    }
}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> StoreError {
        StoreError::File(error)
    }
}

type PuzzlesByDay = BTreeMap<NaiveDate, Puzzle>;

fn read_schedule(schedule_file: &Path) -> io::Result<PuzzlesByDay> {
    if !schedule_file.exists() {
        return Ok(PuzzlesByDay::new());
    }
    let contents = fs::read_to_string(schedule_file)?;
    serde_json::from_str(&contents).map_err(io::Error::other)
}

fn save_schedule(schedule_file: &Path, puzzles: &PuzzlesByDay) -> io::Result<()> {
    if puzzles.is_empty() {
        return match fs::remove_file(schedule_file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
    }
    if let Some(directory) = schedule_file.parent() {
        fs::create_dir_all(directory)?;
    }
    let contents = serde_json::to_string(puzzles).map_err(io::Error::other)?;
    let temporary_file = schedule_file.with_extension("tmp");
    fs::write(&temporary_file, contents)?;
    fs::rename(&temporary_file, schedule_file)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeaningSide {
    Right,
    Opposite,
    Wrong,
}

#[derive(Clone)]
pub struct LiveWord {
    pub word: Arc<PreparedWord>,
    pub expires_at: f64,
}

pub struct ScheduledWord {
    pub day: NaiveDate,
    pub word: String,
}

#[derive(Default)]
struct Schedule {
    puzzles: PuzzlesByDay,
    prepared: BTreeMap<NaiveDate, Arc<PreparedWord>>,
}

pub struct DailyWordStore {
    judge: Arc<MeaningJudge>,
    schedule_file: PathBuf,
    schedule: Mutex<Schedule>,
}

impl DailyWordStore {
    pub fn new(judge: Arc<MeaningJudge>, schedule_file: PathBuf) -> DailyWordStore {
        DailyWordStore {
            judge,
            schedule_file,
            schedule: Mutex::new(Schedule::default()),
        }
    }

    pub async fn load_schedule(&self, now: f64) -> Result<(), StoreError> {
        let puzzles = read_schedule(&self.schedule_file)?;
        self.schedule.lock().unwrap().puzzles = puzzles;
        self.drop_past_days(now);
        self.prepare_upcoming_words(now).await
    }

    pub async fn prepare_upcoming_words(&self, now: f64) -> Result<(), StoreError> {
        let today = game_day(now);
        for day in [today, day_after(today)] {
            let puzzle_to_prepare = {
                let schedule = self.schedule.lock().unwrap();
                if schedule.prepared.contains_key(&day) {
                    None
                } else {
                    schedule.puzzles.get(&day).cloned()
                }
            };
            let Some(puzzle) = puzzle_to_prepare else {
                continue;
            };

            let word = self.judge.prepare(puzzle).await?;
            let mut schedule = self.schedule.lock().unwrap();
            let still_scheduled = schedule.puzzles.get(&day) == Some(&word.puzzle);
            if still_scheduled && !schedule.prepared.contains_key(&day) {
                schedule.prepared.insert(day, Arc::new(word));
            }
        }
        Ok(())
    }

    pub async fn publish(
        &self,
        puzzle: Puzzle,
        day: NaiveDate,
        now: f64,
    ) -> Result<(), StoreError> {
        let today = game_day(now);
        if day < today || day > last_day_to_schedule(today) {
            return Err(StoreError::DayOutOfRange);
        }
        let word = self.judge.prepare(puzzle).await?;

        let mut schedule = self.schedule.lock().unwrap();
        let mut puzzles = schedule.puzzles.clone();
        puzzles.insert(day, word.puzzle.clone());
        save_schedule(&self.schedule_file, &puzzles)?;
        schedule.puzzles = puzzles;
        if day <= day_after(today) {
            schedule.prepared.insert(day, Arc::new(word));
        }
        if day == today {
            self.judge.forget_guesses();
        }
        Ok(())
    }

    pub fn remove(&self, day: NaiveDate) -> Result<bool, StoreError> {
        let mut schedule = self.schedule.lock().unwrap();
        if !schedule.puzzles.contains_key(&day) {
            return Ok(false);
        }
        let mut puzzles = schedule.puzzles.clone();
        puzzles.remove(&day);
        save_schedule(&self.schedule_file, &puzzles)?;
        schedule.puzzles = puzzles;
        schedule.prepared.remove(&day);
        Ok(true)
    }

    pub fn scheduled_words(&self, now: f64) -> Vec<ScheduledWord> {
        self.drop_past_days(now);
        let schedule = self.schedule.lock().unwrap();
        let mut words = Vec::new();
        for (day, puzzle) in &schedule.puzzles {
            words.push(ScheduledWord {
                day: *day,
                word: puzzle.word.clone(),
            });
        }
        words
    }

    pub fn scheduled_puzzle(&self, day: NaiveDate) -> Option<Puzzle> {
        self.schedule.lock().unwrap().puzzles.get(&day).cloned()
    }

    pub async fn add_meaning(
        &self,
        side: MeaningSide,
        text: &str,
        now: f64,
    ) -> Result<Option<Arc<PreparedWord>>, StoreError> {
        let Some(live) = self.current_word(now) else {
            return Ok(None);
        };
        let today = game_day(now);
        let mut puzzle = live.word.puzzle.clone();
        let list = match side {
            MeaningSide::Right => &mut puzzle.also_accepted,
            MeaningSide::Opposite => &mut puzzle.opposite_meanings,
            MeaningSide::Wrong => &mut puzzle.wrong_meanings,
        };
        if !list.iter().any(|existing| existing == text) {
            if list.len() >= MAX_LIST_LENGTH {
                return Err(StoreError::ListFull);
            }
            list.push(text.to_string());
        }

        let mut word = self.judge.prepare(puzzle).await?;
        word.keep_edition_of(&live.word);
        let word = Arc::new(word);

        let mut schedule = self.schedule.lock().unwrap();
        let still_the_same_word = match schedule.prepared.get(&today) {
            Some(current_word) => current_word.is_same_edition(&word),
            None => false,
        };
        if !still_the_same_word {
            return Ok(None);
        }
        let mut puzzles = schedule.puzzles.clone();
        puzzles.insert(today, word.puzzle.clone());
        save_schedule(&self.schedule_file, &puzzles)?;
        schedule.puzzles = puzzles;
        schedule.prepared.insert(today, word.clone());
        Ok(Some(word))
    }

    pub fn drop_past_days(&self, now: f64) {
        let today = game_day(now);
        let mut schedule = self.schedule.lock().unwrap();
        let puzzle_count = schedule.puzzles.len();
        schedule.puzzles.retain(|day, _| *day >= today);
        let prepared_count = schedule.prepared.len();
        schedule.prepared.retain(|day, _| *day >= today);

        if schedule.puzzles.len() < puzzle_count {
            let saved = save_schedule(&self.schedule_file, &schedule.puzzles);
            if let Err(error) = saved {
                eprintln!("Geçmiş günlerin sözcükleri dosyadan silinemedi: {error}");
            }
        }
        if schedule.prepared.len() < prepared_count {
            self.judge.forget_guesses();
        }
    }

    pub fn current_word(&self, now: f64) -> Option<LiveWord> {
        self.drop_past_days(now);
        let today = game_day(now);
        let schedule = self.schedule.lock().unwrap();
        let word = schedule.prepared.get(&today)?;
        Some(LiveWord {
            word: word.clone(),
            expires_at: day_end(today),
        })
    }
}
