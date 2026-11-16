"use strict";
// Tokens live for this tab only. Everything below talks to the same JSON
// routes the game uses (docs/FIREPLACE.md).
const store = {
  get(k) { try { return sessionStorage.getItem(k) || ""; } catch { return ""; } },
  set(k, v) { try { v ? sessionStorage.setItem(k, v) : sessionStorage.removeItem(k); } catch {} },
};
const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
};
let creating = false;
let features = { email: false, email_required: false, min_age: 16, renames: 2 };
let me = null;
let ticket = "";

/** JSON in and out; `raw` sends a Blob as it is (a picture). */
async function api(method, path, body, raw) {
  const send = () => {
    const headers = {};
    const token = store.get("access");
    if (token) headers.Authorization = "Bearer " + token;
    if (raw) headers["Content-Type"] = raw.type || "application/octet-stream";
    else if (body) headers["Content-Type"] = "application/json";
    return fetch(path, { method, headers, body: raw || (body ? JSON.stringify(body) : undefined) });
  };
  let res = await send();
  // An access token lasts an hour; trade the refresh one for another once.
  if (res.status === 401 && store.get("refresh") && path !== "/sessions/refresh") {
    const again = await fetch("/sessions/refresh", { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh: store.get("refresh") }) });
    if (again.ok) { keep(await again.json()); res = await send(); }
  }
  const data = res.status === 204 ? {} : await res.json().catch(() => ({}));
  if (!res.ok) {
    // A wrong password is a 401 too, and must not sign anybody out.
    if (res.status === 401 && !["/sessions", "/me/password", "/me/delete", "/me/email", "/me/recovery"].includes(path)) signOut();
    const err = new Error(data.error || (res.status === 413 ? "that file is too big" : "something went wrong"));
    err.data = data;
    throw err;
  }
  return data;
}

function keep(tokens) { store.set("access", tokens.access); store.set("refresh", tokens.refresh); }
function say(id, text, good) { const e = $(id); e.textContent = text; e.className = "msg " + (good ? "good" : "bad"); }
const when = (seconds) => new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
const day = (seconds) => new Date(seconds * 1000).toLocaleDateString(undefined, { dateStyle: "medium" });

// --- faces: a picture, or a placeholder made from the name -------------------

/** A stable hue from a handle, so a person keeps their colours. */
function hue(text) {
  let h = 0;
  for (const c of text.toLowerCase()) h = (h * 31 + c.codePointAt(0)) % 360;
  return h;
}

function initials(name) {
  const words = name.trim().split(/[\s_-]+/).filter(Boolean);
  const letters = words.length > 1 ? [words[0], words[words.length - 1]] : [words[0] || "?"];
  return letters.map((w) => [...w][0].toUpperCase()).join("").slice(0, 2);
}

/** Fills a `.face` with a person's picture or their placeholder. */
function face(target, p) {
  target.replaceChildren();
  target.classList.toggle("has-pic", !!p.avatar);
  if (p.avatar) {
    const img = el("img");
    img.alt = "";
    img.src = "/avatars/" + p.id + ".jpg?v=" + p.avatar;
    img.onerror = () => { p.avatar = 0; face(target, p); };
    target.append(img);
    return target;
  }
  const h = hue(p.handle);
  target.style.setProperty("--h1", h);
  target.style.setProperty("--h2", (h + 38) % 360);
  target.append(el("span", null, initials(p.display_name || p.handle)));
  return target;
}

// --- signing in and out ------------------------------------------------------

function signOut() {
  store.set("access", ""); store.set("refresh", "");
  me = null;
  $("signed-in").hidden = true; $("auth").hidden = false;
  $("banned").hidden = true; $("code-step").hidden = true; $("auth-card").hidden = false;
  setTab(false);
  show("profile");
}

function setTab(newAccount) {
  creating = newAccount;
  $("tab-in").setAttribute("aria-selected", String(!newAccount));
  $("tab-new").setAttribute("aria-selected", String(newAccount));
  $("auth-go").textContent = newAccount ? "Create account" : "Sign in";
  $("whoami").textContent = newAccount ? "Pull up a chair" : "Welcome back";
  $("password").autocomplete = newAccount ? "new-password" : "current-password";
  $("new-only").hidden = !newAccount;
  $("birthdate").required = $("accept-terms").required = newAccount;
  $("new-email").required = newAccount && features.email_required;
  $("birthdate").max = new Date().toISOString().slice(0, 10);
  say("auth-msg", "", true);
}
$("tab-in").onclick = () => setTab(false);
$("tab-new").onclick = () => setTab(true);

