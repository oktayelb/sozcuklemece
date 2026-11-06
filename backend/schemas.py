from typing import Literal

from pydantic import BaseModel, Field

from backend.game import MAX_GUESS_LENGTH

HintKind = Literal["parts", "sentence", "category"]


class TodayResponse(BaseModel):
    date: str
    word_count: int


class StartRequest(BaseModel):
    slot: int = Field(default=0, ge=0)


class HintOption(BaseModel):
    kind: HintKind
    label: str


class StartResponse(BaseModel):
    game_id: str
    word: str
    hints: list[HintOption]
    duration_seconds: int
    hint_penalty: int
    remaining_seconds: float
    score: int


class HintRequest(BaseModel):
    kind: HintKind


class HintResponse(BaseModel):
    kind: HintKind
    text: str | None
    finished: bool
    remaining_seconds: float
    score: int


class GuessRequest(BaseModel):
    text: str = Field(min_length=1, max_length=MAX_GUESS_LENGTH)


class GuessResponse(BaseModel):
    closeness: int | None
    message: str
    rejected: bool
    won: bool
    finished: bool
    remaining_seconds: float
    score: int


class FinishResponse(BaseModel):
    word: str
    definition: str
    parts: str
    sentence: str
    category: str
    won: bool
    score: int
    seconds_used: int
    hints_used: int
    guess_count: int
    best_closeness: int
