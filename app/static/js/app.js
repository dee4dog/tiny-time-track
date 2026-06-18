// ES TimeTrack — front-end behaviour.
//   * Timesheet grid: Plan/Actual toggle, live coloured totals, arrow-key nav.
//   * Manager tables: click-to-sort columns.
// Persistence on the grid is handled by htmx (each input posts on change).
"use strict";

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

  // ---- Mode toggle (Plan / Actual) ----------------------------------------
  const STORAGE_KEY = "timetrack.mode";

  function setMode(mode) {
    grid.classList.remove("mode-plan", "mode-actual");
    grid.classList.add(mode === "actual" ? "mode-actual" : "mode-plan");
    document.querySelectorAll(".mode-btn").forEach((b) => {
      b.classList.toggle("active", b.dataset.mode === mode);
    });
    try { localStorage.setItem(STORAGE_KEY, mode); } catch (e) { /* ignore */ }
    recomputeTotals();
  }

  document.querySelectorAll(".mode-btn").forEach((b) => {
    b.addEventListener("click", () => setMode(b.dataset.mode));
  });

  // ---- Totals -------------------------------------------------------------
  // The "active" hours input for a cell depends on the current mode: the
  // planned input in Plan mode, the actual input in Actual mode. Overtime is
  // never counted in the day total (brief: total excludes overtime).
  function activeHoursInputs() {
    const cls = grid.classList.contains("mode-actual")
      ? "input.hrs.actual-field"
      : "input.hrs.plan-field";
    return Array.from(grid.querySelectorAll(cls));
  }

  function val(input) {
    const n = parseFloat(input.value);
    return isNaN(n) ? 0 : n;
  }

  function colourFor(cell, hours) {
    cell.classList.remove("total-ok", "total-under", "total-over");
    if (hours === 0) return;            // leave neutral when empty
    if (hours === 8) cell.classList.add("total-ok");
    else if (hours < 8) cell.classList.add("total-under");
    else cell.classList.add("total-over");
  }

  function recomputeTotals() {
    const inputs = activeHoursInputs();
    const dayTotals = [0, 0, 0, 0, 0];
    let grand = 0;

    // Per-day column totals.
    inputs.forEach((inp) => {
      const day = parseInt(inp.dataset.day, 10);
      const v = val(inp);
      if (!isNaN(day)) dayTotals[day] += v;
      grand += v;
    });
    document.querySelectorAll("[data-day-total]").forEach((cell) => {
      const d = parseInt(cell.dataset.dayTotal, 10);
      const t = dayTotals[d] || 0;
      cell.textContent = fmt(t);
      colourFor(cell, t);
    });

    // Per-project row totals.
    grid.querySelectorAll("tbody tr").forEach((tr) => {
      const cells = tr.querySelectorAll(
        grid.classList.contains("mode-actual")
          ? "input.hrs.actual-field"
          : "input.hrs.plan-field"
      );
      let rowTotal = 0;
      cells.forEach((inp) => { rowTotal += val(inp); });
      const out = tr.querySelector("[data-row-total]");
      if (out) out.textContent = fmt(rowTotal);
    });

    const grandCell = grid.querySelector("[data-grand-total]");
    if (grandCell) grandCell.textContent = fmt(grand);
  }

  function fmt(n) {
    if (!n) return "0";
    return Number(n).toString();
  }

  // Recompute whenever any hours input changes (typing or htmx-driven).
  grid.addEventListener("input", (e) => {
    if (e.target.matches("input.hrs")) recomputeTotals();
  });

  // ---- Keyboard navigation between cells ----------------------------------
  // Arrow keys move focus across the visible hours inputs (current mode).
  grid.addEventListener("keydown", (e) => {
    const target = e.target;
    if (!target.matches("input.hrs")) return;

    const inputs = activeHoursInputs();
    const idx = inputs.indexOf(target);
    if (idx === -1) return;

    const cols = 5; // Mon..Fri
    let next = -1;
    switch (e.key) {
      case "ArrowRight": next = idx + 1; break;
      case "ArrowLeft":  next = idx - 1; break;
      case "ArrowDown":  next = idx + cols; break;
      case "ArrowUp":    next = idx - cols; break;
      default: return;
    }
    if (next >= 0 && next < inputs.length) {
      e.preventDefault();
      inputs[next].focus();
      inputs[next].select();
    }
  });

  // ---- Init ---------------------------------------------------------------
  let startMode = "plan";
  try { startMode = localStorage.getItem(STORAGE_KEY) || "plan"; } catch (e) { /* ignore */ }
  setMode(startMode);
})();
