use std::thread;

use ort::session::Session;
use ort::value::Tensor;
use tokio::sync::{mpsc, oneshot};

const MAX_TEXTS_PER_BATCH: usize = 32;
const MAX_WAITING_REQUESTS: usize = 512;
const PADDING_TOKEN_ID: i64 = 0;
const INPUT_IDS: &str = "input_ids";
const ATTENTION_MASK: &str = "attention_mask";
const TOKEN_TYPE_IDS: &str = "token_type_ids";
const EMBEDDING_OUTPUT: &str = "last_hidden_state";
const CLASSIFICATION_OUTPUT: &str = "logits";

#[derive(Clone, Copy, PartialEq)]
pub enum Task {
    Embed,
    Classify,
}

pub struct TokenizedText {
    pub ids: Vec<i64>,
    pub type_ids: Vec<i64>,
}

pub enum InferenceError {
    Overloaded,
    Failed(String),
}

struct Job {
    texts: Vec<TokenizedText>,
    reply: oneshot::Sender<Result<Vec<Vec<f32>>, String>>,
}

pub struct InferenceQueue {
    sender: mpsc::Sender<Job>,
}

impl InferenceQueue {
    pub fn start(session: Session, task: Task) -> InferenceQueue {
        let (sender, receiver) = mpsc::channel(MAX_WAITING_REQUESTS);
        thread::spawn(move || run_jobs(session, task, receiver));
        InferenceQueue { sender }
    }

    pub async fn run(&self, texts: Vec<TokenizedText>) -> Result<Vec<Vec<f32>>, InferenceError> {
        let (reply, answer) = oneshot::channel();
        let job = Job { texts, reply };
        if let Err(error) = self.sender.try_send(job) {
            return match error {
                mpsc::error::TrySendError::Full(_) => Err(InferenceError::Overloaded),
                mpsc::error::TrySendError::Closed(_) => Err(InferenceError::Failed(
                    "çıkarım iş parçacığı durdu".to_string(),
                )),
            };
        }
        match answer.await {
            Ok(Ok(outputs)) => Ok(outputs),
            Ok(Err(reason)) => Err(InferenceError::Failed(reason)),
            Err(_) => Err(InferenceError::Failed(
                "çıkarım iş parçacığı durdu".to_string(),
            )),
        }
    }
}

fn run_jobs(mut session: Session, task: Task, mut receiver: mpsc::Receiver<Job>) {
    let mut input_names = Vec::new();
    for input in session.inputs() {
        input_names.push(input.name().to_string());
    }

    while let Some(first_job) = receiver.blocking_recv() {
        let mut text_count = first_job.texts.len();
        let mut jobs = vec![first_job];
        while text_count < MAX_TEXTS_PER_BATCH {
            match receiver.try_recv() {
                Ok(job) => {
                    text_count += job.texts.len();
                    jobs.push(job);
                }
                Err(_) => break,
            }
        }

        let mut batch = Vec::with_capacity(text_count);
        let mut replies = Vec::with_capacity(jobs.len());
        for job in jobs {
            replies.push((job.reply, job.texts.len()));
            batch.extend(job.texts);
        }

        match run_batch(&mut session, task, &input_names, &batch) {
            Ok(mut outputs) => {
                for (reply, count) in replies {
                    let rest = outputs.split_off(count);
                    let _ = reply.send(Ok(outputs));
                    outputs = rest;
                }
            }
            Err(reason) => {
                for (reply, _) in replies {
                    let _ = reply.send(Err(reason.clone()));
                }
            }
        }
    }
}

struct PaddedBatch {
    rows: usize,
    columns: usize,
    ids: Vec<i64>,
    attention_mask: Vec<i64>,
    type_ids: Vec<i64>,
}

