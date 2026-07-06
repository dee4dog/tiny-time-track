// Tiny Time Track — front-end behaviour.
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
    if (hours === 8) cell.classList.add("total-ok");
    else if (hours < 8) cell.classList.add("total-under");
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

  // ---- Keyboard navigation between cells ----------------------------------
  // Arrow keys move focus across all hours inputs in DOM order (plan row, then
  // actual row, per project). Left/right step one cell; up/down jump a row.
  grid.addEventListener("keydown", (e) => {
    const target = e.target;
    if (!target.matches("input.hrs")) return;

    const inputs = Array.from(grid.querySelectorAll("input.hrs"));
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

  recomputeTotals();
})();
