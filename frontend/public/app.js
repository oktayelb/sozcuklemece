const LOW_TIME_SECONDS = 30;
const TICK_MILLISECONDS = 200;
const PENCIL_SOUND_FILES = ["sounds/pencil-1.mp3", "sounds/pencil-2.mp3", "sounds/pencil-3.mp3"];
const PENCIL_SOUND_VOLUME = 0.7;
const PLAYER_ID_STORAGE_KEY = "sozcuklemece-oyuncu";
const MAX_GUESS_LENGTH = 200;
const SHOW_LENGTH_FROM = 150;

const game = {
  id: null,
  durationSeconds: 0,
  hintPenalty: 0,
  maxTimePenalty: 0,
  scoreAtSync: 0,
  remainingAtSync: 0,
  guessIntervalSeconds: 0,
  nextGuessAt: 0,
  lastCloseness: null,
  judgedCount: 0,
  appealMinCloseness: 0,
  appealsLeft: 0,
  deadline: 0,
  timerId: null,
  isStarting: false,
  isEnding: false,
  isSubmitting: false,
};

let turnstileSiteKey = null;
let turnstileAction = null;
let todaysResult = null;

const playerId = loadPlayerId();

const pencilSounds = PENCIL_SOUND_FILES.map(loadSound);
let nextPencilSound = 0;

function element(id) {
  return document.getElementById(id);
}

