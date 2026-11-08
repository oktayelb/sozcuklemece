use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr};

pub const GAME_START_LIMIT: usize = 60;
pub const GAME_START_WINDOW_SECONDS: f64 = 60.0;
pub const GUESS_LIMIT: usize = 50;
pub const GUESS_WINDOW_SECONDS: f64 = 10.0;
pub const ADMIN_LIMIT: usize = 10;
pub const ADMIN_WINDOW_SECONDS: f64 = 60.0;
pub const UNKNOWN_CLIENT: &str = "unknown";
const IPV6_SUBNET_SEGMENTS: usize = 4;

pub fn rate_limit_key(host: &str) -> String {
    let address: IpAddr = match host.parse() {
        Ok(address) => address,
        Err(_) => return host.to_string(),
    };
    match address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return mapped.to_string();
            }
            let mut segments = address.segments();
            for segment in &mut segments[IPV6_SUBNET_SEGMENTS..] {
                *segment = 0;
            }
            format!("{}/64", Ipv6Addr::from(segments))
        }
    }
}

pub struct RateLimiter {
    max_requests: usize,
    window_seconds: f64,
    recent_requests: HashMap<String, VecDeque<f64>>,
    next_cleanup_at: f64,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window_seconds: f64) -> RateLimiter {
        RateLimiter {
            max_requests,
            window_seconds,
            recent_requests: HashMap::new(),
            next_cleanup_at: 0.0,
        }
    }

    fn forget_idle_clients(&mut self, now: f64) {
        if now < self.next_cleanup_at {
            return;
        }
        self.next_cleanup_at = now + self.window_seconds;
        let window_seconds = self.window_seconds;
        self.recent_requests.retain(|_, request_times| {
            let last_request = *request_times.back().unwrap();
            now - last_request < window_seconds
        });
    }

    pub fn register_request(&mut self, client: &str, now: f64) -> f64 {
        self.forget_idle_clients(now);
        let window_seconds = self.window_seconds;
        let max_requests = self.max_requests;
        let request_times = self.recent_requests.entry(client.to_string()).or_default();
        while let Some(&oldest) = request_times.front() {
            if now - oldest < window_seconds {
                break;
            }
            request_times.pop_front();
        }
        if request_times.len() >= max_requests {
            return request_times[0] + window_seconds - now;
        }
        request_times.push_back(now);
        0.0
    }

    pub fn known_clients(&self) -> Vec<String> {
        self.recent_requests.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: &str = "203.0.113.7";
    const OTHER_CLIENT: &str = "203.0.113.8";
    const START: f64 = 1000.0;

    fn new_limiter() -> RateLimiter {
        RateLimiter::new(2, 10.0)
    }

    fn use_up_limit(limiter: &mut RateLimiter) {
        assert_eq!(limiter.register_request(CLIENT, START), 0.0);
        assert_eq!(limiter.register_request(CLIENT, START + 1.0), 0.0);
    }

    #[test]
    fn request_over_the_limit_gets_the_wait_time() {
        let mut limiter = new_limiter();
        use_up_limit(&mut limiter);
        assert_eq!(limiter.register_request(CLIENT, START + 4.0), 6.0);
    }

    #[test]
    fn window_frees_one_request_at_a_time() {
        let mut limiter = new_limiter();
        use_up_limit(&mut limiter);
        assert_eq!(limiter.register_request(CLIENT, START + 10.0), 0.0);
        assert_eq!(limiter.register_request(CLIENT, START + 10.0), 1.0);
    }

    #[test]
    fn refused_requests_do_not_extend_the_wait() {
        let mut limiter = new_limiter();
        use_up_limit(&mut limiter);
        for _ in 0..100 {
            limiter.register_request(CLIENT, START + 5.0);
        }
        assert_eq!(limiter.register_request(CLIENT, START + 10.0), 0.0);
    }

    #[test]
    fn clients_have_separate_limits() {
        let mut limiter = new_limiter();
        use_up_limit(&mut limiter);
        assert_eq!(limiter.register_request(OTHER_CLIENT, START + 1.0), 0.0);
    }

    #[test]
    fn idle_clients_are_forgotten() {
        let mut limiter = new_limiter();
        limiter.register_request(CLIENT, START);
        limiter.register_request(OTHER_CLIENT, START + 20.0);
        assert_eq!(limiter.known_clients(), vec![OTHER_CLIENT.to_string()]);
    }

    #[test]
    fn cleanup_does_not_reset_a_client_still_at_its_limit() {
        let mut limiter = new_limiter();
        limiter.register_request(OTHER_CLIENT, START);
        limiter.register_request(CLIENT, START + 8.0);
        limiter.register_request(CLIENT, START + 9.0);
        limiter.register_request(OTHER_CLIENT, START + 10.0);
        assert_eq!(limiter.register_request(CLIENT, START + 11.0), 7.0);
    }

    #[test]
    fn rate_limit_keys() {
        let cases = [
            ("203.0.113.7", "203.0.113.7"),
            ("::ffff:203.0.113.7", "203.0.113.7"),
            ("2001:db8:1:2:aaaa::1", "2001:db8:1:2::/64"),
            ("2001:db8:1:2:bbbb::2", "2001:db8:1:2::/64"),
            ("2001:db8:1:3::1", "2001:db8:1:3::/64"),
            ("testclient", "testclient"),
        ];
        for (host, expected_key) in cases {
            assert_eq!(rate_limit_key(host), expected_key);
        }
    }

    #[test]
    fn ipv4_clients_on_a_dual_stack_socket_stay_separate() {
        assert_ne!(
            rate_limit_key("::ffff:203.0.113.7"),
            rate_limit_key("::ffff:203.0.113.8")
        );
    }
}
