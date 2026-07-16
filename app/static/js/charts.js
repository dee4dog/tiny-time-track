// Project-detail charts. Reads JSON embedded by project_detail.html and draws
// with the locally-bundled Chart.js. No network access required.
"use strict";

(function () {
  if (typeof Chart === "undefined") return;

  function readJSON(id) {
    const el = document.getElementById(id);
    if (!el) return null;
    try { return JSON.parse(el.textContent); } catch (e) { return null; }
  }

  // Pull the live theme colours so charts match the CSS palette.
  const css = getComputedStyle(document.documentElement);
  const v = (name, fallback) => (css.getPropertyValue(name).trim() || fallback);
  const accent = v("--accent", "#c2674a");
  const danger = v("--danger", "#c0563e");
  const amber = v("--amber", "#cf9a4e");
  const accentFill = v("--accent-soft", "rgba(194,103,74,0.12)");
  const grid = v("--chart-grid", "rgba(120,72,52,0.08)");
  const muted = v("--muted", "");
  if (muted) Chart.defaults.color = muted;

  const moneyTicks = {
    callback: (v) => "R " + Number(v).toLocaleString("en-ZA").replace(/,/g, " "),
  };

  // ---- Burn vs fee --------------------------------------------------------
  const burn = readJSON("burn-data");
  const burnCanvas = document.getElementById("burnChart");
  if (burn && burnCanvas) {
    const feeLine = burn.labels.map(() => burn.fee);
    new Chart(burnCanvas, {
      type: "line",
      data: {
        labels: burn.labels,
        datasets: [
          {
            label: "Cumulative cost",
            data: burn.values,
            borderColor: accent,
            backgroundColor: accentFill,
            fill: true,
            tension: 0.2,
            pointRadius: 2,
          },
          {
            label: "Fee",
            data: feeLine,
            borderColor: danger,
            borderDash: [6, 4],
            pointRadius: 0,
            fill: false,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { mode: "index", intersect: false },
        scales: {
          y: { beginAtZero: true, grid: { color: grid }, ticks: moneyTicks },
          x: { grid: { display: false } },
        },
        plugins: { legend: { position: "bottom" } },
      },
    });
  }

  // ---- Planned vs actual hours -------------------------------------------
  const pva = readJSON("pva-data");
  const pvaCanvas = document.getElementById("pvaChart");
  if (pva && pvaCanvas) {
    new Chart(pvaCanvas, {
      type: "bar",
      data: {
        labels: pva.labels,
        datasets: [
          { label: "Planned", data: pva.planned, backgroundColor: accent },
          { label: "Actual", data: pva.actual, backgroundColor: amber },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        scales: {
          y: { beginAtZero: true, grid: { color: grid }, title: { display: true, text: "Hours" } },
          x: { grid: { display: false } },
        },
        plugins: { legend: { position: "bottom" } },
      },
    });
  }
})();
