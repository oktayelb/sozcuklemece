const MAX_DAYS_AHEAD = 90;
const TEXT_FIELDS = ["word", "definition", "parts", "sentence", "category", "field"];
const LIST_FIELDS = [
  "also_accepted",
  "opposite_meanings",
  "wrong_meanings",
  "inflected_stems",
  "right_test_guesses",
  "wrong_test_guesses",
];

function element(id) {
  return document.getElementById(id);
}

function linesOf(text) {
  const lines = [];
  for (const line of text.split("\n")) {
    if (line.trim() !== "") {
      lines.push(line.trim());
    }
  }
  return lines;
}

function readWordForm() {
  const fields = element("word-form").elements;
  const word = {};
  for (const name of TEXT_FIELDS) {
    word[name] = fields[name].value.trim();
  }
  for (const name of LIST_FIELDS) {
    word[name] = linesOf(fields[name].value);
  }
  return word;
}

async function failureMessage(response) {
  if (response.status === 429) {
    return `Çok fazla deneme yapıldı. ${response.headers.get("Retry-After")} saniye sonra tekrar dene.`;
  }
  const body = await response.json().catch(() => ({}));
  if (body.detail) {
    return body.detail;
  }
  return `İstek başarısız oldu: ${response.status}`;
}

async function callAdminApi(method, path, body) {
  const options = {
    method,
    cache: "no-store",
    headers: { "X-Admin-Token": element("admin-token").value },
  };
  if (body !== undefined) {
    options.headers["Content-Type"] = "application/json";
    options.body = JSON.stringify(body);
  }

  let response;
  try {
    response = await fetch(path, options);
  } catch (error) {
    throw new Error("Sunucuya ulaşılamadı.");
  }
  if (!response.ok) {
    throw new Error(await failureMessage(response));
  }
  return response.json();
}

function formatMoment(isoMoment) {
  const moment = new Date(isoMoment);
  return moment.toLocaleString("tr-TR", { dateStyle: "long", timeStyle: "short", timeZone: "Europe/Istanbul" });
}

function addDays(isoDate, days) {
  const date = new Date(`${isoDate}T00:00:00Z`);
  date.setUTCDate(date.getUTCDate() + days);
  return date.toISOString().slice(0, 10);
}

function formatDay(isoDate) {
  const date = new Date(`${isoDate}T12:00:00Z`);
  return date.toLocaleDateString("tr-TR", { day: "numeric", month: "long", weekday: "long", timeZone: "UTC" });
}

function describeStatus(status) {
  if (status.word === null) {
    return "Bugün için sözcük yok, oyun kapalı.";
  }
  return `Bugün yayında: ${status.word}. Bitiş: ${formatMoment(status.expires_at)}.`;
}

function firstFreeDay(status) {
  const takenDays = status.scheduled.map((item) => item.date);
  let day = status.today;
  while (takenDays.includes(day)) {
    day = addDays(day, 1);
  }
  return day;
}

function describeSchedule(status) {
  if (status.scheduled.length === 0) {
    return "Takvimde sözcük yok.";
  }
  const lastDay = status.scheduled[status.scheduled.length - 1].date;
  const freeDay = firstFreeDay(status);
  if (freeDay > lastDay) {
    return `Takvim ${formatDay(lastDay)} gününe kadar dolu.`;
  }
  return `İlk boş gün: ${formatDay(freeDay)}. O gün oyun kapalı kalır.`;
}

function fillWordForm(puzzle) {
  const fields = element("word-form").elements;
  for (const name of TEXT_FIELDS) {
    fields[name].value = puzzle[name];
  }
  for (const name of LIST_FIELDS) {
    fields[name].value = puzzle[name].join("\n");
  }
}

async function editScheduledWord(date) {
  const message = element("schedule-message");
  try {
    const puzzle = await callAdminApi("GET", `/api/admin/word/${date}`);
    fillWordForm(puzzle);
    element("word-form").elements.date.value = date;
    element("check-report").hidden = true;
    element("form-message").textContent = `${formatDay(date)} günündeki ${puzzle.word} düzenleniyor. Değişiklikler yeniden yayınlayınca kaydedilir.`;
    element("word-form").scrollIntoView({ behavior: "smooth" });
  } catch (error) {
    message.textContent = error.message;
  }
}

