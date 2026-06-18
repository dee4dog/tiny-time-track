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

  const accent = "#2f6fed";
  const danger = "#c0392b";
  const amber = "#d39c00";
  const grid = "rgba(0,0,0,0.06)";

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
            backgroundColor: "rgba(47,111,237,0.08)",
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
          { label: "Planned", data: pva.planned, backgroundColor: "rgba(47,111,237,0.55)" },
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
