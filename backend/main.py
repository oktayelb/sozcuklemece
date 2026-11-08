import asyncio
import math
import os
import secrets
import time
from contextlib import asynccontextmanager
from datetime import datetime
from pathlib import Path

from fastapi import Depends, FastAPI, HTTPException, Request
from fastapi.staticfiles import StaticFiles

from backend.daily_word import (
    GAME_TIMEZONE,
    WORD_FILE,
    DailyWordStore,
    current_game_day,
)
from backend.game import (
    GAME_SECONDS,
    HINT_LABELS,
    HINT_PENALTY,
    Game,
    remove_stale_games,
)
from backend.rate_limit import (
    ADMIN_LIMIT,
    ADMIN_WINDOW_SECONDS,
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
    StartResponse,
    TodayResponse,
    WordStatusResponse,
    WordUpload,
)
from backend.similarity import BertEncoder, MeaningJudge

FRONTEND_DIRECTORY = Path(__file__).parent.parent / "frontend"
ADMIN_TOKEN_VARIABLE = "ADMIN_TOKEN"
ADMIN_TOKEN_HEADER = "X-Admin-Token"
CLEANUP_INTERVAL_SECONDS = 60


def remove_expired_state(app):
    app.state.word_store.drop_expired_word(time.time())
    remove_stale_games(app.state.games, time.monotonic())


async def clean_up_periodically(app):
    while True:
        await asyncio.sleep(CLEANUP_INTERVAL_SECONDS)
        remove_expired_state(app)


@asynccontextmanager
async def lifespan(app):
    app.state.judge = MeaningJudge(BertEncoder())
    app.state.word_store = DailyWordStore(app.state.judge, WORD_FILE)
    app.state.word_store.load_saved_word(time.time())
    app.state.games = {}
    app.state.game_start_limiter = RateLimiter(GAME_START_LIMIT, GAME_START_WINDOW_SECONDS)
    app.state.guess_limiter = RateLimiter(GUESS_LIMIT, GUESS_WINDOW_SECONDS)
    app.state.admin_limiter = RateLimiter(ADMIN_LIMIT, ADMIN_WINDOW_SECONDS)
    cleanup_task = asyncio.create_task(clean_up_periodically(app))
    yield
    cleanup_task.cancel()


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


def require_admin(request: Request):
    enforce_rate_limit(request, request.app.state.admin_limiter)
    expected_token = os.environ.get(ADMIN_TOKEN_VARIABLE, "")
    if not expected_token:
        raise HTTPException(status_code=503, detail="Sunucuda ADMIN_TOKEN ayarlanmamış.")
    given_token = request.headers.get(ADMIN_TOKEN_HEADER, "")
    if not secrets.compare_digest(given_token.encode(), expected_token.encode()):
        raise HTTPException(status_code=401, detail="Parola yanlış.")


def word_status(store):
    word = store.current_word(time.time())
    if word is None:
        return WordStatusResponse(word=None, expires_at=None)
    expires_at = datetime.fromtimestamp(store.expires_at, GAME_TIMEZONE)
    return WordStatusResponse(word=word.puzzle["word"], expires_at=expires_at.isoformat())


@app.get("/api/today", response_model=TodayResponse)
def get_today(request: Request):
    word = request.app.state.word_store.current_word(time.time())
    return TodayResponse(date=current_game_day().isoformat(), word_available=word is not None)


@app.get("/api/admin/word", response_model=WordStatusResponse, dependencies=[Depends(require_admin)])
def get_word_status(request: Request):
    return word_status(request.app.state.word_store)


@app.post("/api/admin/word", response_model=WordStatusResponse, dependencies=[Depends(require_admin)])
def publish_word(body: WordUpload, request: Request):
    request.app.state.word_store.publish(body.model_dump(), time.time())
    return word_status(request.app.state.word_store)


@app.post("/api/games", response_model=StartResponse)
def start_game(request: Request):
    enforce_rate_limit(request, request.app.state.game_start_limiter)
    word = request.app.state.word_store.current_word(time.time())
    if word is None:
        raise HTTPException(status_code=404, detail="Bugünün sözcüğü henüz hazır değil.")

    now = time.monotonic()
    game = Game(word, started_at=now)
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