async function deleteScheduledWord(item) {
  if (!window.confirm(`${formatDay(item.date)} günündeki ${item.word} silinsin mi?`)) {
    return;
  }
  try {
    renderStatus(await callAdminApi("DELETE", `/api/admin/word/${item.date}`));
  } catch (error) {
    element("schedule-message").textContent = error.message;
  }
}

function buildScheduleButton(label, action) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "check-move";
  button.textContent = label;
  button.addEventListener("click", action);
  return button;
}

function buildScheduleRow(item, today) {
  const row = document.createElement("li");
  row.className = "schedule-row";

  const day = document.createElement("span");
  day.className = "schedule-day";
  day.textContent = item.date === today ? `Bugün, ${formatDay(item.date)}` : formatDay(item.date);

  const word = document.createElement("span");
  word.className = "check-text";
  word.textContent = item.word;

  const actions = document.createElement("span");
  actions.className = "schedule-actions";
  actions.append(
    buildScheduleButton("Düzenle", () => editScheduledWord(item.date)),
    buildScheduleButton("Sil", () => deleteScheduledWord(item)),
  );

  row.append(day, word, actions);
  return row;
}

function renderStatus(status) {
  element("word-status").textContent = describeStatus(status);
  element("schedule-message").textContent = describeSchedule(status);
  const rows = status.scheduled.map((item) => buildScheduleRow(item, status.today));
  element("schedule-list").replaceChildren(...rows);

  const dateInput = element("word-form").elements.date;
  dateInput.min = status.today;
  dateInput.max = addDays(status.today, MAX_DAYS_AHEAD);
  if (dateInput.value === "" || dateInput.value < status.today) {
    dateInput.value = firstFreeDay(status);
  }
}

async function refreshStatus() {
  try {
    renderStatus(await callAdminApi("GET", "/api/admin/word"));
  } catch (error) {
    element("word-status").textContent = error.message;
  }
}

async function publishWord(event) {
  event.preventDefault();
  const button = element("publish-button");
  const message = element("form-message");
  const form = element("word-form");
  const word = readWordForm();
  word.date = form.elements.date.value;
  button.disabled = true;
  message.textContent = "";
  try {
    const status = await callAdminApi("POST", "/api/admin/word", word);
    form.reset();
    element("check-report").hidden = true;
    renderStatus(status);
    if (word.date === status.today) {
      message.textContent = `${word.word} yayınlandı, şimdi oyunda.`;
    } else {
      message.textContent = `${word.word}, ${formatDay(word.date)} için takvime eklendi.`;
    }
  } catch (error) {
    message.textContent = error.message;
  } finally {
    button.disabled = false;
  }
}

function fieldLines(name) {
  return linesOf(element("word-form").elements[name].value);
}

function setFieldLines(name, lines) {
  element("word-form").elements[name].value = lines.join("\n");
}

function addLines(name, newLines) {
  const lines = fieldLines(name);
  for (const line of newLines) {
    if (!lines.includes(line)) {
      lines.push(line);
    }
  }
  setFieldLines(name, lines);
}

function moveLine(text, fromName, toName) {
  setFieldLines(fromName, fieldLines(fromName).filter((line) => line !== text));
  addLines(toName, [text]);
}

async function runWithButton(buttonId, work) {
  const button = element(buttonId);
  const message = element("check-message");
  button.disabled = true;
  message.textContent = "";
  try {
    await work(message);
  } catch (error) {
    message.textContent = error.message;
  } finally {
    button.disabled = false;
  }
}

function countWins(results, expected) {
  const matching = results.filter((result) => result.expected === expected);
  const wins = matching.filter((result) => result.won).length;
  return `${wins}/${matching.length}`;
}

function summaryText(report) {
  const results = report.test_guesses;
  let text = `Kazanması gerekenlerden ${countWins(results, "right")} kazandı. Kazanmaması gerekenlerden ${countWins(results, "wrong")} kazandı.`;
  if (report.threshold_raised) {
    const threshold = report.win_similarity.toLocaleString("tr-TR", { maximumFractionDigits: 3 });
    text += ` Bir yanlış deneme tahmini eşiği geçtiği için bu sözcüğün kazanma eşiği ${threshold} oldu, doğru tahminler de zorlaşır. O tahmini yanlış anlamlara taşımak daha iyi.`;
  }
  return text;
}

