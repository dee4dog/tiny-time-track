// Tiny Time Track — front-end behaviour.
//   * Timesheet grid: live coloured totals, keyboard nav, autosave feedback
//     (saved flash / failure toast, server-cleaned values).
//   * Manager tables: click-to-sort columns.
// Persistence on the grid is handled by htmx (each input posts on change).
"use strict";

// ---- One-shot banners: drop ?saved=/&msg=/&err= from the URL after render --
// so a refresh doesn't re-show a stale "saved" message.
(function () {
  const url = new URL(window.location.href);
  let changed = false;
  ["saved", "msg", "err"].forEach((k) => {
    if (url.searchParams.has(k)) { url.searchParams.delete(k); changed = true; }
  });
  if (changed && window.history.replaceState) {
    window.history.replaceState(null, "", url.pathname + url.search + url.hash);
  }
})();

// ---- Sortable tables (manager projects view) ------------------------------
(function () {
  document.querySelectorAll("table.sortable").forEach((table) => {
    const headers = table.querySelectorAll("thead th[data-sort]");
    headers.forEach((th, colIndex) => {
      th.classList.add("sortable-col");
      th.addEventListener("click", () => {
        const type = th.dataset.sort;
        const asc = !(th.dataset.dir === "asc");
        headers.forEach((h) => { h.removeAttribute("data-dir"); h.classList.remove("sorted"); });
        th.dataset.dir = asc ? "asc" : "desc";
        th.classList.add("sorted");

        const tbody = table.querySelector("tbody");
        const rows = Array.from(tbody.querySelectorAll("tr"));
        rows.sort((a, b) => {
          const av = cellValue(a.children[colIndex], type);
          const bv = cellValue(b.children[colIndex], type);
          if (av < bv) return asc ? -1 : 1;
          if (av > bv) return asc ? 1 : -1;
          return 0;
        });
        rows.forEach((r) => tbody.appendChild(r));
      });
    });
  });

  function cellValue(cell, type) {
    if (!cell) return type === "num" ? 0 : "";
    if (type === "num") {
      const raw = cell.dataset.sortValue !== undefined
        ? cell.dataset.sortValue
        : cell.textContent.replace(/[^0-9.\-]/g, "");
      const n = parseFloat(raw);
      return isNaN(n) ? 0 : n;
    }
    return cell.textContent.trim().toLowerCase();
  }
})();