$("auth-form").onsubmit = async (e) => {
  e.preventDefault();
  const go = $("auth-go");
  go.disabled = true;
  try {
    const body = { handle: $("handle").value.trim(), password: $("password").value };
    if (creating) {
      // Checked here to answer at once; the Fireplace checks again and decides.
      if (ageOn($("birthdate").value) < features.min_age) {
        return say("auth-msg", "You must be at least " + features.min_age + " to make an account.");
      }
      Object.assign(body, { birthdate: $("birthdate").value, accept_terms: $("accept-terms").checked,
        policy_emails: $("policy-emails-new").checked, email: $("new-email").value.trim() });
    }
    const made = await api("POST", creating ? "/accounts" : "/sessions", body);
    $("birthdate").value = ""; $("password").value = "";
    keep(made);
    await load();
    if (made.recovery_codes) showCodes(made.recovery_codes);
  } catch (err) {
    if (err.data && err.data.banned) return showBanned(err.data.banned);
    if (err.data && err.data.code_needed) return askForCode(err.data);
    say("auth-msg", err.message, false);
  } finally { go.disabled = false; }
};

function askForCode(data) {
  ticket = data.ticket;
  $("auth-card").hidden = true; $("code-step").hidden = false;
  $("code-why").textContent = data.new_place
    ? "You are signing in from somewhere new. We emailed you a 6-digit code to make sure it is you."
    : "Two-step sign-in is on. We emailed you a 6-digit code.";
  $("code").value = "";
  say("code-msg", "", true);
  $("code").focus();
}

$("code-form").onsubmit = async (e) => {
  e.preventDefault();
  try {
    keep(await api("POST", "/sessions/code", { ticket, code: $("code").value.trim() }));
    ticket = ""; $("password").value = "";
    $("code-step").hidden = true; $("auth-card").hidden = false;
    await load();
  } catch (err) { say("code-msg", err.message); }
};
$("code-back").onclick = () => { $("code-step").hidden = true; $("auth-card").hidden = false; };

function showBanned(ban) {
  $("auth-card").hidden = true; $("banned").hidden = false;
  $("banned-until").textContent = ban.until ? "Until " + when(ban.until) + "." : "This is for good.";
  $("banned-reason").textContent = ban.reason;
  $("password").value = "";
}
$("banned-back").onclick = () => { $("banned").hidden = true; $("auth-card").hidden = false; };
$("signout").onclick = signOut;

// --- the signed-in page ------------------------------------------------------

async function load() {
  me = await api("GET", "/me");
  $("auth").hidden = true; $("signed-in").hidden = false;
  $("me-name").textContent = me.display_name || me.handle;
  $("me-handle").textContent = "@" + me.handle;
  face($("me-face"), me);
  face($("pic-face"), me);
  $("pic-remove").hidden = !me.avatar;
  $("display").value = me.display_name;
  $("renames-left").textContent = me.renames_left + " of " + features.renames + " changes left";
  $("share").checked = me.share_location;
  $("policy-emails").checked = me.policy_emails;
  $("email").value = me.email || "";
  $("email-box").hidden = !features.email;
  $("email-pill").textContent = !me.email ? "none" : me.email_verified ? "confirmed" : "not confirmed";
  $("email-pill").className = "pill " + (me.email_verified ? "ok" : "warn");
  $("email-state").textContent = !me.email ? "No email on this account yet: add one to play with it."
    : me.email_verified ? "Your email is " + me.email + "." : "Waiting for the code we sent to " + me.email + ".";
  if (me.email && !me.email_verified && $("email-code-form").hidden) showEmailCode(me.email);
  if (me.email_verified) $("email-code-form").hidden = true;
  const unconfirmed = features.email_required && !me.email_verified;
  $("confirm-banner").hidden = !unconfirmed;
  $("email-flag").hidden = !unconfirmed;
  $("confirm-to").textContent = me.email || "your address";
  $("codes-left").textContent = me.recovery_codes_left + " of 8 left";
  $("two-step-card").hidden = !features.email;
  $("two-step").checked = !!me.two_factor;
  $("admin-nav").hidden = !me.admin;
  fillPlaces();
  await Promise.all([loadFriends(), loadSessions()]);
}

