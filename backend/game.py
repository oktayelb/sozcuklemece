import uuid

from backend.similarity import WIN_SIMILARITY, closeness_percent, normalize_text

GAME_SECONDS = 180
GAME_RESULT_GRACE_SECONDS = 120
BASE_SCORE = 1000
MAX_TIME_PENALTY = 500
HINT_PENALTY = 150
MIN_GUESS_LENGTH = 3
MAX_GUESS_LENGTH = 200
WIN_CLOSENESS = 100

HINT_LABELS = {
    "parts": "Kök ve ekler",
    "sentence": "Cümlede kullanımı",
    "category": "Kategori ve alan",
}


def closeness_label(closeness):
    if closeness >= WIN_CLOSENESS:
        return "Buldun!"
    if closeness >= 85:
        return "Çok yakın"
    if closeness >= 60:
        return "Yakın"
    if closeness >= 30:
        return "Isınıyor"
    return "Uzak"


def compact_text(text):
    return normalize_text(text).replace(" ", "").replace("-", "")


def repeats_the_word(guess, puzzle):
    compact_guess = compact_text(guess)
    forbidden_forms = [puzzle["word"]] + puzzle.get("inflected_stems", [])
    for form in forbidden_forms:
        compact_form = compact_text(form)
        if compact_form and compact_form in compact_guess:
            return True
    return False


def remove_stale_games(games, now):
    for game_id, game in list(games.items()):
        if game.is_stale(now):
            del games[game_id]


class Game:
    def __init__(self, word, started_at):
        self.id = uuid.uuid4().hex
        self.word = word
        self.puzzle = word.puzzle
        self.started_at = started_at
        self.used_hints = []
        self.guess_count = 0
        self.best_closeness = 0
        self.finished = False
        self.won = False
        self.final_score = 0
        self.seconds_used = 0.0

    def is_stale(self, now):
        return now - self.started_at >= GAME_SECONDS + GAME_RESULT_GRACE_SECONDS

    def remaining_seconds(self, now):
        if self.finished:
            return GAME_SECONDS - self.seconds_used
        elapsed = now - self.started_at
        return max(0.0, GAME_SECONDS - elapsed)

    def score_if_won_now(self, now):
        elapsed_share = 1 - self.remaining_seconds(now) / GAME_SECONDS
        time_penalty = MAX_TIME_PENALTY * elapsed_share
        hint_penalty = HINT_PENALTY * len(self.used_hints)
        return round(BASE_SCORE - time_penalty - hint_penalty)

    def current_score(self, now):
        if self.finished:
            return self.final_score
        return self.score_if_won_now(now)

    def finish(self, now, won):
        if self.finished:
            return
        if won:
            self.final_score = self.score_if_won_now(now)
        self.seconds_used = GAME_SECONDS - self.remaining_seconds(now)
        self.won = won
        self.finished = True

    def finish_if_time_is_up(self, now):
        if self.remaining_seconds(now) <= 0:
            self.finish(now, won=False)

    def hint_text(self, kind):
        if kind == "category":
            return f"{self.puzzle['category']} · {self.puzzle['field']}"
        return self.puzzle[kind]

    def reveal_hint(self, kind, now):
        self.finish_if_time_is_up(now)
        if self.finished:
            text = None
        else:
            text = self.hint_text(kind)
            if kind not in self.used_hints:
                self.used_hints.append(kind)
        return {
            "kind": kind,
            "text": text,
            "finished": self.finished,
            "remaining_seconds": self.remaining_seconds(now),
            "score": self.current_score(now),
        }

    def guess_response(self, now, message, closeness=None, rejected=False):
        return {
            "closeness": closeness,
            "message": message,
            "rejected": rejected,
            "won": self.won,
            "finished": self.finished,
            "remaining_seconds": self.remaining_seconds(now),
            "score": self.current_score(now),
        }

    def submit_guess(self, text, judge, now):
        self.finish_if_time_is_up(now)
        if self.finished:
            return self.guess_response(now, "Oyun bitti.")
        if len(normalize_text(text)) < MIN_GUESS_LENGTH:
            return self.guess_response(now, "Biraz daha açık yaz.", rejected=True)
        if repeats_the_word(text, self.puzzle):
            return self.guess_response(now, "Sözcüğün kendisini kullanmadan anlat.", rejected=True)

        similarity = judge.similarity(self.word, text)
        if similarity >= WIN_SIMILARITY:
            closeness = WIN_CLOSENESS
            self.finish(now, won=True)
        else:
            closeness = closeness_percent(similarity)
        self.guess_count += 1
        self.best_closeness = max(self.best_closeness, closeness)
        return self.guess_response(now, closeness_label(closeness), closeness=closeness)

    def result(self):
        return {
            "word": self.puzzle["word"],
            "definition": self.puzzle["definition"],
            "parts": self.puzzle["parts"],
            "sentence": self.puzzle["sentence"],
            "category": self.hint_text("category"),
            "won": self.won,
            "score": self.final_score,
            "seconds_used": round(self.seconds_used),
            "hints_used": len(self.used_hints),
            "guess_count": self.guess_count,
            "best_closeness": self.best_closeness,
        }
