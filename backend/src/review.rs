use std::collections::HashMap;

use serde::Serialize;

pub const CLOSE_CALL_CLOSENESS: i64 = 70;
pub const MAX_REVIEW_ITEMS: usize = 300;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReviewItem {
    pub text: String,
    pub closeness: i64,
    pub appeals: u32,
    pub seen: u32,
}

pub struct ReviewQueue {
    edition: Option<String>,
    items: HashMap<String, ReviewItem>,
}

impl Default for ReviewQueue {
    fn default() -> ReviewQueue {
        ReviewQueue::new()
    }
}

impl ReviewQueue {
    pub fn new() -> ReviewQueue {
        ReviewQueue {
            edition: None,
            items: HashMap::new(),
        }
    }

    fn follow(&mut self, edition: &str) {
        if self.edition.as_deref() == Some(edition) {
            return;
        }
        self.edition = Some(edition.to_string());
        self.items.clear();
    }

    fn item_for(&mut self, key: &str, text: &str, closeness: i64) -> Option<&mut ReviewItem> {
        if !self.items.contains_key(key) {
            if self.items.len() >= MAX_REVIEW_ITEMS {
                return None;
            }
            let item = ReviewItem {
                text: text.to_string(),
                closeness,
                appeals: 0,
                seen: 0,
            };
            self.items.insert(key.to_string(), item);
        }
        self.items.get_mut(key)
    }

    pub fn record_close_call(&mut self, edition: &str, key: &str, text: &str, closeness: i64) {
        self.follow(edition);
        if closeness < CLOSE_CALL_CLOSENESS {
            return;
        }
        if let Some(item) = self.item_for(key, text, closeness) {
            item.seen += 1;
            item.closeness = item.closeness.max(closeness);
        }
    }

    pub fn record_appeal(&mut self, edition: &str, key: &str, text: &str, closeness: i64) {
        self.follow(edition);
        if let Some(item) = self.item_for(key, text, closeness) {
            item.appeals += 1;
            item.closeness = item.closeness.max(closeness);
        }
    }

    pub fn remove(&mut self, edition: &str, text: &str) {
        self.follow(edition);
        self.items.retain(|_, item| item.text != text);
    }

    pub fn items(&self, edition: &str) -> Vec<ReviewItem> {
        if self.edition.as_deref() != Some(edition) {
            return Vec::new();
        }
        let mut items: Vec<ReviewItem> = self.items.values().cloned().collect();
        items.sort_by(|first, second| {
            second
                .appeals
                .cmp(&first.appeals)
                .then(second.closeness.cmp(&first.closeness))
                .then(first.text.cmp(&second.text))
        });
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDITION: &str = "bugun";

    #[test]
    fn far_guesses_are_not_close_calls() {
        let mut queue = ReviewQueue::new();
        queue.record_close_call(EDITION, "a", "a", CLOSE_CALL_CLOSENESS - 1);
        assert!(queue.items(EDITION).is_empty());
    }

    #[test]
    fn same_guess_is_counted_once_with_its_best_closeness() {
        let mut queue = ReviewQueue::new();
        queue.record_close_call(EDITION, "sosu silmek", "Sosu silmek", 75);
        queue.record_close_call(EDITION, "sosu silmek", "sosu silmek!", 80);
        let items = queue.items(EDITION);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "Sosu silmek");
        assert_eq!(items[0].closeness, 80);
        assert_eq!(items[0].seen, 2);
    }

    #[test]
    fn appeals_come_first() {
        let mut queue = ReviewQueue::new();
        queue.record_close_call(EDITION, "yakin", "yakın", 95);
        queue.record_appeal(EDITION, "itiraz", "itiraz", 55);
        let items = queue.items(EDITION);
        assert_eq!(items[0].text, "itiraz");
        assert_eq!(items[0].appeals, 1);
    }

    #[test]
    fn new_word_starts_an_empty_list() {
        let mut queue = ReviewQueue::new();
        queue.record_appeal(EDITION, "a", "a", 60);
        queue.record_appeal("yarin", "b", "b", 60);
        assert!(queue.items(EDITION).is_empty());
        assert_eq!(queue.items("yarin").len(), 1);
    }

    #[test]
    fn list_stops_growing_when_full() {
        let mut queue = ReviewQueue::new();
        for number in 0..MAX_REVIEW_ITEMS + 5 {
            let text = format!("tahmin {number}");
            queue.record_appeal(EDITION, &text, &text, 60);
        }
        assert_eq!(queue.items(EDITION).len(), MAX_REVIEW_ITEMS);
    }

    #[test]
    fn decided_guess_leaves_the_list() {
        let mut queue = ReviewQueue::new();
        queue.record_appeal(EDITION, "a", "A", 60);
        queue.remove(EDITION, "A");
        assert!(queue.items(EDITION).is_empty());
    }
}
