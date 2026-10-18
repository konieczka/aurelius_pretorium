// Saved logins, the page's half (logins.rs has the rest). Runs on a tab's top frame in a script world of its own: it
// shares the page's DOM, but the page can't see this script, or the message handler it talks to the app with.
// When a password field shows up it asks for the site's logins and fills in the latest saved; when a login form is
// sent it says what was typed, and the tab's toolbar asks whether to save it.
(() => {
  if (window.__pretorium) return;
  const ask = (m) => window.webkit.messageHandlers.logins.postMessage(JSON.stringify(m)).then((r) => JSON.parse(r ?? "null"));

  const shown = (el) => el.isConnected && !el.disabled && el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden";
  const passwords = (scope = document) => [...scope.querySelectorAll("input[type=password]")].filter(shown);
  const isNew = (pw) => /new-password/.test(pw.autocomplete);
  const marked = (i) => /username|email/.test(i.autocomplete) || i.type === "email" || /user|email|login|account|ident/i.test(i.name + " " + i.id);

  /** The username field that goes with password field `pw`: the last one before it that says it's one (in its form, if
   * it has one), else the last text field before it. */
  function userFor(pw) {
    const fields = [...(pw.form ?? document).querySelectorAll("input")].filter((i) => /^(text|email|tel)$/.test(i.type) && shown(i));
    const before = fields.filter((i) => i.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING);
    return before.findLast(marked) ?? before.at(-1) ?? null;
  }

  // The element's own setter, past any the page put on it (React's value tracker, which would swallow the change):
  // then the events read as typing.
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  function set(el, value) {
    setValue.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  }

  /** The login forms on the page: a password field (and its username field) where it's the only one in its form, so not
   * a sign-up or a password change. */
  const forms = () =>
    passwords()
      .filter((pw) => !isNew(pw) && passwords(pw.form ?? document).length === 1)
      .map((pw) => ({ pw, user: userFor(pw) }));

  /** Fill `login` in: into every login form when asked to (the toolbar), else into empty ones, once each. */
  function fill(login, force = true) {
    for (const { pw, user } of forms()) {
      if (!force && (filled.has(pw) || pw.value || (user && user.value && user.value !== login.user))) continue;
      if (user) set(user, login.user);
      set(pw, login.password);
      filled.add(pw);
    }
  }
  const filled = new WeakSet();

  let logins = null; // the site's, once asked for
  async function look() {
    if (!forms().length) return;
    logins ??= ask({ t: "logins" }).then((l) => l ?? [], () => []);
    const saved = await logins;
    // Someone already typed a user: theirs, if it's saved
    for (const { pw, user } of forms()) {
      const login = (user?.value && saved.find((l) => l.user === user.value)) || (!user?.value && saved[0]);
      if (login && !pw.value && !filled.has(pw)) fill(login, false);
    }
  }
  let timer = 0;
  new MutationObserver(() => {
    clearTimeout(timer);
    timer = setTimeout(look, 150);
  }).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["type", "style", "class", "hidden"] });
  look();

  // A login was sent: a form submitted, a button outside any form clicked (single-page apps' forms often aren't), or
  // Enter pressed in a field. Signing up or changing a password saves the new one.
  let last = "";
  function sent(scope) {
    const pws = passwords(scope ?? document).filter((p) => p.value);
    if (!pws.length) return;
    const pw = pws.find(isNew) ?? (pws.length === 3 ? pws[1] : pws[0]); // a change form: current, new, again
    const user = (userFor(pw)?.value ?? "").trim();
    if (user + "\n" + pw.value === last) return;
    last = user + "\n" + pw.value;
    ask({ t: "sent", user, password: pw.value }).catch(() => {});
  }
  addEventListener("submit", (e) => e.target instanceof HTMLFormElement && sent(e.target), true);
  addEventListener("click", (e) => {
    const b = e.target instanceof Element && e.target.closest("button, input[type=submit], input[type=button], [role=button]");
    if (b && !b.form && !b.closest("form")) sent(null); // in a form, its submit event says (and a show-password toggle doesn't)
  }, true);
  addEventListener("keydown", (e) => e.key === "Enter" && e.target instanceof HTMLInputElement && sent(e.target.form), true);

  window.__pretorium = { fill: (login) => fill(login) };
})();
