import numpy as np

VECTOR_SIZE = 4096


class ExactMatchEncoder:
    def __init__(self):
        self.position_of_text = {}

    def encode(self, texts):
        vectors = np.zeros((len(texts), VECTOR_SIZE))
        for row, text in enumerate(texts):
            if text not in self.position_of_text:
                self.position_of_text[text] = len(self.position_of_text)
            vectors[row, self.position_of_text[text]] = 1.0
        return vectors


class RecordingEncoder(ExactMatchEncoder):
    def __init__(self):
        super().__init__()
        self.encoded_texts = []

    def encode(self, texts):
        self.encoded_texts.extend(texts)
        return super().encode(texts)


class BrokenEncoder(ExactMatchEncoder):
    def encode(self, texts):
        raise RuntimeError("model çalışmadı")
