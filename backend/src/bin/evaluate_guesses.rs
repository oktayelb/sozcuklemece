use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use sozcuklemece_backend::encoder::TeiEncoder;
use sozcuklemece_backend::puzzle::Puzzle;
use sozcuklemece_backend::similarity::{MeaningJudge, PreparedWord, WIN_SIMILARITY};

const DEFAULT_TEI_URL: &str = "http://127.0.0.1:8080";
const LABELS_THAT_SHOULD_WIN: [&str; 1] = ["correct"];
const LABEL_ORDER: [&str; 4] = ["correct", "partial", "wrong", "opposite"];

type GuessesByWord = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn tools_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tools")
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} okunamadı: {error}", path.display()));
    serde_json::from_str(&contents)
        .unwrap_or_else(|error| panic!("{} çözülemedi: {error}", path.display()))
}

async fn count_wins(
    judge: &MeaningJudge,
    word: &PreparedWord,
    label: &str,
    guesses: &[String],
) -> usize {
    let mut wins = 0;
    for guess in guesses {
        let similarity = judge
            .similarity(word, guess)
            .await
            .expect("TEI tahmini değerlendiremedi");
        let won = similarity >= WIN_SIMILARITY;
        if won {
            wins += 1;
        }
        if won != LABELS_THAT_SHOULD_WIN.contains(&label) {
            println!(
                "  {similarity:5.2}  [{label}] {}: {guess}",
                word.puzzle.word
            );
        }
    }
    wins
}

#[tokio::main]
async fn main() {
    let guesses_path = match env::args().nth(1) {
        Some(path) => PathBuf::from(path),
        None => tools_directory().join("sample_guesses.json"),
    };
    let tei_url = env::var("TEI_URL").unwrap_or(DEFAULT_TEI_URL.to_string());
    let puzzles: Vec<Puzzle> = read_json(&tools_directory().join("sample_puzzles.json"));
    let guesses_by_word: GuessesByWord = read_json(&guesses_path);
    let judge = MeaningJudge::new(TeiEncoder::new(&tei_url));

    let mut wins_by_label: BTreeMap<String, usize> = BTreeMap::new();
    let mut totals_by_label: BTreeMap<String, usize> = BTreeMap::new();
    println!("Hatalı kararlar (kazanma eşiği {WIN_SIMILARITY}):");
    for puzzle in puzzles {
        let word_name = puzzle.word.clone();
        let word = judge
            .prepare(puzzle)
            .await
            .expect("TEI sözcüğü hazırlayamadı");
        let Some(guesses_by_label) = guesses_by_word.get(&word_name) else {
            continue;
        };
        for (label, guesses) in guesses_by_label {
            let wins = count_wins(&judge, &word, label, guesses).await;
            *wins_by_label.entry(label.clone()).or_default() += wins;
            *totals_by_label.entry(label.clone()).or_default() += guesses.len();
        }
    }

    println!("\nKazanma oranları:");
    for label in LABEL_ORDER {
        let Some(total) = totals_by_label.get(label) else {
            continue;
        };
        let wins = wins_by_label[label];
        let percent = (wins as f64 / *total as f64 * 100.0).round();
        println!("  {label:9} {wins:3} / {total:3}  (%{percent})");
    }
}
