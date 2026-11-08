mod support;

use std::net::TcpListener;

use sozcuklemece_backend::encoder::TeiEncoder;
use support::FakeTei;

fn texts(count: usize) -> Vec<String> {
    let mut texts = Vec::new();
    for number in 0..count {
        texts.push(format!("metin {number}"));
    }
    texts
}

fn closed_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    address
}

#[tokio::test]
async fn requests_take_turns_between_replicas() {
    let first = FakeTei::start().await;
    let second = FakeTei::start().await;
    let encoder = TeiEncoder::new(&format!("{}, {}", first.url, second.url));

    for text in texts(4) {
        encoder.encode(&[text]).await.unwrap();
    }

    assert_eq!(first.encoded_texts().len(), 2);
    assert_eq!(second.encoded_texts().len(), 2);
}

#[tokio::test]
async fn unreachable_replica_is_skipped() {
    let working = FakeTei::start().await;
    let encoder = TeiEncoder::new(&format!("{},{}", closed_address(), working.url));

    for text in texts(3) {
        assert!(encoder.encode(&[text]).await.is_ok());
    }

    assert_eq!(working.encoded_texts().len(), 3);
}

#[tokio::test]
async fn all_replicas_unreachable_is_an_error() {
    let encoder = TeiEncoder::new(&format!("{},{}", closed_address(), closed_address()));
    assert!(encoder.encode(&texts(1)).await.is_err());
}
