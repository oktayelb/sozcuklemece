from typing import Annotated, Literal

from pydantic import BaseModel, Field, StringConstraints

from backend.game import MAX_GUESS_LENGTH

MAX_SHORT_TEXT_LENGTH = 60
MAX_LONG_TEXT_LENGTH = 500

HintKind = Literal["parts", "sentence", "category"]
ShortText = Annotated[str, StringConstraints(strip_whitespace=True, min_length=1, max_length=MAX_SHORT_TEXT_LENGTH)]
LongText = Annotated[str, StringConstraints(strip_whitespace=True, min_length=1, max_length=MAX_LONG_TEXT_LENGTH)]


class TodayResponse(BaseModel):
    date: str
    word_available: bool


class WordUpload(BaseModel):
    word: ShortText
    definition: LongText
    parts: LongText
    sentence: LongText
    category: ShortText
    field: ShortText
    also_accepted: list[LongText] = []
    wrong_meanings: list[LongText] = []
    inflected_stems: list[ShortText] = []


class WordStatusResponse(BaseModel):
    word: str | None
    expires_at: str | None


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