(function () {
  const grid = document.getElementById("grid");
  if (!grid) return; // not on the timesheet page

  // Hours in a standard day; drives the under/over colouring of totals.
  const DAY_HOURS = parseFloat(grid.dataset.dayHours) || 8;

  function val(input) {
    const n = parseFloat(input.value);
    return isNaN(n) ? 0 : n;
  }

  function fmt(n) {
    if (!n) return "0";
    return Number(n).toString();
  }

  function colourFor(cell, hours) {
    cell.classList.remove("total-ok", "total-under", "total-over");
    if (hours === 0) return;            // leave neutral when empty
    if (hours === DAY_HOURS) cell.classList.add("total-ok");
    else if (hours < DAY_HOURS) cell.classList.add("total-under");
    else cell.classList.add("total-over");
  }

  // ---- Totals -------------------------------------------------------------
  // Planned and actual are both shown (two rows per project), so each gets its
  // own per-day column totals, per-row week totals, and grand total.
  function sumColumns(selector, dayTotals) {
    let grand = 0;
    grid.querySelectorAll(selector).forEach((inp) => {
      const day = parseInt(inp.dataset.day, 10);
      const v = val(inp);
      if (!isNaN(day)) dayTotals[day] += v;
      grand += v;
    });
    return grand;
  }

  function paintDayTotals(attr, dayTotals) {
    grid.querySelectorAll("[data-" + attr + "]").forEach((cell) => {
      const d = parseInt(cell.getAttribute("data-" + attr), 10);
      const t = dayTotals[d] || 0;
      cell.textContent = fmt(t);
      colourFor(cell, t);
    });
  }

  function paintRowTotals(rowSelector, inputSelector) {
    grid.querySelectorAll(rowSelector).forEach((tr) => {
      let rowTotal = 0;
      tr.querySelectorAll(inputSelector).forEach((inp) => { rowTotal += val(inp); });
      const out = tr.querySelector("[data-row-total]");
      if (out) out.textContent = fmt(rowTotal);
    });
  }

  function recomputeTotals() {
    const planDay = [0, 0, 0, 0, 0];
    const actDay = [0, 0, 0, 0, 0];
    const planGrand = sumColumns("input.hrs.plan-field", planDay);
    const actGrand = sumColumns("input.hrs.actual-field", actDay);

    paintDayTotals("day-total-plan", planDay);
    paintDayTotals("day-total-actual", actDay);
    paintRowTotals("tr.plan-row", "input.hrs.plan-field");
    paintRowTotals("tr.actual-row", "input.hrs.actual-field");

    const pg = grid.querySelector("[data-grand-total-plan]");
    if (pg) pg.textContent = fmt(planGrand);
    const ag = grid.querySelector("[data-grand-total-actual]");
    if (ag) ag.textContent = fmt(actGrand);
  }

  // Recompute whenever any hours input changes (typing or htmx-driven).
  grid.addEventListener("input", (e) => {
    if (e.target.matches("input.hrs")) recomputeTotals();
  });

  // ---- Autosave feedback ---------------------------------------------------
  // Success: the server replies with the value it actually stored (rounded to
  // 0.5, clamped to the allowed range). Reflect that back into the input so
  // the grid always shows what will be on the record, and flash the cell.
  // Failure: keep a toast up until the next successful save.
  let toast = null;

  function showSaveError(message) {
    hideSaveError();
    toast = document.createElement("div");
    toast.className = "save-toast";
    toast.setAttribute("role", "alert");
    const text = document.createElement("span");
    text.textContent = message;
    const dismiss = document.createElement("button");
    dismiss.type = "button";
    dismiss.textContent = "Dismiss";
    dismiss.addEventListener("click", hideSaveError);
    toast.append(text, dismiss);
    document.body.appendChild(toast);
  }

  function hideSaveError() {
    if (toast) { toast.remove(); toast = null; }
  }

  document.body.addEventListener("htmx:afterRequest", (e) => {
    const input = e.detail.elt;
    if (!(input instanceof HTMLInputElement) || !input.closest("#grid")) return;
    if (!e.detail.successful) return;

    hideSaveError();

    // Only sync if the user hasn't typed something newer since this request
    // was sent (rapid edits queue separate requests).
    const params = e.detail.requestConfig && e.detail.requestConfig.parameters;
    const sent = params ? String(params.value != null ? params.value : "") : null;
    const stored = e.detail.xhr ? e.detail.xhr.responseText : null;
    if (stored !== null && sent !== null && input.value === sent && input.value !== stored) {
      input.value = stored;
      recomputeTotals();
    }

    input.classList.add("saved-flash");
    setTimeout(() => input.classList.remove("saved-flash"), 900);
  });

  document.body.addEventListener("htmx:responseError", (e) => {
    const input = e.detail.elt;
    if (!(input instanceof HTMLInputElement) || !input.closest("#grid")) return;
    const status = e.detail.xhr ? e.detail.xhr.status : 0;
    showSaveError(status === 423
      ? "This week is locked — your change was NOT saved."
      : "Change not saved (error " + status + "). Please re-enter the value.");
  });

  document.body.addEventListener("htmx:sendError", (e) => {
    const input = e.detail.elt;
    if (!(input instanceof HTMLInputElement) || !input.closest("#grid")) return;
    showSaveError("Change not saved — connection problem. It will NOT retry automatically.");
  });

  // ---- Keyboard navigation between cells ----------------------------------
  // Enter moves down (Excel-style); Ctrl/Cmd+Arrows move in any direction.
  // Plain Up/Down are left alone so they step the number value natively.
  // Inputs run in DOM order: plan row, then actual row, per project (5 cols).
  grid.addEventListener("keydown", (e) => {
    const target = e.target;
    if (!target.matches("input.hrs")) return;

    const inputs = Array.from(grid.querySelectorAll("input.hrs"));
    const idx = inputs.indexOf(target);
    if (idx === -1) return;

    const cols = 5; // Mon..Fri
    let next = -1;
    if (e.key === "Enter") {
      next = idx + cols;
    } else if (e.ctrlKey || e.metaKey) {
      switch (e.key) {
        case "ArrowRight": next = idx + 1; break;
        case "ArrowLeft":  next = idx - 1; break;
        case "ArrowDown":  next = idx + cols; break;
        case "ArrowUp":    next = idx - cols; break;
        default: return;
      }
    } else {
      return;
    }
    if (next >= 0 && next < inputs.length) {
      e.preventDefault();
      inputs[next].focus();
      inputs[next].select();
    }
  });

  // Scrolling over a focused number input silently changes its value in some
  // browsers — blur instead so a scroll can never corrupt an entry.
  grid.addEventListener("wheel", (e) => {
    if (e.target.matches('input[type="number"]') && document.activeElement === e.target) {
      e.target.blur();
    }
  }, { passive: true });

  recomputeTotals();
})();
