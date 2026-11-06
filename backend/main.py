import time
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, HTTPException, Request
from fastapi.staticfiles import StaticFiles

from backend.game import (
    GAME_SECONDS,
    HINT_LABELS,
    HINT_PENALTY,
    MAX_TIME_PENALTY,
    Game,
)
from backend.puzzles import (
    WORDS_PER_DAY,
    current_game_day,
    load_puzzles,
    puzzles_for_day,
)
from backend.schemas import (
    FinishResponse,
    GuessRequest,
    GuessResponse,
    HintOption,
    HintRequest,
    HintResponse,
    StartRequest,
    StartResponse,
    TodayResponse,
)
from backend.similarity import BertEncoder, MeaningJudge

FRONTEND_DIRECTORY = Path(__file__).parent.parent / "frontend"


@asynccontextmanager
async def lifespan(app):
    app.state.puzzles = load_puzzles()
    app.state.judge = MeaningJudge(BertEncoder(), app.state.puzzles)
    app.state.games = {}
    yield


app = FastAPI(title="Sözcüklemece", lifespan=lifespan)


def find_game(request, game_id):
    game = request.app.state.games.get(game_id)
    if game is None:
        raise HTTPException(status_code=404, detail="Oyun bulunamadı.")
    return game


@app.get("/api/today", response_model=TodayResponse)
def get_today():
    return TodayResponse(date=current_game_day().isoformat(), word_count=WORDS_PER_DAY)


@app.post("/api/games", response_model=StartResponse)
def start_game(body: StartRequest, request: Request):
    if body.slot >= WORDS_PER_DAY:
        raise HTTPException(status_code=404, detail="Bugün için böyle bir sözcük yok.")

    todays_puzzles = puzzles_for_day(request.app.state.puzzles, current_game_day())
    now = time.monotonic()
    game = Game(todays_puzzles[body.slot], started_at=now)
    request.app.state.games[game.id] = game

    hint_options = []
    for kind, label in HINT_LABELS.items():
        hint_options.append(HintOption(kind=kind, label=label))

    return StartResponse(
        game_id=game.id,
        word=game.puzzle["word"],
        hints=hint_options,
        duration_seconds=GAME_SECONDS,
        hint_penalty=HINT_PENALTY,
        points_lost_per_second=MAX_TIME_PENALTY / GAME_SECONDS,
        remaining_seconds=game.remaining_seconds(now),
        score=game.current_score(now),
    )


@app.post("/api/games/{game_id}/hints", response_model=HintResponse)
def reveal_hint(game_id: str, body: HintRequest, request: Request):
    game = find_game(request, game_id)
    return game.reveal_hint(body.kind, time.monotonic())


@app.post("/api/games/{game_id}/guesses", response_model=GuessResponse)
def submit_guess(game_id: str, body: GuessRequest, request: Request):
    game = find_game(request, game_id)
    return game.submit_guess(body.text, request.app.state.judge, time.monotonic())


@app.post("/api/games/{game_id}/finish", response_model=FinishResponse)
def finish_game(game_id: str, request: Request):
    game = find_game(request, game_id)
    game.finish(time.monotonic(), won=False)
    return game.result()


app.mount("/", StaticFiles(directory=FRONTEND_DIRECTORY, html=True), name="frontend")