function fillPlaces() {
  const rows = (me.places || []).map((p) => {
    const li = el("li");
    const ip = el("code", null, p.ip);
    const meta = el("small", null, "first " + day(p.first) + " · last " + when(p.last));
    const tag = el("span", "pill " + (p.confirmed ? "ok" : "warn"), p.confirmed ? "confirmed" : "not confirmed");
    const text = el("div"); text.append(ip, meta);
    li.append(text, tag);
    return li;
  });
  $("places").replaceChildren(...(rows.length ? rows : [el("li", "empty", "Nothing yet.")]));
}

function person(p, extra) {
  const li = el("li");
  const who = el("div", "who");
  const f = face(el("span", "face"), p);
  if (extra && "online" in extra) {
    const dot = el("span", "dot" + (extra.online ? " on" : ""));
    dot.title = extra.online ? "Online" : "Offline";
    f.append(dot);
  }
  const text = el("div");
  text.append(el("b", null, p.display_name || p.handle),
    el("small", null, "@" + p.handle + (extra && extra.where ? " · " + extra.where : "")));
  who.append(f, text);
  const buttons = el("div", "row");
  for (const [label, route, cls] of (extra && extra.buttons) || []) {
    const b = el("button", cls, label);
    b.onclick = async () => {
      b.disabled = true;
      try { await api("POST", route, { handle: p.handle }); await loadFriends(); }
      catch (err) { say("friend-msg", err.message); b.disabled = false; }
    };
    buttons.append(b);
  }
  li.append(who, buttons);
  return li;
}

async function loadFriends() {
  const f = await api("GET", "/friends");
  const fill = (id, rows, make) => {
    $(id).replaceChildren(...rows.map(make));
    if (!rows.length) $(id).append(el("li", "empty", "Nobody yet."));
  };
  const where = (p) => p.online ? (p.world ? "in " + p.world : "online") : "";
  fill("friends", f.friends, (p) => person(p, { online: p.online, where: where(p), buttons: [["Remove", "/friends/remove"]] }));
  $("incoming-box").hidden = !f.incoming.length;
  $("incoming-count").hidden = !f.incoming.length;
  $("incoming-count").textContent = f.incoming.length;
  fill("incoming", f.incoming, (p) => person(p, { buttons: [["Accept", "/friends/accept", "main"], ["Decline", "/friends/remove"]] }));
  $("outgoing-box").hidden = !f.outgoing.length;
  fill("outgoing", f.outgoing, (p) => person(p, { buttons: [["Cancel", "/friends/remove"]] }));

  // The rail: who is on first, then everybody else.
  const sorted = [...f.friends].sort((a, b) => (b.online - a.online) || (a.display_name || a.handle).localeCompare(b.display_name || b.handle));
  const online = f.friends.filter((p) => p.online).length;
  $("rail-online").textContent = f.friends.length ? online + " online" : "";
  $("rail").replaceChildren(...sorted.map((p) => {
    const li = el("li", p.online ? "on" : "");
    const fc = face(el("span", "face"), p);
    const dot = el("span", "dot" + (p.online ? " on" : "")); fc.append(dot);
    const text = el("div");
    text.append(el("b", null, p.display_name || p.handle), el("small", null, p.online ? (where(p) || "online") : "offline"));
    li.append(fc, text);
    return li;
  }));
  if (!f.friends.length) $("rail").append(el("li", "empty", "No friends yet. Add someone by their handle."));
  if (f.incoming.length) {
    const li = el("li", "ask", f.incoming.length === 1 ? "1 person wants to be your friend" : f.incoming.length + " people want to be your friend");
    li.onclick = () => show("friends");
    $("rail").prepend(li);
  }
}

$("add-form").onsubmit = async (e) => {
  e.preventDefault();
  try {
    const r = await api("POST", "/friends/add", { handle: $("add-handle").value.trim() });
    say("friend-msg", r.state === "friends" ? "You are now friends." : "Request sent.", true);
    $("add-handle").value = "";
    await loadFriends();
  } catch (err) { say("friend-msg", err.message); }
};

