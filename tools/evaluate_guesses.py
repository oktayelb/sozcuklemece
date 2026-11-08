import json
import sys
from pathlib import Path

from backend.similarity import WIN_SIMILARITY, BertEncoder, MeaningJudge

SAMPLE_PUZZLES_FILE = Path(__file__).with_name("sample_puzzles.json")
DEFAULT_GUESSES_FILE = Path(__file__).with_name("sample_guesses.json")
LABELS_THAT_SHOULD_WIN = ("correct",)


def load_sample_puzzles():
    with open(SAMPLE_PUZZLES_FILE, encoding="utf-8") as puzzles_file:
        return json.load(puzzles_file)


def load_guesses(path):
    with open(path, encoding="utf-8") as guesses_file:
        return json.load(guesses_file)


def show_mistakes(word, label, guesses, judge):
    wins = 0
    for guess in guesses:
        similarity = judge.similarity(word, guess)
        won = similarity >= WIN_SIMILARITY
        if won:
            wins += 1
        if won != (label in LABELS_THAT_SHOULD_WIN):
            print(f"  {similarity:5.2f}  [{label}] {word.puzzle['word']}: {guess}")
    return wins


def main():
    guesses_path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_GUESSES_FILE
    guesses_by_word = load_guesses(guesses_path)
    judge = MeaningJudge(BertEncoder())

    wins_by_label = {}
    totals_by_label = {}
    print(f"Hatalı kararlar (kazanma eşiği {WIN_SIMILARITY}):")
    for puzzle in load_sample_puzzles():
        word = judge.prepare(puzzle)
        for label, guesses in guesses_by_word[puzzle["word"]].items():
            wins = show_mistakes(word, label, guesses, judge)
            wins_by_label[label] = wins_by_label.get(label, 0) + wins
            totals_by_label[label] = totals_by_label.get(label, 0) + len(guesses)

    print("\nKazanma oranları:")
    for label, total in totals_by_label.items():
        wins = wins_by_label[label]
        print(f"  {label:9s} {wins:3d} / {total:3d}  (%{round(wins / total * 100)})")


if __name__ == "__main__":
    main()
