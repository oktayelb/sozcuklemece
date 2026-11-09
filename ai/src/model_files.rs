use std::fs;
use std::path::Path;

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use serde::Deserialize;
use serde_json::Value;
use tokenizers::{Tokenizer, TruncationParams, TruncationStrategy};

const MODEL_FILE: &str = "onnx/model.onnx";
const CONFIG_FILE: &str = "config.json";
const SENTENCE_BERT_CONFIG_FILE: &str = "sentence_bert_config.json";
const TOKENIZER_FILE: &str = "tokenizer.json";

#[derive(Deserialize)]
struct ModelConfig {
    max_position_embeddings: usize,
    #[serde(default)]
    id2label: serde_json::Map<String, Value>,
}

#[derive(Deserialize)]
struct SentenceBertConfig {
    max_seq_length: usize,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => return Err(format!("{} okunamadı: {error}", path.display())),
    };
    match serde_json::from_str(&contents) {
        Ok(value) => Ok(value),
        Err(error) => Err(format!("{} çözülemedi: {error}", path.display())),
    }
}

fn read_model_config(model_directory: &Path) -> Result<ModelConfig, String> {
    read_json(&model_directory.join(CONFIG_FILE))
}

pub fn read_max_tokens(model_directory: &Path) -> Result<usize, String> {
    let config = read_model_config(model_directory)?;
    let sentence_bert_path = model_directory.join(SENTENCE_BERT_CONFIG_FILE);
    if !sentence_bert_path.exists() {
        return Ok(config.max_position_embeddings);
    }
    let sentence_bert: SentenceBertConfig = read_json(&sentence_bert_path)?;
    Ok(sentence_bert
        .max_seq_length
        .min(config.max_position_embeddings))
}

pub fn read_labels(model_directory: &Path) -> Result<Vec<String>, String> {
    let config = read_model_config(model_directory)?;
    let mut labels = vec![String::new(); config.id2label.len()];
    for (id, label) in config.id2label {
        let Ok(index) = id.parse::<usize>() else {
            return Err(format!("config.json içinde geçersiz etiket numarası: {id}"));
        };
        let Some(label) = label.as_str() else {
            return Err(format!("config.json içinde {id} etiketi metin değil"));
        };
        if index >= labels.len() {
            return Err(format!(
                "config.json içinde etiket numaraları sıralı değil: {id}"
            ));
        }
        labels[index] = label.to_string();
    }
    if labels.is_empty() {
        return Err("config.json içinde id2label yok, model sınıflandırıcı değil".to_string());
    }
    Ok(labels)
}

pub fn load_tokenizer(
    model_directory: &Path,
    truncate_at: Option<usize>,
) -> Result<Tokenizer, String> {
    let path = model_directory.join(TOKENIZER_FILE);
    let mut tokenizer = match Tokenizer::from_file(&path) {
        Ok(tokenizer) => tokenizer,
        Err(error) => return Err(format!("{} okunamadı: {error}", path.display())),
    };
    tokenizer.with_padding(None);
    let mut truncation = None;
    if let Some(max_length) = truncate_at {
        truncation = Some(TruncationParams {
            max_length,
            strategy: TruncationStrategy::LongestFirst,
            ..TruncationParams::default()
        });
    }
    if let Err(error) = tokenizer.with_truncation(truncation) {
        return Err(format!("kesme ayarı yapılamadı: {error}"));
    }
    Ok(tokenizer)
}

pub fn open_session(model_directory: &Path, threads: usize) -> Result<Session, String> {
    let path = model_directory.join(MODEL_FILE);
    let environment = match ort::init().build() {
        Ok(environment) => environment,
        Err(error) => return Err(format!("ONNX Runtime başlatılamadı: {error}")),
    };
    let builder = match Session::builder(&environment) {
        Ok(builder) => builder,
        Err(error) => return Err(format!("ONNX Runtime başlatılamadı: {error}")),
    };
    let builder = match builder.with_optimization_level(GraphOptimizationLevel::All) {
        Ok(builder) => builder,
        Err(error) => return Err(format!("eniyileme düzeyi ayarlanamadı: {error}")),
    };
    let mut builder = match builder.with_intra_threads(threads) {
        Ok(builder) => builder,
        Err(error) => return Err(format!("iş parçacığı sayısı ayarlanamadı: {error}")),
    };
    match builder.commit_from_file(&path) {
        Ok(session) => Ok(session),
        Err(error) => Err(format!("{} açılamadı: {error}", path.display())),
    }
}