$("name-form").onsubmit = async (e) => {
  e.preventDefault();
  try {
    const r = await api("POST", "/me/name", { display_name: $("display").value });
    $("display").value = r.display_name;
    say("name-msg", "Saved. Servers see it the next time you join.", true);
    await load();
  } catch (err) { say("name-msg", err.message); }
};

$("share").onchange = async () => {
  try { await api("POST", "/settings", { share_location: $("share").checked }); say("name-msg", "", true); }
  catch (err) { say("name-msg", err.message); $("share").checked = !$("share").checked; }
};

$("policy-emails").onchange = async () => {
  const on = $("policy-emails").checked;
  try {
    await api("POST", "/settings", { policy_emails: on });
    say("policy-msg", on ? "On. You will hear about changes at your confirmed address." : "Off. No more of these.", true);
  } catch (err) { say("policy-msg", err.message); $("policy-emails").checked = !on; }
};

// --- your picture: made small here first, and redrawn by the server ----------

/** A square JPEG of at most 512px, so a phone photo uploads in a blink. */
async function squeeze(file) {
  const bitmap = await createImageBitmap(file);
  const side = Math.min(bitmap.width, bitmap.height);
  const size = Math.min(512, side);
  const canvas = el("canvas");
  canvas.width = canvas.height = size;
  canvas.getContext("2d").drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, size, size);
  bitmap.close();
  return new Promise((done) => canvas.toBlob(done, "image/jpeg", 0.9));
}

$("pic-file").onchange = async () => {
  const file = $("pic-file").files[0];
  $("pic-file").value = "";
  if (!file) return;
  if (file.size > 20 * 1024 * 1024) return say("pic-msg", "That file is over 20 MB.");
  say("pic-msg", "Uploading...", true);
  try {
    // A picture the browser cannot read goes as it is, and the server says why it will not do.
    const blob = await squeeze(file).catch(() => file);
    await api("POST", "/me/avatar", null, blob);
    say("pic-msg", "Saved.", true);
    await load();
  } catch (err) { say("pic-msg", err.message); }
};

$("pic-remove").onclick = async () => {
  try { await api("POST", "/me/avatar/delete"); say("pic-msg", "Removed, and deleted.", true); await load(); }
  catch (err) { say("pic-msg", err.message); }
};

// --- one dialog for every question, instead of the browser's own boxes ------

/**
 * Asks, in the page's own dialog. With `password`, asks for it and resolves
 * with what was typed; otherwise with true. Cancel resolves with null.
 * `run(password)` may throw: the error is shown and the dialog stays open.
 */
function ask({ title, text = "", go = "OK", danger = false, password = false, run }) {
  return new Promise((done) => {
    $("ask-title").textContent = title;
    $("ask-text").textContent = text;
    $("ask-pw-row").hidden = !password;
    $("ask-pw").value = "";
    $("ask-pw").required = password;
    $("ask-go").textContent = go;
    $("ask-go").className = danger ? "danger solid" : "main";
    say("ask-msg", "", true);
    $("ask-box").hidden = false;
    (password ? $("ask-pw") : $("ask-go")).focus();
    const close = (value) => {
      $("ask-box").hidden = true;
      $("ask-form").onsubmit = null; $("ask-cancel").onclick = null; document.onkeydown = null;
      done(value);
    };
    $("ask-cancel").onclick = () => close(null);
    document.onkeydown = (e) => { if (e.key === "Escape") close(null); };
    $("ask-form").onsubmit = async (e) => {
      e.preventDefault();
      const value = password ? $("ask-pw").value : true;
      if (!run) return close(value);
      $("ask-go").disabled = true;
      try { await run(value); close(value); }
      catch (err) { say("ask-msg", err.message); }
      finally { $("ask-go").disabled = false; }
    };
  });
}

// --- security: sessions, two-step, password, codes ---------------------------

async function loadSessions() {
  const sessions = await api("GET", "/me/sessions");
  $("sessions").replaceChildren(...sessions.map((s) => {
    const li = el("li");
    const text = el("div");
    const name = el("b", null, s.client);
    if (s.current) name.append(el("span", "pill ok", "this device"));
    text.append(name, el("small", null, s.ip + " · signed in " + day(s.started) + " · last used " + when(s.last)));
    li.append(text);
    if (!s.current) {
      const end = el("button", "small", "Sign out");
      end.onclick = async () => {
        end.disabled = true;
        try { await api("POST", "/me/sessions/end", { id: s.id }); say("sessions-msg", s.client + " is signed out.", true); await loadSessions(); }
        catch (err) { say("sessions-msg", err.message); end.disabled = false; }
      };
      li.append(end);
    }
    return li;
  }));
  $("end-others").hidden = sessions.length < 2;
}

