mod support;

use std::collections::HashMap;
use std::env;

use sozcuklemece_backend::encoder::TeiEncoder;
use sozcuklemece_backend::similarity::{MeaningJudge, PreparedWord, WIN_SIMILARITY};
use support::load_sample_puzzles;

const DEFAULT_TEI_URL: &str = "http://127.0.0.1:8080";

async fn prepared_words() -> (MeaningJudge, HashMap<String, PreparedWord>) {
    let tei_url = env::var("TEI_URL").unwrap_or(DEFAULT_TEI_URL.to_string());
    let judge = MeaningJudge::new(TeiEncoder::new(&tei_url));
    let mut words = HashMap::new();
    for puzzle in load_sample_puzzles() {
        let name = puzzle.word.clone();
        words.insert(name, judge.prepare(puzzle).await.expect("TEI çalışmıyor"));
    }
    (judge, words)
}

#[tokio::test]
#[ignore = "çalışan bir TEI sunucusu gerekir"]
async fn every_definition_wins_its_own_puzzle() {
    let (judge, words) = prepared_words().await;
    for word in words.values() {
        let similarity = judge
            .similarity(word, &word.puzzle.definition)
            .await
            .unwrap();
        assert!(similarity >= WIN_SIMILARITY, "{}", word.puzzle.word);
    }
}

#[tokio::test]
#[ignore = "çalışan bir TEI sunucusu gerekir"]
async fn every_wrong_meaning_loses_its_own_puzzle() {
    let (judge, words) = prepared_words().await;
    for word in words.values() {
        let mut wrong_meanings = word.puzzle.wrong_meanings.clone();
        wrong_meanings.extend(word.puzzle.opposite_meanings.iter().cloned());
        for wrong_meaning in &wrong_meanings {
            let similarity = judge.similarity(word, wrong_meaning).await.unwrap();
            assert!(similarity < WIN_SIMILARITY, "{wrong_meaning}");
        }
    }
}

#[tokio::test]
#[ignore = "çalışan bir TEI sunucusu gerekir"]
async fn casing_and_punctuation_do_not_change_the_result() {
    let (judge, words) = prepared_words().await;
    let word = &words["sıyırgaç"];
    let plain = judge
        .similarity(word, "sosu silen ekmek parçası")
        .await
        .unwrap();
    let shouted = judge
        .similarity(word, "SOSU SİLEN EKMEK PARÇASI!!!")
        .await
        .unwrap();
    assert!((plain - shouted).abs() < 1e-6);
}

#[tokio::test]
#[ignore = "çalışan bir TEI sunucusu gerekir"]
async fn correct_paraphrase_wins() {
    let (judge, words) = prepared_words().await;
    let similarity = judge
        .similarity(&words["önözlem"], "güzel bir an bitmeden onu özlemek")
        .await
        .unwrap();
    assert!(similarity >= WIN_SIMILARITY);
}

#[tokio::test]
#[ignore = "çalışan bir TEI sunucusu gerekir"]
async fn opposite_meaning_scores_high_on_definition_but_does_not_win() {
    let (judge, words) = prepared_words().await;
    let cases = [
        ("önözlem", "eski güzel günlere özlem duymak"),
        ("sorusavar", "her soruya dürüstçe cevap veren siyasetçi"),
    ];
    for (name, opposite_guess) in cases {
        let similarity = judge
            .similarity(&words[name], opposite_guess)
            .await
            .unwrap();
        assert!(similarity < WIN_SIMILARITY, "{opposite_guess}");
    }
}

#[tokio::test]
#[ignore = "çalışan bir TEI sunucusu gerekir"]
async fn unrelated_guess_is_far() {
    let (judge, words) = prepared_words().await;
    let similarity = judge
        .similarity(&words["kökağı"], "siyasi parti lideri")
        .await
        .unwrap();
    assert!(similarity < 0.3);
}
