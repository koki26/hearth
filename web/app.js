"use strict";
// Tokens live for this tab only. Everything below talks to the same JSON
// routes the game uses (docs/FIREPLACE.md).
const store = {
  get(k) { try { return sessionStorage.getItem(k) || ""; } catch { return ""; } },
  set(k, v) { try { v ? sessionStorage.setItem(k, v) : sessionStorage.removeItem(k); } catch {} },
};
const $ = (id) => document.getElementById(id);
let creating = false;

async function api(method, path, body) {
  const headers = { "Content-Type": "application/json" };
  const token = store.get("access");
  if (token) headers.Authorization = "Bearer " + token;
  let res = await fetch(path, { method, headers, body: body ? JSON.stringify(body) : undefined });
  // An access token lasts an hour; trade the refresh one for another once.
  if (res.status === 401 && store.get("refresh") && path !== "/sessions/refresh") {
    const again = await fetch("/sessions/refresh", { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh: store.get("refresh") }) });
    if (again.ok) {
      keep(await again.json());
      headers.Authorization = "Bearer " + store.get("access");
      res = await fetch(path, { method, headers, body: body ? JSON.stringify(body) : undefined });
    }
  }
  const data = res.status === 204 ? {} : await res.json().catch(() => ({}));
  if (!res.ok) {
    // A wrong password is a 401 too, and must not sign anybody out.
    if (res.status === 401 && !["/sessions", "/me/password", "/me/delete", "/me/email", "/me/recovery"].includes(path)) signOut();
    throw new Error(data.error || "something went wrong");
  }
  return data;
}

function keep(tokens) { store.set("access", tokens.access); store.set("refresh", tokens.refresh); }
function say(id, text, good) { const el = $(id); el.textContent = text; el.className = "msg " + (good ? "good" : "bad"); }

function signOut() {
  store.set("access", ""); store.set("refresh", "");
  $("signed-in").hidden = true; $("auth").hidden = false;
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
  $("new-terms").hidden = !newAccount;
  say("auth-msg", "", true);
}
$("tab-in").onclick = () => setTab(false);
$("tab-new").onclick = () => setTab(true);

$("auth-form").onsubmit = async (e) => {
  e.preventDefault();
  try {
    const made = await api("POST", creating ? "/accounts" : "/sessions",
      { handle: $("handle").value.trim(), password: $("password").value });
    keep(made);
    $("password").value = "";
    await load();
    if (made.recovery_codes) showCodes(made.recovery_codes);
  } catch (err) { say("auth-msg", err.message); }
};

$("signout").onclick = signOut;

async function load() {
  const me = await api("GET", "/me");
  $("auth").hidden = true; $("signed-in").hidden = false;
  $("me-name").textContent = me.display_name || me.handle;
  $("me-handle").textContent = "@" + me.handle;
  $("avatar").textContent = initial(me.display_name || me.handle);
  $("display").value = me.display_name;
  $("share").checked = me.share_location;
  $("email").value = me.email || "";
  $("email-state").textContent = !me.email ? "No email on this account."
    : me.email_verified ? "Confirmed." : "Waiting for you to open the link we sent.";
  $("codes-left").textContent = me.recovery_codes_left + " of 8 left";
  await loadFriends();
}

function person(p, extra) {
  const li = document.createElement("li");
  const who = document.createElement("div"); who.className = "who";
  const face = document.createElement("span"); face.className = "avatar"; face.textContent = initial(p.display_name || p.handle);
  if (extra && "online" in extra) {
    const dot = document.createElement("span"); dot.className = "dot" + (extra.online ? " on" : "");
    dot.title = extra.online ? "Online" : "Offline";
    face.append(dot);
  }
  const text = document.createElement("div");
  const name = document.createElement("b");
  name.append(p.display_name || p.handle);
  const sub = document.createElement("small");
  sub.textContent = "@" + p.handle + (extra && extra.where ? " · " + extra.where : "");
  text.append(name, sub);
  who.append(face, text);
  const buttons = document.createElement("div"); buttons.className = "row";
  for (const [label, route, cls] of (extra && extra.buttons) || []) {
    const b = document.createElement("button"); b.textContent = label; if (cls) b.className = cls;
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
    const ul = $(id); ul.replaceChildren(...rows.map(make));
    if (!rows.length) { const li = document.createElement("li"); li.className = "empty"; li.textContent = "Nobody yet."; ul.append(li); }
  };
  fill("friends", f.friends, (p) => person(p, {
    online: p.online,
    where: p.online ? (p.world ? "in " + p.world : "online") : "",
    buttons: [["Remove", "/friends/remove"]],
  }));
  $("incoming-box").hidden = !f.incoming.length;
  $("incoming-count").hidden = !f.incoming.length;
  $("incoming-count").textContent = f.incoming.length;
  fill("incoming", f.incoming, (p) => person(p, { buttons: [["Accept", "/friends/accept", "main"], ["Decline", "/friends/remove"]] }));
  $("outgoing-box").hidden = !f.outgoing.length;
  fill("outgoing", f.outgoing, (p) => person(p, { buttons: [["Cancel", "/friends/remove"]] }));
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
  } catch (err) { say("name-msg", err.message); }
};