function buildMoveButton(result) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "check-move";
  if (result.expected === "right") {
    button.textContent = "Doğru anlatımlara taşı";
    button.addEventListener("click", () => {
      moveLine(result.text, "right_test_guesses", "also_accepted");
      button.replaceWith("Taşındı");
    });
  } else {
    button.textContent = "Yanlış anlamlara taşı";
    button.addEventListener("click", () => {
      moveLine(result.text, "wrong_test_guesses", "wrong_meanings");
      button.replaceWith("Taşındı");
    });
  }
  return button;
}

function buildReportRow(result) {
  const row = document.createElement("li");
  row.className = result.as_expected ? "check-row" : "check-row check-row-off";

  const mark = document.createElement("span");
  mark.className = "check-mark";
  mark.textContent = result.as_expected ? "✓" : "✗";

  const text = document.createElement("span");
  text.className = "check-text";
  text.textContent = result.text;

  const verdict = document.createElement("span");
  verdict.className = "check-verdict";
  verdict.textContent = result.won ? "kazandı" : `%${result.closeness}`;
  if (result.won_by_entailment) {
    verdict.textContent += " (çıkarımla)";
  }
  if (result.note) {
    verdict.textContent += ` · ${result.note}`;
  }

  row.append(mark, text, verdict);
  if (!result.as_expected) {
    row.append(buildMoveButton(result));
  }
  return row;
}

function checkWord() {
  return runWithButton("check-button", async (message) => {
    message.textContent = "Deneniyor…";
    const report = await callAdminApi("POST", "/api/admin/word/check", readWordForm());
    const misjudged = report.test_guesses.filter((result) => !result.as_expected);
    misjudged.sort((first, second) => first.expected.localeCompare(second.expected));
    const judgedWell = report.test_guesses.filter((result) => result.as_expected);
    element("check-summary").textContent = summaryText(report);
    element("check-list").replaceChildren(...[...misjudged, ...judgedWell].map(buildReportRow));
    element("check-report").hidden = false;
    message.textContent = "";
  });
}

const REVIEW_VERDICTS = [
  ["right", "Doğru say"],
  ["opposite", "Zıt anlam"],
  ["wrong", "Yanlış anlam"],
  ["dismiss", "Geç"],
];

function reviewDetails(item) {
  const parts = [item.won_by_entailment ? "çıkarımla kazandı" : `%${item.closeness}`];
  if (item.appeals > 0) {
    parts.push(`${item.appeals} itiraz`);
  }
  if (item.seen > 1) {
    parts.push(`${item.seen} kez`);
  }
  return parts.join(" · ");
}

function buildReviewRow(item) {
  const row = document.createElement("li");
  row.className = item.appeals > 0 ? "check-row check-row-off" : "check-row";

  const mark = document.createElement("span");
  mark.className = "check-mark";
  mark.textContent = item.appeals > 0 ? "!" : "·";

  const text = document.createElement("span");
  text.className = "check-text";
  text.textContent = item.text;

  const details = document.createElement("span");
  details.className = "check-verdict";
  details.textContent = reviewDetails(item);

  const actions = document.createElement("span");
  actions.className = "review-actions";
  for (const [verdict, label] of REVIEW_VERDICTS) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "check-move";
    button.textContent = label;
    button.addEventListener("click", () => decideReview(item.text, verdict));
    actions.append(button);
  }

  row.append(mark, text, details, actions);
  return row;
}

function renderReview(review) {
  const message = element("review-message");
  if (review.word === null) {
    message.textContent = "Yayında sözcük yok.";
  } else if (review.items.length === 0) {
    message.textContent = `${review.word} için bekleyen tahmin yok.`;
  } else {
    message.textContent = `${review.word} için ${review.items.length} tahmin bekliyor.`;
  }
  element("review-list").replaceChildren(...review.items.map(buildReviewRow));
}

async function refreshReview() {
  try {
    renderReview(await callAdminApi("GET", "/api/admin/review"));
  } catch (error) {
    element("review-message").textContent = error.message;
  }
}

async function decideReview(text, verdict) {
  element("review-message").textContent = "Kaydediliyor…";
  try {
    renderReview(await callAdminApi("POST", "/api/admin/review", { text, verdict }));
  } catch (error) {
    element("review-message").textContent = error.message;
  }
}

element("admin-token").addEventListener("change", refreshStatus);
element("admin-token").addEventListener("change", refreshReview);
element("review-refresh").addEventListener("click", refreshReview);
element("check-button").addEventListener("click", checkWord);
element("word-form").addEventListener("submit", publishWord);
