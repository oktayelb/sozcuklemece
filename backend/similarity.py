import re
from functools import lru_cache

from sentence_transformers import SentenceTransformer

MODEL_NAME = "emrecan/bert-base-turkish-cased-mean-nli-stsb-tr"
WIN_SIMILARITY = 0.625
ZERO_CLOSENESS_SIMILARITY = 0.15
HIGHEST_CLOSENESS_WITHOUT_WIN = 99
GUESS_CACHE_SIZE = 20_000


def turkish_lower(text):
    return text.replace("I", "ı").replace("İ", "i").lower()


def normalize_text(text):
    lowered = turkish_lower(text)
    without_punctuation = re.sub(r"[^\w\s-]", " ", lowered)
    return " ".join(without_punctuation.split())


def closeness_percent(similarity):
    similarity_range = WIN_SIMILARITY - ZERO_CLOSENESS_SIMILARITY
    percent = round((similarity - ZERO_CLOSENESS_SIMILARITY) / similarity_range * 100)
    return max(0, min(percent, HIGHEST_CLOSENESS_WITHOUT_WIN))


class BertEncoder:
    def __init__(self):
        self.model = SentenceTransformer(MODEL_NAME, device="cpu")

    def encode(self, texts):
        return self.model.encode(texts, normalize_embeddings=True, show_progress_bar=False)


class PreparedWord:
    def __init__(self, puzzle, right_vectors, wrong_vectors):
        self.puzzle = puzzle
        self.right_vectors = right_vectors
        self.wrong_vectors = wrong_vectors


class MeaningJudge:
    def __init__(self, encoder):
        self.encoder = encoder
        self.encode_guess = lru_cache(maxsize=GUESS_CACHE_SIZE)(self.encode_single)

    def encode_all(self, texts):
        normalized_texts = [normalize_text(text) for text in texts]
        return self.encoder.encode(normalized_texts)

    def encode_single(self, normalized_text):
        return self.encoder.encode([normalized_text])[0]

    def prepare(self, puzzle):
        right_vectors = self.encode_all([puzzle["definition"]] + puzzle["also_accepted"])
        wrong_vectors = None
        if puzzle["wrong_meanings"]:
            wrong_vectors = self.encode_all(puzzle["wrong_meanings"])
        return PreparedWord(puzzle, right_vectors, wrong_vectors)

    def forget_guesses(self):
        self.encode_guess.cache_clear()

    def similarity(self, word, guess):
        guess_vector = self.encode_guess(normalize_text(guess))
        closest_right = float((word.right_vectors @ guess_vector).max())
        if word.wrong_vectors is None:
            return closest_right
        closest_wrong = float((word.wrong_vectors @ guess_vector).max())
        wrong_side_excess = max(0.0, closest_wrong - closest_right)
        return closest_right - wrong_side_excess
