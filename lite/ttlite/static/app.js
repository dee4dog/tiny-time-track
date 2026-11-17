// Tiny Time Track Lite - front-end behaviour (no libraries).
//  * Week grid: autosave per cell, live totals, Excel-like keyboard moves.
//  * Week notes: debounced autosave.
//  * Forms/links marked data-flush wait for pending saves first, so pressing
//    "Save timesheet" straight after typing never loses the last entry.
"use strict";

(() => {
  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

  // ---- Toast -------------------------------------------------------------
  const toast = $("#toast");
  let toastTimer;
  function showError(message) {
    if (!toast) return;
    toast.textContent = message;
    toast.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { toast.hidden = true; }, 6000);
  }

  // ---- Pending-save tracking ----------------------------------------------
  const inflight = new Set();
  const flushers = [];          // functions that start saves for unsaved edits
  function track(promise) {
    inflight.add(promise);
    promise.finally(() => inflight.delete(promise));
    return promise;
  }
  async function flushAll() {
    flushers.forEach((fn) => fn());
    while (inflight.size) await Promise.allSettled(Array.from(inflight));
  }
  window.addEventListener("beforeunload", (e) => {
    if (inflight.size) { e.preventDefault(); e.returnValue = ""; }
  });

  async function post(url, data) {
    const res = await fetch(url, {
      method: "POST",
      body: new URLSearchParams(data),
      headers: { Accept: "application/json" },
      keepalive: true,          // finish the save even if the page is left
    });
    let body = {};
    try { body = await res.json(); } catch (_) { /* non-JSON error page */ }
    if (!res.ok) throw new Error(body.error || `Couldn't save (error ${res.status}).`);
    return body;
  }

  // ---- Forms & links: confirm, flush, then go --------------------------------
  document.addEventListener("submit", async (e) => {
    const form = e.target;
    if (form.dataset.confirm && !window.confirm(form.dataset.confirm)) { e.preventDefault(); return; }
    if (form.hasAttribute("data-flush") && (inflight.size || hasDirty())) {
      e.preventDefault();
      await flushAll();
      if ($("input.hrs.invalid")) { showError("Fix the highlighted cell first."); return; }
      HTMLFormElement.prototype.submit.call(form);
    }
  });
  document.addEventListener("click", async (e) => {
    const link = e.target.closest("a[data-flush]");
    if (!link || e.defaultPrevented || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (!inflight.size && !hasDirty()) return;
    e.preventDefault();
    const popup = link.target === "_blank" ? window.open("about:blank", "_blank") : null;
    await flushAll();
    if (popup) popup.location.href = link.href; else window.location.href = link.href;
  });
  $$("[data-autosubmit]").forEach((el) =>
    el.addEventListener("change", () => { if (el.value) el.form.requestSubmit(); })
  );

  // ---- Hours (mirrors the server's parser) ---------------------------------
  function parseHours(text) {
    let t = String(text || "").trim().toLowerCase().replace(",", ".");
    if (t.endsWith("h")) t = t.slice(0, -1).trim();
    if (!t) return 0;
    const hm = /^(\d{1,2}):([0-5]\d)$/.exec(t);
    const v = hm ? Number(hm[1]) + Number(hm[2]) / 60 : Number(t);
    return Number.isFinite(v) && v >= 0 ? Math.round(v * 4) / 4 : NaN;
  }
  const fmt = (n) => String(Math.round(n * 100) / 100);

  // ---- Week grid -------------------------------------------------------------
  const grid = $("#grid");
  const stateEl = $("#save-state");
  const cells = grid ? $$("input.hrs", grid) : [];
  const hasDirty = () => cells.some((c) => !c.disabled && c.value !== c.dataset.saved);

  function setState(text, isError) {
    if (!stateEl) return;
    stateEl.textContent = text;
    stateEl.classList.toggle("error", !!isError);
  }

  if (grid) {
    const target = parseFloat(grid.dataset.target) || 8;
    const rows = $$("tbody tr", grid).filter((tr) => $("input.hrs", tr));

    const STATUS = { ok: "on target", under: "under target", over: "over target" };
    const paint = (td, total) => {
      td.classList.remove("ok", "under", "over");
      td.removeAttribute("title");
      if (!total) return;
      const status = Math.abs(total - target) < 1e-9 ? "ok" : total < target ? "under" : "over";
      td.classList.add(status);
      td.title = `${fmt(total)} h — ${STATUS[status]} (${fmt(target)} h)`;
    };

    function recompute() {
      for (const [kind, rowClass] of [["planned", "plan-row"], ["actual", "actual-row"]]) {
        const perCol = new Map();
        let grand = 0;
        $$(`tr.${rowClass}`, grid).forEach((tr) => {
          let rowTotal = 0;
          $$("input.hrs", tr).forEach((inp) => {
            const v = parseHours(inp.value);
            const n = Number.isNaN(v) ? 0 : v;
            rowTotal += n;
            perCol.set(inp.dataset.col, (perCol.get(inp.dataset.col) || 0) + n);
          });
          const out = $("[data-row-total]", tr);
          if (out) out.textContent = fmt(rowTotal);
          grand += rowTotal;
        });
        $$(`[data-day-total="${kind}"]`, grid).forEach((td) => {
          const total = perCol.get(td.dataset.col) || 0;
          td.textContent = fmt(total);
          paint(td, total);
        });
        const g = $(`[data-grand-total="${kind}"]`, grid);
        if (g) g.textContent = fmt(grand);
      }
    }

    function markInvalid(inp, message) {
      inp.classList.add("invalid");
      inp.setAttribute("aria-invalid", "true");
      inp.title = message;
    }
    function clearInvalid(inp) {
      inp.classList.remove("invalid");
      inp.removeAttribute("aria-invalid");
      inp.removeAttribute("title");
    }

    function save(inp) {
      const value = inp.value.trim();
      if (value === inp.dataset.saved) { clearInvalid(inp); return; }
      if (Number.isNaN(parseHours(value))) {
        markInvalid(inp, "Not a number of hours — try 7.5 or 7:30.");
        setState("Not saved — check the highlighted cell", true);
        return;
      }
      setState("Saving…");
      track(
        post("/api/cell", {
          project_id: inp.dataset.project, day: inp.dataset.day, field: inp.dataset.field, value,
        }).then((data) => {
          if (inp.value.trim() === value) inp.value = data.value;   // normalise (7:30 -> 7.5)
          inp.dataset.saved = data.value;
          clearInvalid(inp);
          inp.classList.add("saved-flash");
          setTimeout(() => inp.classList.remove("saved-flash"), 500);
          if (!inflight.size || inflight.size === 1) setState("All changes saved");
        }).catch((err) => {
          markInvalid(inp, err.message);
          setState("Not saved — check the highlighted cell", true);
          showError(err.message);
        }).finally(recompute)
      );
    }

    cells.forEach((inp) => {
      inp.dataset.saved = inp.value;
      inp.addEventListener("change", () => save(inp));
      inp.addEventListener("input", recompute);
      inp.addEventListener("focus", () => inp.select());
    });
    flushers.push(() => cells.forEach((c) => { if (!c.disabled && c.value !== c.dataset.saved) save(c); }));

    // Excel-like movement. Left/right only leave the cell at the text edges.
    grid.addEventListener("keydown", (e) => {
      const inp = e.target;
      if (!inp.matches("input.hrs")) return;
      if (e.key === "Escape") {
        inp.value = inp.dataset.saved;
        clearInvalid(inp);
        recompute();
        return;
      }
      const r = rows.indexOf(inp.closest("tr"));
      let c = Number(inp.dataset.col);
      let nr = r;
      const whole = inp.selectionStart === 0 && inp.selectionEnd === inp.value.length;
      switch (e.key) {
        case "ArrowUp": nr = r - 1; break;
        case "ArrowDown": case "Enter": nr = r + 1; break;
        case "ArrowLeft": if (!whole && inp.selectionStart > 0) return; c -= 1; break;
        case "ArrowRight": if (!whole && inp.selectionEnd < inp.value.length) return; c += 1; break;
        default: return;
      }
      const next = rows[nr] && $(`input.hrs[data-col="${c}"]`, rows[nr]);
      if (next) { e.preventDefault(); next.focus(); }
      else if (e.key === "Enter") { e.preventDefault(); inp.blur(); }
    });

    recompute();
  }

  // ---- Week notes ------------------------------------------------------------
  const note = $("#week-note");
  const noteState = $("#note-state");
  if (note && !note.disabled) {
    let saved = note.value;
    let timer;
    const saveNote = () => {
      clearTimeout(timer);
      const text = note.value;
      if (text === saved) return;
      if (noteState) noteState.textContent = "Saving…";
      track(
        post("/api/week-note", { week: note.dataset.week, text })
          .then(() => { saved = text; if (noteState) noteState.textContent = "Saved"; })
          .catch((err) => { if (noteState) noteState.textContent = "Not saved"; showError(err.message); })
      );
    };
    note.addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(saveNote, 700); });
    note.addEventListener("blur", saveNote);
    flushers.push(saveNote);
  }
})();

// Open a collapsed section when linked to directly (e.g. /timesheets#revisions).
(() => {
  const target = window.location.hash && document.getElementById(window.location.hash.slice(1));
  if (target && target.tagName === "DETAILS") { target.open = true; target.scrollIntoView(); }
})();