$("end-others").onclick = async () => {
  const yes = await ask({ title: "Sign out everywhere else?", text: "Every other device and the game will need to sign in again.", go: "Sign them out", danger: true });
  if (!yes) return;
  try {
    const r = await api("POST", "/me/sessions/end", { others: true });
    say("sessions-msg", r.ended === 1 ? "1 other session is signed out." : r.ended + " other sessions are signed out.", true);
    await loadSessions();
  } catch (err) { say("sessions-msg", err.message); }
};

$("two-step").onchange = async () => {
  const on = $("two-step").checked;
  $("two-step").checked = !on; // only changes once the password says so
  const done = await ask({
    title: on ? "Turn on two-step sign-in" : "Turn off two-step sign-in",
    text: on ? "Every sign-in will need a code we email you." : "Only sign-ins from a new IP address will need a code.",
    go: on ? "Turn on" : "Turn off", password: true,
    run: (password) => api("POST", "/me/two-factor", { password, on }),
  });
  if (done == null) return;
  $("two-step").checked = on;
  say("two-step-msg", on ? "On. Every sign-in now needs a code from your email." : "Off. New places still need a code.", true);
  await load();
};

$("pw-form").onsubmit = async (e) => {
  e.preventDefault();
  try {
    keep(await api("POST", "/me/password", { password: $("pw-old").value, new_password: $("pw-new").value }));
    $("pw-old").value = ""; $("pw-new").value = "";
    say("pw-msg", "Changed. You were signed out everywhere else, including the game.", true);
    await loadSessions();
  } catch (err) { say("pw-msg", err.message); }
};

function showCodes(codes) {
  $("codes").replaceChildren(...codes.map((c) => el("li", null, c)));
  $("codes-box").hidden = false;
  $("codes-done").focus();
}
$("codes-done").onclick = () => { $("codes-box").hidden = true; $("codes").replaceChildren(); };

$("codes-new").onclick = async () => {
  let codes = null;
  const done = await ask({
    title: "Make new recovery codes?", text: "The codes you have now stop working.", go: "Make new codes", password: true,
    run: async (password) => { codes = (await api("POST", "/me/recovery", { password })).recovery_codes; },
  });
  if (done == null) return;
  say("codes-msg", "", true);
  showCodes(codes);
  await load();
};

// --- settings: the email, with its code ----------------------------------------

function showEmailCode(address) {
  $("email-code-form").hidden = false;
  $("email-code-note").textContent = "Enter the 6-digit code we sent to " + address + ".";
  $("email-code").value = "";
  $("email-code").focus();
}

$("email-form").onsubmit = async (e) => {
  e.preventDefault();
  const email = $("email").value.trim();
  if (me.email && email.toLowerCase() === me.email.toLowerCase() && me.email_verified) {
    return say("email-msg", "That is already your email.", true);
  }
  const done = await ask({
    title: "Change your email", text: "We will send a code to " + email + " to make sure it is yours.",
    go: "Send the code", password: true,
    run: (password) => api("POST", "/me/email", { password, email }),
  });
  if (done == null) return;
  say("email-msg", "", true);
  await load();
  showEmailCode(email);
};

async function confirmEmail(code, msg) {
  try {
    await api("POST", "/me/email/confirm", { code });
    $("email-code-form").hidden = true;
    $("email-code").value = ""; $("confirm-code").value = "";
    await load();
    say("email-msg", "Confirmed.", true);
  } catch (err) { say(msg, err.message); }
}
$("email-code-form").onsubmit = (e) => { e.preventDefault(); confirmEmail($("email-code").value.trim(), "email-msg"); };
$("confirm-form").onsubmit = (e) => { e.preventDefault(); confirmEmail($("confirm-code").value.trim(), "confirm-msg"); };

