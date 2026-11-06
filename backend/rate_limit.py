import ipaddress
import threading
from collections import deque

GAME_START_LIMIT = 60
GAME_START_WINDOW_SECONDS = 60
GUESS_LIMIT = 50
GUESS_WINDOW_SECONDS = 10
IPV6_SUBNET_SIZE = 64
UNKNOWN_CLIENT = "unknown"


def rate_limit_key(host):
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        return host
    if address.version == 4:
        return str(address)
    if address.ipv4_mapped is not None:
        return str(address.ipv4_mapped)
    subnet = ipaddress.ip_network((address, IPV6_SUBNET_SIZE), strict=False)
    return str(subnet)


class RateLimiter:
    def __init__(self, max_requests, window_seconds):
        self.max_requests = max_requests
        self.window_seconds = window_seconds
        self.recent_requests = {}
        self.next_cleanup_at = 0.0
        self.lock = threading.Lock()

    def has_expired(self, request_time, now):
        return now - request_time >= self.window_seconds

    def forget_idle_clients(self, now):
        if now < self.next_cleanup_at:
            return
        self.next_cleanup_at = now + self.window_seconds
        for client, request_times in list(self.recent_requests.items()):
            if self.has_expired(request_times[-1], now):
                del self.recent_requests[client]

    def register_request(self, client, now):
        with self.lock:
            self.forget_idle_clients(now)
            request_times = self.recent_requests.setdefault(client, deque())
            while request_times and self.has_expired(request_times[0], now):
                request_times.popleft()
            if len(request_times) >= self.max_requests:
                return request_times[0] + self.window_seconds - now
            request_times.append(now)
            return 0.0
