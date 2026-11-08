use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, NaiveDate, SecondsFormat};
use chrono_tz::Europe::Istanbul;
use serde::{Deserialize, Serialize};

use crate::encoder::EncoderError;
use crate::puzzle::Puzzle;
use crate::similarity::{MeaningJudge, PreparedWord};

pub const WORD_TTL_SECONDS: f64 = 24.0 * 60.0 * 60.0;

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

#[derive(Debug)]
pub enum StoreError {
    Encoder(EncoderError),
    File(io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StoreError::Encoder(error) => write!(formatter, "{error}"),
            StoreError::File(error) => write!(formatter, "sözcük dosyası: {error}"),
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

#[derive(Serialize, Deserialize)]
struct SavedWord {
    puzzle: Puzzle,
    expires_at: f64,
}

fn read_saved_word(word_file: &Path) -> io::Result<Option<SavedWord>> {
    if !word_file.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(word_file)?;
    let saved: SavedWord = serde_json::from_str(&contents).map_err(io::Error::other)?;
    Ok(Some(saved))
}

fn save_word(word_file: &Path, puzzle: &Puzzle, expires_at: f64) -> io::Result<()> {
    if let Some(directory) = word_file.parent() {
        fs::create_dir_all(directory)?;
    }
    let saved = SavedWord {
        puzzle: puzzle.clone(),
        expires_at,
    };
    let contents = serde_json::to_string(&saved).map_err(io::Error::other)?;
    let temporary_file = word_file.with_extension("tmp");
    fs::write(&temporary_file, contents)?;
    fs::rename(&temporary_file, word_file)
}

fn delete_word_file(word_file: &Path) {
    match fs::remove_file(word_file) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("Süresi dolan sözcük dosyası silinemedi: {error}"),
    }
}

#[derive(Clone)]
pub struct LiveWord {
    pub word: Arc<PreparedWord>,
    pub expires_at: f64,
}

pub struct DailyWordStore {
    judge: Arc<MeaningJudge>,
    word_file: PathBuf,
    live: Mutex<Option<LiveWord>>,
}

impl DailyWordStore {
    pub fn new(judge: Arc<MeaningJudge>, word_file: PathBuf) -> DailyWordStore {
        DailyWordStore {
            judge,
            word_file,
            live: Mutex::new(None),
        }
    }

    pub async fn load_saved_word(&self, now: f64) -> Result<(), StoreError> {
        let Some(saved) = read_saved_word(&self.word_file)? else {
            return Ok(());
        };
        if now >= saved.expires_at {
            fs::remove_file(&self.word_file)?;
            return Ok(());
        }
        let word = self.judge.prepare(saved.puzzle).await?;
        *self.live.lock().unwrap() = Some(LiveWord {
            word: Arc::new(word),
            expires_at: saved.expires_at,
        });
        Ok(())
    }

    pub async fn publish(&self, puzzle: Puzzle, now: f64) -> Result<(), StoreError> {
        let word = self.judge.prepare(puzzle).await?;
        let expires_at = now + WORD_TTL_SECONDS;
        let mut live = self.live.lock().unwrap();
        save_word(&self.word_file, &word.puzzle, expires_at)?;
        *live = Some(LiveWord {
            word: Arc::new(word),
            expires_at,
        });
        self.judge.forget_guesses();
        Ok(())
    }

    pub fn drop_expired_word(&self, now: f64) {
        let mut live = self.live.lock().unwrap();
        let has_expired = match live.as_ref() {
            Some(live_word) => now >= live_word.expires_at,
            None => false,
        };
        if has_expired {
            *live = None;
            delete_word_file(&self.word_file);
            self.judge.forget_guesses();
        }
    }

    pub fn current_word(&self, now: f64) -> Option<LiveWord> {
        self.drop_expired_word(now);
        self.live.lock().unwrap().clone()
    }
}