async function resend(msg) {
  try { await api("POST", "/me/email/resend"); say(msg, "A new code is on its way.", true); }
  catch (err) { say(msg, err.message); }
}
$("email-resend").onclick = () => resend("email-msg");
$("confirm-resend").onclick = () => resend("confirm-msg");

// --- your data ------------------------------------------------------------------

$("export").onclick = async () => {
  const data = await api("GET", "/me/export");
  const a = el("a");
  a.href = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: "application/json" }));
  a.download = "hearth-account-" + data.handle + ".json";
  a.click();
  URL.revokeObjectURL(a.href);
};

$("delete-account").onclick = async () => {
  const done = await ask({
    title: "Delete your account?", text: "Your account, picture and friends list are deleted for good. This cannot be undone.",
    go: "Delete for good", danger: true, password: true,
    run: (password) => api("POST", "/me/delete", { password }),
  });
  if (done == null) return;
  signOut();
  say("auth-msg", "Your account is gone.", true);
};

// --- admin -------------------------------------------------------------------

const DURATIONS = [["1 hour", 60], ["1 day", 1440], ["7 days", 10080], ["30 days", 43200], ["1 year", 525600], ["For good", 0]];
for (const select of [$("ban-time"), $("ip-time")]) {
  select.replaceChildren(...DURATIONS.map(([label, minutes]) => { const o = el("option", null, label); o.value = minutes; return o; }));
  select.value = 1440;
}
let accounts = [];
let banning = null;

async function loadAdmin() {
  try {
    [accounts] = await Promise.all([api("GET", "/admin/accounts"), loadIpBans()]);
    drawAccounts();
  } catch (err) { say("admin-msg", err.message); }
}

function drawAccounts() {
  const q = $("admin-search").value.trim().toLowerCase();
  const hit = (a) => !q || [a.handle, a.display_name, a.email || "", ...(a.places || []).map((p) => p.ip)]
    .some((t) => t.toLowerCase().includes(q));
  const shown = accounts.filter(hit);
  $("admin-count").textContent = shown.length + " of " + accounts.length;
  $("admin-accounts").replaceChildren(...shown.map(accountRow));
  if (!shown.length) $("admin-accounts").append(el("li", "empty", "Nobody matches."));
}
$("admin-search").oninput = drawAccounts;

function accountRow(a) {
  const li = el("li");
  const head = el("div", "acct");
  const who = el("div", "who");
  const text = el("div");
  const name = el("b", null, a.display_name);
  if (a.admin) name.append(el("span", "pill ok", "admin"));
  if (a.ban) name.append(el("span", "pill warn", a.ban.until ? "suspended until " + day(a.ban.until) : "suspended for good"));
  text.append(name, el("small", null, "@" + a.handle + " · " + (a.email ? a.email + (a.email_verified ? "" : " (not confirmed)") : "no email") + " · since " + day(a.created)));
  who.append(face(el("span", "face"), a), text);
  head.append(who);

  const actions = el("div", "row");
  const act = (label, cls, run) => { const b = el("button", cls, label); b.onclick = () => run(b); actions.append(b); };
  const post = async (b, route, body, done) => {
    b.disabled = true;
    try { await api("POST", route, body); say("admin-msg", done, true); await loadAdmin(); }
    catch (err) { say("admin-msg", err.message); b.disabled = false; }
  };
  if (a.id === me.id) {
    actions.append(el("span", "note", "you"));
  } else {
    if (a.ban) act("Unban", "", (b) => post(b, "/admin/unban", { handle: a.handle }, "Unbanned " + a.handle + "."));
    else if (!a.admin) act("Suspend", "danger", () => openBan(a));
    act("Sign out", "", (b) => post(b, "/admin/sign-out", { handle: a.handle }, a.handle + " is signed out everywhere."));
    if (!a.ban) act(a.admin ? "Remove admin" : "Make admin", "", (b) => post(b, "/admin/set-admin", { handle: a.handle, admin: !a.admin },
      a.admin ? a.handle + " is no longer an admin." : a.handle + " is now an admin."));
    act("Delete", "danger", (b) => {
      ask({ title: "Delete " + a.handle + "?", text: "The account, its picture and its friends are deleted for good. This cannot be undone.", go: "Delete for good", danger: true })
        .then((yes) => { if (yes) post(b, "/admin/delete", { handle: a.handle }, a.handle + " is deleted."); });
    });
  }
  head.append(actions);
  li.append(head);

  const places = el("details", "acct-places");
  places.append(el("summary", null, (a.places || []).length + " sign-in addresses"));
  const list = el("ul", "places");
  for (const p of a.places || []) {
    const row = el("li");
    const text = el("div");
    text.append(el("code", null, p.ip), el("small", null, "first " + day(p.first) + " · last " + when(p.last)));
    const block = el("button", "danger small", "Block IP");
    block.onclick = () => { $("ip-addr").value = p.ip; $("ip-reason").focus(); $("ip-form").scrollIntoView({ behavior: "smooth", block: "center" }); };
    row.append(text, el("span", "pill " + (p.confirmed ? "ok" : "warn"), p.confirmed ? "confirmed" : "not confirmed"), block);
    list.append(row);
  }
  places.append(list);
  li.append(places);
  return li;
}

