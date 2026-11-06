import pytest

from backend.rate_limit import RateLimiter, rate_limit_key

CLIENT = "203.0.113.7"
OTHER_CLIENT = "203.0.113.8"
START = 1000.0


def new_limiter():
    return RateLimiter(max_requests=2, window_seconds=10)


def use_up_limit(limiter):
    assert limiter.register_request(CLIENT, START) == 0
    assert limiter.register_request(CLIENT, START + 1) == 0


def test_request_over_the_limit_gets_the_wait_time():
    limiter = new_limiter()
    use_up_limit(limiter)
    assert limiter.register_request(CLIENT, START + 4) == 6


def test_window_frees_one_request_at_a_time():
    limiter = new_limiter()
    use_up_limit(limiter)
    assert limiter.register_request(CLIENT, START + 10) == 0
    assert limiter.register_request(CLIENT, START + 10) == 1


def test_refused_requests_do_not_extend_the_wait():
    limiter = new_limiter()
    use_up_limit(limiter)
    for _ in range(100):
        limiter.register_request(CLIENT, START + 5)
    assert limiter.register_request(CLIENT, START + 10) == 0


def test_clients_have_separate_limits():
    limiter = new_limiter()
    use_up_limit(limiter)
    assert limiter.register_request(OTHER_CLIENT, START + 1) == 0


def test_idle_clients_are_forgotten():
    limiter = new_limiter()
    limiter.register_request(CLIENT, START)
    limiter.register_request(OTHER_CLIENT, START + 20)
    assert list(limiter.recent_requests) == [OTHER_CLIENT]


def test_cleanup_does_not_reset_a_client_still_at_its_limit():
    limiter = new_limiter()
    limiter.register_request(OTHER_CLIENT, START)
    limiter.register_request(CLIENT, START + 8)
    limiter.register_request(CLIENT, START + 9)
    limiter.register_request(OTHER_CLIENT, START + 10)
    assert limiter.register_request(CLIENT, START + 11) == 7


@pytest.mark.parametrize(
    "host, expected_key",
    [
        ("203.0.113.7", "203.0.113.7"),
        ("::ffff:203.0.113.7", "203.0.113.7"),
        ("2001:db8:1:2:aaaa::1", "2001:db8:1:2::/64"),
        ("2001:db8:1:2:bbbb::2", "2001:db8:1:2::/64"),
        ("2001:db8:1:3::1", "2001:db8:1:3::/64"),
        ("testclient", "testclient"),
    ],
)
def test_rate_limit_key(host, expected_key):
    assert rate_limit_key(host) == expected_key


def test_ipv4_clients_on_a_dual_stack_socket_stay_separate():
    first_client = rate_limit_key("::ffff:203.0.113.7")
    second_client = rate_limit_key("::ffff:203.0.113.8")
    assert first_client != second_client