function newPlayerId() {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function loadPlayerId() {
  try {
    const savedId = localStorage.getItem(PLAYER_ID_STORAGE_KEY);
    if (savedId) {
      return savedId;
    }
  } catch (error) {
    console.debug("Oyuncu kimliği okunamadı:", error);
  }
  const id = newPlayerId();
  try {
    localStorage.setItem(PLAYER_ID_STORAGE_KEY, id);
  } catch (error) {
    console.debug("Oyuncu kimliği kaydedilemedi:", error);
  }
  return id;
}

function loadSound(file) {
  const sound = new Audio(file);
  sound.preload = "auto";
  sound.volume = PENCIL_SOUND_VOLUME;
  return sound;
}

async function playPencilSound() {
  const sound = pencilSounds[nextPencilSound];
  nextPencilSound = (nextPencilSound + 1) % pencilSounds.length;
  sound.currentTime = 0;
  try {
    await sound.play();
  } catch (error) {
    console.debug("Kalem sesi çalınamadı:", error);
  }
}

function showScreen(screenId) {
  for (const screen of document.querySelectorAll(".screen")) {
    screen.hidden = screen.id !== screenId;
  }
  document.body.classList.toggle("landing-page", screenId === "landing-screen");
  window.scrollTo(0, 0);
}

function showError(message) {
  const banner = element("error-banner");
  banner.textContent = message;
  banner.hidden = false;
}

function clearError() {
  element("error-banner").hidden = true;
}

class AlreadyPlayedError extends Error {}

class RefusedInputError extends Error {}

class RateLimitError extends Error {
  constructor(retryAfterSeconds) {
    super("İstek sınırı aşıldı.");
    this.retryAfterSeconds = retryAfterSeconds;
  }
}

async function callApi(path, body) {
  const options = { cache: "no-store" };
  if (body !== undefined) {
    options.method = "POST";
    options.headers = { "Content-Type": "application/json" };
    options.body = JSON.stringify(body);
  }
  const response = await fetch(path, options);
  if (response.status === 429) {
    throw new RateLimitError(Number(response.headers.get("Retry-After")));
  }
  if (response.status === 403) {
    throw new HumanCheckError("İnsan doğrulaması geçilemedi.");
  }
  if (response.status === 409) {
    throw new AlreadyPlayedError("Bugünün sözcüğü zaten oynandı.");
  }
  if (response.status === 422) {
    const body = await response.json().catch(() => ({}));
    throw new RefusedInputError(body.detail || "Gönderilen metin kabul edilmedi.");
  }
  if (!response.ok) {
    throw new Error(`İstek başarısız oldu: ${response.status}`);
  }
  return response.json();
}

class HumanCheckError extends Error {}

function requestHumanCheckToken() {
  return new Promise((resolve, reject) => {
    if (typeof turnstile === "undefined" || turnstileSiteKey === null) {
      reject(new HumanCheckError("Turnstile yüklenemedi."));
      return;
    }
    let widgetId = null;
    const finish = (settle) => {
      if (widgetId !== null) {
        turnstile.remove(widgetId);
      }
      settle();
    };
    widgetId = turnstile.render("#human-check", {
      sitekey: turnstileSiteKey,
      action: turnstileAction,
      appearance: "interaction-only",
      language: "tr",
      callback: (token) => finish(() => resolve(token)),
      "error-callback": () => finish(() => reject(new HumanCheckError("Turnstile doğrulaması başarısız."))),
      "timeout-callback": () => finish(() => reject(new HumanCheckError("Turnstile doğrulaması zaman aşımına uğradı."))),
    });
  });
}

function requestFailureMessage(error, fallbackMessage) {
  if (error instanceof HumanCheckError) {
    return "İnsan doğrulaması yapılamadı. Sayfayı yenileyip tekrar dene.";
  }
  if (error instanceof RateLimitError) {
    return `Çok fazla istek gönderildi. ${error.retryAfterSeconds} saniye sonra tekrar dene.`;
  }
  if (error instanceof RefusedInputError) {
    return error.message;
  }
  return fallbackMessage;
}

function formatDate(isoDate) {
  const date = new Date(`${isoDate}T12:00:00`);
  return date.toLocaleDateString("tr-TR", { day: "numeric", month: "long", year: "numeric" });
}

function formatClock(totalSeconds) {
  const roundedSeconds = Math.ceil(totalSeconds);
  const minutes = Math.floor(roundedSeconds / 60);
  const seconds = String(roundedSeconds % 60).padStart(2, "0");
  return `${minutes}:${seconds}`;
}

async function fetchPlayerToday() {
  try {
    return await callApi(`/api/players/${playerId}/today`);
  } catch (error) {
    return { state: "new", result: null };
  }
}

function showPlayButton(playerToday) {
  const playButton = element("play-button");
  todaysResult = playerToday.result;
  if (playerToday.state === "finished") {
    playButton.textContent = "Sonucunu gör";
  } else if (playerToday.state === "playing") {
    playButton.textContent = "Devam et";
  } else {
    playButton.textContent = "Oyna";
  }
  element("played-note").hidden = playerToday.state !== "finished";
  playButton.hidden = false;
}

async function loadLandingPage() {
  clearError();
  showScreen("landing-screen");
  element("play-button").hidden = true;
  element("played-note").hidden = true;
  try {
    const today = await callApi("/api/today");
    element("today-date").textContent = formatDate(today.date);
    turnstileSiteKey = today.turnstile_site_key;
    turnstileAction = today.turnstile_action;
    if (today.word_available) {
      showPlayButton(await fetchPlayerToday());
    } else {
      showError("Bugünün sözcüğü henüz hazır değil. Biraz sonra tekrar dene.");
    }
  } catch (error) {
    showError("Sunucuya ulaşılamadı. Sayfayı yenileyip tekrar dene.");
  }
}

async function showTodaysResult() {
  const playerToday = await fetchPlayerToday();
  if (playerToday.state === "finished") {
    renderResult(playerToday.result);
  } else {
    showError("Sonucun alınamadı. Sayfayı yenileyip tekrar dene.");
  }
}

function onPlayButton() {
  if (todaysResult !== null) {
    renderResult(todaysResult);
  } else {
    startGame();
  }
}

function secondsRemaining() {
  return Math.max(0, (game.deadline - performance.now()) / 1000);
}

function syncWithServer(response) {
  game.deadline = performance.now() + response.remaining_seconds * 1000;
  game.scoreAtSync = response.score;
  game.remainingAtSync = response.remaining_seconds;
}

function liveScore() {
  const secondsSinceSync = game.remainingAtSync - secondsRemaining();
  const timePenalty = (game.maxTimePenalty * secondsSinceSync) / game.durationSeconds;
  return Math.round(game.scoreAtSync - timePenalty);
}

function renderClock() {
  const remaining = secondsRemaining();

  element("timer-text").textContent = formatClock(remaining);
  element("live-score").textContent = liveScore();
  element("time-bar-fill").style.width = `${(remaining / game.durationSeconds) * 100}%`;
  element("game-screen").classList.toggle("low-time", remaining <= LOW_TIME_SECONDS);
}

function secondsUntilNextGuess() {
  return Math.max(0, (game.nextGuessAt - performance.now()) / 1000);
}

function startGuessWait() {
  game.nextGuessAt = performance.now() + game.guessIntervalSeconds * 1000;
  renderGuessButton();
}

function renderGuessButton() {
  const button = element("guess-button");
  const waitSeconds = Math.ceil(secondsUntilNextGuess());
  button.disabled = game.isSubmitting || waitSeconds > 0;
  button.textContent = waitSeconds > 0 ? `Yaz (${waitSeconds})` : "Yaz";
}

function tick() {
  renderClock();
  renderGuessButton();
  if (secondsRemaining() <= 0) {
    endGame();
  }
}

function buildHintSlot(hint, revealedText) {
  const slot = document.createElement("div");
  slot.className = "hint-slot";

  const button = document.createElement("button");
  button.type = "button";
  button.className = "hint-button";

  const label = document.createElement("span");
  label.textContent = hint.label;
  const cost = document.createElement("span");
  cost.className = "hint-cost";
  cost.textContent = `−${game.hintPenalty}`;

  button.append(label, cost);
  button.addEventListener("click", () => openHint(hint, slot));
  slot.append(button);
  if (revealedText !== undefined) {
    renderHintText(hint, slot, revealedText);
  }
  return slot;
}

function renderHintText(hint, slot, text) {
  const line = document.createElement("p");
  line.className = `entry-line hint-${hint.kind}`;

  const label = document.createElement("span");
  label.className = "hint-line-label";
  label.textContent = hint.label;

  line.append(label, document.createTextNode(text));
  slot.replaceChildren(line);
  slot.classList.add("hint-slot-open");
}

async function openHint(hint, slot) {
  try {
    const response = await callApi(`/api/games/${game.id}/hints`, { kind: hint.kind });
    if (response.finished) {
      endGame();
      return;
    }
    syncWithServer(response);
    renderClock();
    renderHintText(hint, slot, response.text);
  } catch (error) {
    element("guess-feedback").textContent = "İpucu alınamadı, tekrar dene.";
  }
}

function changeText(change) {
  if (change === null || change === 0) {
    return "";
  }
  if (change > 0) {
    return `▲${change}`;
  }
  return `▼${-change}`;
}

function feedbackText(message, change, note) {
  let text = message;
  if (changeText(change) !== "") {
    text = `${text} ${changeText(change)}`;
  }
  if (note) {
    text = `${text}. ${note}`;
  }
  return text;
}

function closenessChange(closeness) {
  let change = null;
  if (game.lastCloseness !== null) {
    change = closeness - game.lastCloseness;
  }
  game.lastCloseness = closeness;
  return change;
}

function canAppeal(closeness) {
  return closeness >= game.appealMinCloseness && closeness < 100;
}

function removeAppealButtons() {
  for (const button of document.querySelectorAll(".appeal-button")) {
    button.remove();
  }
}

async function appealGuess(guessIndex, button) {
  button.disabled = true;
  try {
    await callApi(`/api/games/${game.id}/appeals`, { guess_index: guessIndex });
    game.appealsLeft -= 1;
    const sent = document.createElement("span");
    sent.className = "appeal-sent";
    sent.textContent = "İtirazın iletildi";
    button.replaceWith(sent);
    if (game.appealsLeft <= 0) {
      removeAppealButtons();
    }
  } catch (error) {
    button.disabled = false;
    element("guess-feedback").textContent = requestFailureMessage(error, "İtiraz gönderilemedi.");
  }
}

function buildAppealPart(guessIndex, closeness, appealed) {
  if (appealed) {
    const sent = document.createElement("span");
    sent.className = "appeal-sent";
    sent.textContent = "İtirazın iletildi";
    return sent;
  }
  if (!canAppeal(closeness) || game.appealsLeft <= 0) {
    return null;
  }
  const button = document.createElement("button");
  button.type = "button";
  button.className = "appeal-button";
  button.textContent = "Bence doğru";
  button.addEventListener("click", () => appealGuess(guessIndex, button));
  return button;
}

function addGuessToList(text, closeness, label, change, note, appealed) {
  const guessIndex = game.judgedCount;
  game.judgedCount += 1;
  const item = document.createElement("li");
  item.className = "guess-item";

  const guessText = document.createElement("span");
  guessText.className = "guess-text";
  guessText.textContent = text;

  const meter = document.createElement("span");
  meter.className = "meter";
  const meterFill = document.createElement("span");
  meterFill.className = "meter-fill";
  meterFill.style.width = `${closeness}%`;
  meter.append(meterFill);

  const verdict = document.createElement("span");
  verdict.className = "guess-verdict";
  verdict.textContent = `${label} · %${closeness}`;
  if (changeText(change) !== "") {
    const changeTag = document.createElement("span");
    changeTag.className = change > 0 ? "guess-change guess-warmer" : "guess-change guess-colder";
    changeTag.textContent = changeText(change);
    verdict.append(" ", changeTag);
  }

  item.append(guessText, meter, verdict);
  if (note) {
    const noteLine = document.createElement("span");
    noteLine.className = "guess-note";
    noteLine.textContent = note;
    item.append(noteLine);
  }
  const appealPart = buildAppealPart(guessIndex, closeness, appealed);
  if (appealPart !== null) {
    item.append(appealPart);
  }
  element("guess-list").prepend(item);
}

function renderGuessLength() {
  const length = element("guess-input").value.length;
  const counter = element("guess-length");
  counter.textContent = `${length}/${MAX_GUESS_LENGTH}`;
  counter.hidden = length < SHOW_LENGTH_FROM;
}

async function submitGuess(event) {
  event.preventDefault();
  const input = element("guess-input");
  const text = input.value.trim();
  if (text === "" || game.isSubmitting || secondsUntilNextGuess() > 0) {
    return;
  }

  game.isSubmitting = true;
  renderGuessButton();
  try {
    const response = await callApi(`/api/games/${game.id}/guesses`, { text });
    if (response.won) {
      playPencilSound();
    }
    if (response.finished) {
      endGame();
      return;
    }
    syncWithServer(response);
    renderClock();
    if (response.rejected) {
      element("guess-feedback").textContent = response.message;
    } else {
      const change = closenessChange(response.closeness);
      element("guess-feedback").textContent = feedbackText(response.message, change, response.note);
      addGuessToList(text, response.closeness, response.message, change, response.note, false);
      input.value = "";
      renderGuessLength();
    }
  } catch (error) {
    element("guess-feedback").textContent = requestFailureMessage(error, "Tahmin gönderilemedi, tekrar dene.");
  } finally {
    game.isSubmitting = false;
    startGuessWait();
    input.focus();
  }
}

async function startGame() {
  if (game.isStarting) {
    return;
  }
  game.isStarting = true;
  clearError();
  try {
    const turnstileToken = await requestHumanCheckToken();
    const started = await callApi("/api/games", {
      turnstile_token: turnstileToken,
      player_id: playerId,
    });

    game.id = started.game_id;
    game.durationSeconds = started.duration_seconds;
    game.hintPenalty = started.hint_penalty;
    game.maxTimePenalty = started.max_time_penalty;
    game.guessIntervalSeconds = started.guess_interval_seconds;
    game.nextGuessAt = 0;
    game.isEnding = false;
    syncWithServer(started);

    element("game-word").textContent = started.word;
    game.appealMinCloseness = started.appeal_min_closeness;
    game.appealsLeft = started.max_appeals - started.guesses.filter((guess) => guess.appealed).length;
    element("guess-list").replaceChildren();
    game.lastCloseness = null;
    game.judgedCount = 0;
    for (const guess of started.guesses) {
      const change = closenessChange(guess.closeness);
      addGuessToList(guess.text, guess.closeness, guess.label, change, guess.note, guess.appealed);
    }
    element("guess-feedback").textContent = "";
    element("guess-input").value = "";
    renderGuessLength();
    const revealedTexts = new Map(started.revealed_hints.map((hint) => [hint.kind, hint.text]));
    const hintSlots = started.hints.map((hint) => buildHintSlot(hint, revealedTexts.get(hint.kind)));
    element("hint-list").replaceChildren(...hintSlots);

    showScreen("game-screen");
    element("guess-input").focus();
    renderClock();
    renderGuessButton();
    game.timerId = setInterval(tick, TICK_MILLISECONDS);
  } catch (error) {
    if (error instanceof AlreadyPlayedError) {
      await showTodaysResult();
    } else {
      showError(requestFailureMessage(error, "Oyun başlatılamadı. Biraz sonra tekrar dene."));
    }
  } finally {
    game.isStarting = false;
  }
}

async function endGame() {
  if (game.isEnding) {
    return;
  }
  game.isEnding = true;
  clearInterval(game.timerId);
  try {
    const result = await callApi(`/api/games/${game.id}/finish`, {});
    renderResult(result);
  } catch (error) {
    await loadLandingPage();
    showError("Oyunun sonucu alınamadı.");
  }
}

function renderResult(result) {
  const stamp = element("result-stamp");
  stamp.textContent = result.won ? "Buldun" : "Süre doldu";
  stamp.classList.toggle("stamp-lost", !result.won);

  element("result-word").textContent = result.word;
  element("result-category").textContent = result.category;
  element("result-parts").textContent = result.parts;
  element("result-definition").textContent = result.definition;
  element("result-sentence").textContent = result.sentence;

  element("result-score").textContent = result.score;
  element("result-time").textContent = formatClock(result.seconds_used);
  element("result-hints").textContent = result.hints_used;
  element("result-guesses").textContent = result.guess_count;
  element("result-closeness").textContent = `%${result.best_closeness}`;
  renderCrowd(result.stats, result.score);

  clearError();
  showScreen("result-screen");
}

function rankText(stats) {
  if (stats.better_than_percent === null) {
    return "Bugün oynayan ilk kişi sensin.";
  }
  return `Diğer oyuncuların yüzde ${stats.better_than_percent} kadarından yüksek puan aldın.`;
}

function bucketLabel(bucket) {
  if (bucket.max_score === 0) {
    return "Bulamadı";
  }
  return `${bucket.min_score}–${bucket.max_score}`;
}

function buildBucketRow(bucket, mostPlayers, isMine) {
  const row = document.createElement("li");
  row.className = isMine ? "crowd-bucket crowd-bucket-mine" : "crowd-bucket";

  const label = document.createElement("span");
  label.className = "crowd-bucket-label";
  label.textContent = bucketLabel(bucket);

  const bar = document.createElement("span");
  bar.className = "crowd-bar";
  const barFill = document.createElement("span");
  barFill.className = "crowd-bar-fill";
  barFill.style.width = `${(bucket.players / mostPlayers) * 100}%`;
  bar.append(barFill);

  const count = document.createElement("span");
  count.className = "crowd-bucket-count";
  count.textContent = isMine ? `${bucket.players} · sen` : `${bucket.players}`;

  row.append(label, bar, count);
  return row;
}

function renderCrowd(stats, myScore) {
  const crowd = element("crowd");
  if (!stats) {
    crowd.hidden = true;
    return;
  }
  element("crowd-rank").textContent = rankText(stats);
  element("crowd-players").textContent = stats.players;
  element("crowd-win").textContent = `%${stats.win_percent}`;
  element("crowd-score").textContent = stats.average_score;
  element("crowd-guesses").textContent = stats.average_guesses.toLocaleString("tr-TR");
  if (stats.average_winning_seconds === null) {
    element("crowd-time").textContent = "–";
  } else {
    element("crowd-time").textContent = formatClock(stats.average_winning_seconds);
  }

  const mostPlayers = Math.max(1, ...stats.score_buckets.map((bucket) => bucket.players));
  const rows = stats.score_buckets.map((bucket) => {
    const isMine = myScore >= bucket.min_score && myScore <= bucket.max_score;
    return buildBucketRow(bucket, mostPlayers, isMine);
  });
  element("crowd-buckets").replaceChildren(...rows);
  crowd.hidden = false;
}

element("play-button").addEventListener("click", onPlayButton);
element("guess-form").addEventListener("submit", submitGuess);
element("guess-input").addEventListener("input", renderGuessLength);
element("home-button").addEventListener("click", loadLandingPage);

loadLandingPage();