function openBan(a) {
  banning = a;
  $("ban-title").textContent = "Suspend " + a.handle;
  $("ban-reason").value = "";
  say("ban-msg", "", true);
  $("ban-box").hidden = false;
  $("ban-reason").focus();
}
$("ban-cancel").onclick = () => { $("ban-box").hidden = true; };
$("ban-form").onsubmit = async (e) => {
  e.preventDefault();
  try {
    await api("POST", "/admin/ban", { handle: banning.handle, minutes: Number($("ban-time").value), reason: $("ban-reason").value });
    $("ban-box").hidden = true;
    say("admin-msg", banning.handle + " is suspended and signed out.", true);
    await loadAdmin();
  } catch (err) { say("ban-msg", err.message); }
};

async function loadIpBans() {
  const bans = await api("GET", "/admin/bans");
  $("ip-bans").replaceChildren(...bans.map((b) => {
    const li = el("li");
    const text = el("div");
    text.append(el("code", null, b.ip), el("small", null, (b.ban.until ? "until " + when(b.ban.until) : "for good") + " · " + b.ban.reason + " · by " + b.by));
    const lift = el("button", "", "Unblock");
    lift.onclick = async () => {
      lift.disabled = true;
      try { await api("POST", "/admin/unban-ip", { ip: b.ip }); say("ip-msg", b.ip + " is unblocked.", true); await loadIpBans(); }
      catch (err) { say("ip-msg", err.message); lift.disabled = false; }
    };
    li.append(text, lift);
    return li;
  }));
  if (!bans.length) $("ip-bans").append(el("li", "empty", "No address is blocked."));
}

$("ip-form").onsubmit = async (e) => {
  e.preventDefault();
  try {
    await api("POST", "/admin/ban-ip", { ip: $("ip-addr").value.trim(), minutes: Number($("ip-time").value), reason: $("ip-reason").value });
    say("ip-msg", $("ip-addr").value.trim() + " is blocked.", true);
    $("ip-addr").value = ""; $("ip-reason").value = "";
    await loadIpBans();
  } catch (err) { say("ip-msg", err.message); }
};

// --- the sidebar: one view at a time -----------------------------------------

function show(view) {
  for (const b of document.querySelectorAll(".nav button")) {
    if (b.dataset.view === view) b.setAttribute("aria-current", "page"); else b.removeAttribute("aria-current");
  }
  for (const v of document.querySelectorAll(".view")) v.hidden = v.id !== "view-" + view;
  document.body.dataset.view = view;
  if (view === "admin" && me && me.admin) loadAdmin();
  window.scrollTo(0, 0);
}
for (const b of document.querySelectorAll("[data-view]")) b.onclick = () => show(b.dataset.view);

/** Whole years from a YYYY-MM-DD date to today, in the browser's calendar. */
function ageOn(date) {
  const [y, m, d] = date.split("-").map(Number);
  const now = new Date();
  return now.getFullYear() - y - ((now.getMonth() + 1 < m || (now.getMonth() + 1 === m && now.getDate() < d)) ? 1 : 0);
}

// --- start: what this server offers, then whoever this tab was ---------------

(async () => {
  try { features = { ...features, ...(await (await fetch("/features")).json()) }; } catch {}
  $("policy-new-row").hidden = !features.email;
  $("new-email-row").hidden = !features.email_required;
  $("min-age").textContent = features.min_age;
  setTab(creating);
  if (store.get("access") || store.get("refresh")) load().catch(signOut);
})();