$("share").onchange = async () => {
  try { await api("POST", "/settings", { share_location: $("share").checked }); say("name-msg", "", true); }
  catch (err) { say("name-msg", err.message); $("share").checked = !$("share").checked; }
};

$("pw-form").onsubmit = async (e) => {
  e.preventDefault();
  try {
    keep(await api("POST", "/me/password", { password: $("pw-old").value, new_password: $("pw-new").value }));
    $("pw-old").value = ""; $("pw-new").value = "";
    say("pw-msg", "Changed. You were signed out everywhere else, including the game.", true);
  } catch (err) { say("pw-msg", err.message); }
};

$("del-form").onsubmit = async (e) => {
  e.preventDefault();
  if (!confirm("Delete your Hearth account for good? This cannot be undone.")) return;
  try {
    await api("POST", "/me/delete", { password: $("del-pw").value });
    signOut();
    say("auth-msg", "Your account is gone.", true);
  } catch (err) { say("del-msg", err.message); }
};

if (store.get("access") || store.get("refresh")) load().catch(signOut);

// --- what this server offers, recovery codes, email, export (D63) ------------

fetch("/features").then((r) => r.json()).then((f) => {
  $("email-nav").hidden = !f.email;
  $("min-age").textContent = f.min_age;
}).catch(() => {});

function showCodes(codes) {
  $("codes").replaceChildren(...codes.map((c) => { const li = document.createElement("li"); li.textContent = c; return li; }));
  $("codes-box").hidden = false;
  $("codes-done").focus();
}
$("codes-done").onclick = () => { $("codes-box").hidden = true; $("codes").replaceChildren(); };

$("codes-form").onsubmit = async (e) => {
  e.preventDefault();
  try {
    const r = await api("POST", "/me/recovery", { password: $("codes-pw").value });
    $("codes-pw").value = "";
    say("codes-msg", "The old codes no longer work.", true);
    showCodes(r.recovery_codes);
    await load();
  } catch (err) { say("codes-msg", err.message); }
};

async function saveEmail(address) {
  try {
    await api("POST", "/me/email", { password: $("email-pw").value, email: address });
    $("email-pw").value = "";
    say("email-msg", address ? "Saved. Open the link we sent to confirm it." : "Removed, and deleted.", true);
    await load();
  } catch (err) { say("email-msg", err.message); }
}
$("email-form").onsubmit = (e) => { e.preventDefault(); saveEmail($("email").value.trim()); };
$("email-remove").onclick = () => { if ($("email-pw").reportValidity()) saveEmail(""); };

$("export").onclick = async () => {
  const data = await api("GET", "/me/export");
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: "application/json" }));
  a.download = "hearth-account-" + data.handle + ".json";
  a.click();
  URL.revokeObjectURL(a.href);
};

// --- the sidebar: one view at a time -----------------------------------------

function initial(name) { return [...name.trim()][0]?.toUpperCase() || "?"; }

function show(view) {
  for (const b of document.querySelectorAll(".nav button")) {
    if (b.dataset.view === view) b.setAttribute("aria-current", "page"); else b.removeAttribute("aria-current");
  }
  for (const v of document.querySelectorAll(".view")) v.hidden = v.id !== "view-" + view;
  window.scrollTo(0, 0);
}
for (const b of document.querySelectorAll(".nav button")) b.onclick = () => show(b.dataset.view);
