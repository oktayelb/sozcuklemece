const TEXT_FIELDS = ["word", "definition", "parts", "sentence", "category", "field"];
const LIST_FIELDS = ["also_accepted", "opposite_meanings", "wrong_meanings", "inflected_stems"];

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
  if (response.status === 401) {
    return "Parola yanlış.";
  }
  if (response.status === 422) {
    const body = await response.json().catch(() => ({}));
    return body.detail || "Eksik ya da çok uzun bir alan var.";
  }
  if (response.status === 429) {
    return `Çok fazla deneme yapıldı. ${response.headers.get("Retry-After")} saniye sonra tekrar dene.`;
  }
  if (response.status === 503) {
    return "Sunucuda ADMIN_TOKEN ayarlanmamış.";
  }
  return `İstek başarısız oldu: ${response.status}`;
}

async function callAdminApi(method, body) {
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
    response = await fetch("/api/admin/word", options);
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

function describeStatus(status) {
  if (status.word === null) {
    return "Şu an yayında sözcük yok.";
  }
  return `Yayında: ${status.word}. Silinme zamanı: ${formatMoment(status.expires_at)}.`;
}

async function refreshStatus() {
  try {
    const status = await callAdminApi("GET");
    element("word-status").textContent = describeStatus(status);
  } catch (error) {
    element("word-status").textContent = error.message;
  }
}

async function publishWord(event) {
  event.preventDefault();
  const button = element("publish-button");
  const message = element("form-message");
  button.disabled = true;
  message.textContent = "";
  try {
    const status = await callAdminApi("POST", readWordForm());
    element("word-status").textContent = describeStatus(status);
    message.textContent = `${status.word} yayınlandı.`;
  } catch (error) {
    message.textContent = error.message;
  } finally {
    button.disabled = false;
  }
}

element("admin-token").addEventListener("change", refreshStatus);
element("word-form").addEventListener("submit", publishWord);
