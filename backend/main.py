import math
import time
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, HTTPException, Request
from fastapi.staticfiles import StaticFiles

from backend.game import (
    GAME_SECONDS,
    HINT_LABELS,
    HINT_PENALTY,
    Game,
)
from backend.puzzles import (
    WORDS_PER_DAY,
    current_game_day,
    load_puzzles,
    puzzles_for_day,
)
from backend.rate_limit import (
    GAME_START_LIMIT,
    GAME_START_WINDOW_SECONDS,
    GUESS_LIMIT,
    GUESS_WINDOW_SECONDS,
    UNKNOWN_CLIENT,
    RateLimiter,
    rate_limit_key,
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
    app.state.game_start_limiter = RateLimiter(GAME_START_LIMIT, GAME_START_WINDOW_SECONDS)
    app.state.guess_limiter = RateLimiter(GUESS_LIMIT, GUESS_WINDOW_SECONDS)
    yield


app = FastAPI(title="Sözcüklemece", lifespan=lifespan)


def find_game(request, game_id):
    game = request.app.state.games.get(game_id)
    if game is None:
        raise HTTPException(status_code=404, detail="Oyun bulunamadı.")
    return game


def enforce_rate_limit(request, limiter):
    host = request.client.host if request.client else UNKNOWN_CLIENT
    seconds_to_wait = limiter.register_request(rate_limit_key(host), time.monotonic())
    if seconds_to_wait > 0:
        raise HTTPException(
            status_code=429,
            detail="Çok fazla istek gönderildi. Biraz bekleyip tekrar dene.",
            headers={"Retry-After": str(math.ceil(seconds_to_wait))},
        )


@app.get("/api/today", response_model=TodayResponse)
def get_today():
    return TodayResponse(date=current_game_day().isoformat(), word_count=WORDS_PER_DAY)


@app.post("/api/games", response_model=StartResponse)
def start_game(body: StartRequest, request: Request):
    enforce_rate_limit(request, request.app.state.game_start_limiter)
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
        remaining_seconds=game.remaining_seconds(now),
        score=game.current_score(now),
    )


@app.post("/api/games/{game_id}/hints", response_model=HintResponse)
def reveal_hint(game_id: str, body: HintRequest, request: Request):
    game = find_game(request, game_id)
    return game.reveal_hint(body.kind, time.monotonic())


@app.post("/api/games/{game_id}/guesses", response_model=GuessResponse)
def submit_guess(game_id: str, body: GuessRequest, request: Request):
    enforce_rate_limit(request, request.app.state.guess_limiter)
    game = find_game(request, game_id)
    return game.submit_guess(body.text, request.app.state.judge, time.monotonic())


@app.post("/api/games/{game_id}/finish", response_model=FinishResponse)
def finish_game(game_id: str, request: Request):
    game = find_game(request, game_id)
    game.finish(time.monotonic(), won=False)
    return game.result()


app.mount("/", StaticFiles(directory=FRONTEND_DIRECTORY, html=True), name="frontend")
