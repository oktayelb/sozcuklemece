use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub enum Clock {
    System {
        started: Instant,
    },
    Manual {
        unix_start: f64,
        elapsed: Mutex<f64>,
    },
}

impl Clock {
    pub fn system() -> Clock {
        Clock::System {
            started: Instant::now(),
        }
    }

    pub fn manual(unix_start: f64) -> Clock {
        Clock::Manual {
            unix_start,
            elapsed: Mutex::new(0.0),
        }
    }

    pub fn unix_seconds(&self) -> f64 {
        match self {
            Clock::System { .. } => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("sistem saati 1970'ten önce olamaz")
                .as_secs_f64(),
            Clock::Manual {
                unix_start,
                elapsed,
            } => unix_start + *elapsed.lock().unwrap(),
        }
    }

    pub fn monotonic_seconds(&self) -> f64 {
        match self {
            Clock::System { started } => started.elapsed().as_secs_f64(),
            Clock::Manual { elapsed, .. } => *elapsed.lock().unwrap(),
        }
    }

    pub fn advance(&self, seconds: f64) {
        match self {
            Clock::System { .. } => panic!("sistem saati ileri alınamaz"),
            Clock::Manual { elapsed, .. } => *elapsed.lock().unwrap() += seconds,
        }
    }
}