fn pad(texts: &[TokenizedText]) -> PaddedBatch {
    let mut columns = 0;
    for text in texts {
        columns = columns.max(text.ids.len());
    }
    let mut batch = PaddedBatch {
        rows: texts.len(),
        columns,
        ids: Vec::with_capacity(texts.len() * columns),
        attention_mask: Vec::with_capacity(texts.len() * columns),
        type_ids: Vec::with_capacity(texts.len() * columns),
    };
    for text in texts {
        let padding = columns - text.ids.len();
        batch.ids.extend(&text.ids);
        batch.ids.extend(vec![PADDING_TOKEN_ID; padding]);
        batch.attention_mask.extend(vec![1; text.ids.len()]);
        batch.attention_mask.extend(vec![0; padding]);
        batch.type_ids.extend(&text.type_ids);
        batch.type_ids.extend(vec![0; padding]);
    }
    batch
}

fn run_batch(
    session: &mut Session,
    task: Task,
    input_names: &[String],
    texts: &[TokenizedText],
) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let batch = pad(texts);
    let shape = [batch.rows, batch.columns];

    let mut inputs = Vec::new();
    for name in input_names {
        let values = match name.as_str() {
            INPUT_IDS => batch.ids.clone(),
            ATTENTION_MASK => batch.attention_mask.clone(),
            TOKEN_TYPE_IDS => batch.type_ids.clone(),
            other => return Err(format!("modelin beklenmeyen bir girdisi var: {other}")),
        };
        let tensor = match Tensor::from_array((shape, values)) {
            Ok(tensor) => tensor,
            Err(error) => return Err(error.to_string()),
        };
        inputs.push((name.clone(), tensor));
    }

    let outputs = match session.run(inputs) {
        Ok(outputs) => outputs,
        Err(error) => return Err(error.to_string()),
    };
    let output_name = match task {
        Task::Embed => EMBEDDING_OUTPUT,
        Task::Classify => CLASSIFICATION_OUTPUT,
    };
    let Some(output) = outputs.get(output_name) else {
        return Err(format!("modelin {output_name} çıktısı yok"));
    };
    let values = match output.try_extract_tensor::<f32>() {
        Ok((_, values)) => values,
        Err(error) => return Err(error.to_string()),
    };

    match task {
        Task::Embed => Ok(mean_pool(values, &batch)),
        Task::Classify => Ok(split_rows(values, batch.rows)),
    }
}

fn mean_pool(hidden_states: &[f32], batch: &PaddedBatch) -> Vec<Vec<f32>> {
    let hidden_size = hidden_states.len() / (batch.rows * batch.columns);
    let mut pooled = Vec::with_capacity(batch.rows);
    for row in 0..batch.rows {
        let mut sum = vec![0.0; hidden_size];
        let mut token_count = 0.0;
        for column in 0..batch.columns {
            let position = row * batch.columns + column;
            if batch.attention_mask[position] == 0 {
                continue;
            }
            token_count += 1.0;
            let start = position * hidden_size;
            for (index, value) in hidden_states[start..start + hidden_size].iter().enumerate() {
                sum[index] += value;
            }
        }
        for value in sum.iter_mut() {
            *value /= token_count;
        }
        pooled.push(sum);
    }
    pooled
}

fn split_rows(values: &[f32], rows: usize) -> Vec<Vec<f32>> {
    let row_length = values.len() / rows;
    let mut split = Vec::with_capacity(rows);
    for row in values.chunks(row_length) {
        split.push(row.to_vec());
    }
    split
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(ids: &[i64]) -> TokenizedText {
        TokenizedText {
            ids: ids.to_vec(),
            type_ids: vec![0; ids.len()],
        }
    }

    #[test]
    fn shorter_texts_are_padded_and_masked() {
        let batch = pad(&[text(&[2, 7, 3]), text(&[2, 3])]);
        assert_eq!(batch.columns, 3);
        assert_eq!(batch.ids, vec![2, 7, 3, 2, 3, 0]);
        assert_eq!(batch.attention_mask, vec![1, 1, 1, 1, 1, 0]);
    }

    #[test]
    fn padding_does_not_change_the_mean() {
        let batch = pad(&[text(&[2, 7, 3]), text(&[2, 3])]);
        let hidden_states = vec![
            1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0, 6.0, 60.0, 100.0, 1000.0,
        ];
        let pooled = mean_pool(&hidden_states, &batch);
        assert_eq!(pooled, vec![vec![2.0, 20.0], vec![5.0, 50.0]]);
    }
}
