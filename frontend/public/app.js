const LOW_TIME_SECONDS = 30;
const TICK_MILLISECONDS = 200;
const PENCIL_SOUND_FILES = ["sounds/pencil-1.mp3", "sounds/pencil-2.mp3", "sounds/pencil-3.mp3"];
const PENCIL_SOUND_VOLUME = 0.7;

const game = {
  id: null,
  durationSeconds: 0,
  hintPenalty: 0,
  guessIntervalSeconds: 0,
  nextGuessAt: 0,
  deadline: 0,
  timerId: null,
  isStarting: false,
  isEnding: false,
  isSubmitting: false,
};

const pencilSounds = PENCIL_SOUND_FILES.map(loadSound);
let nextPencilSound = 0;

function element(id) {
  return document.getElementById(id);
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
  if (!response.ok) {
    throw new Error(`İstek başarısız oldu: ${response.status}`);
  }
  return response.json();
}

function requestFailureMessage(error, fallbackMessage) {
  if (error instanceof RateLimitError) {
    return `Çok fazla istek gönderildi. ${error.retryAfterSeconds} saniye sonra tekrar dene.`;
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

async function loadLandingPage() {
  clearError();
  showScreen("landing-screen");
  element("play-button").hidden = true;
  try {
    const today = await callApi("/api/today");
    element("today-date").textContent = formatDate(today.date);
    if (today.word_available) {
      element("play-button").hidden = false;
    } else {
      showError("Bugünün sözcüğü henüz hazır değil. Biraz sonra tekrar dene.");
    }
  } catch (error) {
    showError("Sunucuya ulaşılamadı. Sayfayı yenileyip tekrar dene.");
  }
}

function secondsRemaining() {
  return Math.max(0, (game.deadline - performance.now()) / 1000);
}

function syncWithServer(response) {
  game.deadline = performance.now() + response.remaining_seconds * 1000;
}

function renderClock() {
  const remaining = secondsRemaining();

  element("timer-text").textContent = formatClock(remaining);
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

function buildHintSlot(hint) {
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

function addGuessToList(text, closeness, label) {
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

  item.append(guessText, meter, verdict);
  element("guess-list").prepend(item);
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
    element("guess-feedback").textContent = response.message;
    if (!response.rejected) {
      addGuessToList(text, response.closeness, response.message);
      input.value = "";
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
    const started = await callApi("/api/games", {});

    game.id = started.game_id;
    game.durationSeconds = started.duration_seconds;
    game.hintPenalty = started.hint_penalty;
    game.guessIntervalSeconds = started.guess_interval_seconds;
    game.nextGuessAt = 0;
    game.isEnding = false;
    syncWithServer(started);

    element("game-word").textContent = started.word;
    element("guess-list").replaceChildren();
    element("guess-feedback").textContent = "";
    element("guess-input").value = "";
    element("hint-list").replaceChildren(...started.hints.map(buildHintSlot));

    showScreen("game-screen");
    element("guess-input").focus();
    renderClock();
    renderGuessButton();
    game.timerId = setInterval(tick, TICK_MILLISECONDS);
  } catch (error) {
    showError(requestFailureMessage(error, "Oyun başlatılamadı. Biraz sonra tekrar dene."));
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

  clearError();
  showScreen("result-screen");
}

element("play-button").addEventListener("click", startGame);
element("guess-form").addEventListener("submit", submitGuess);
element("home-button").addEventListener("click", loadLandingPage);

loadLandingPage();
