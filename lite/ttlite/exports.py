"""Excel and CSV exports of a period timesheet report.

The workbook opens on a "Timesheet" sheet: projects down the side, weeks (or
months, for long periods) across the top, with live SUM() totals both ways and
print setup (landscape, fit to one page wide, header rows repeated).
"""
from __future__ import annotations

import csv
import io
from datetime import datetime

from openpyxl import Workbook
from openpyxl.styles import Alignment, Font, PatternFill
from openpyxl.utils import get_column_letter
from openpyxl.worksheet.worksheet import Worksheet

from ttlite import APP_NAME
from ttlite.services import DAY_NAMES, Report, week_label

HEAD_FILL = PatternFill("solid", fgColor="F5E2D8")
TOTAL_FILL = PatternFill("solid", fgColor="FBEEE7")
HEAD_FONT = Font(bold=True, color="3B2C25")
TITLE_FONT = Font(bold=True, size=14, color="8F4630")
BOLD = Font(bold=True)
HOURS = "0.00"
PERCENT = "0.0%"
_FORMULA_CHARS = ("=", "+", "-", "@", "\t", "\r")


def filename(report: Report, ext: str, kind: str = "") -> str:
    suffix = f"_{kind}" if kind else ""
    return f"timesheet_{report.start:%Y-%m-%d}_to_{report.end:%Y-%m-%d}{suffix}.{ext}"


def _hours(minutes: int) -> float:
    return minutes / 60  # exact for quarter hours


def _append(ws: Worksheet, values: list, *, font: Font | None = None, fill: PatternFill | None = None) -> int:
    ws.append(values)
    row = ws.max_row
    for cell in ws[row]:
        # Data that merely looks like a formula stays text (no spreadsheet injection);
        # real formulas are only ever written through _formula().
        if isinstance(cell.value, str) and cell.value.startswith("="):
            cell.data_type = "s"
        if font:
            cell.font = font
        if fill:
            cell.fill = fill
    return row


def _formula(ws: Worksheet, row: int, col: int, formula: str, *, fmt: str = HOURS,
             font: Font | None = None, fill: PatternFill | None = None) -> None:
    cell = ws.cell(row, col)
    cell.value = formula
    cell.data_type = "f"
    cell.number_format = fmt
    if font:
        cell.font = font
    if fill:
        cell.fill = fill


def _heading(ws: Worksheet, report: Report, settings: dict[str, str], title: str) -> None:
    _append(ws, [title], font=TITLE_FONT)
    who = " · ".join(v for v in (settings.get("user_name"), settings.get("company")) if v)
    if who:
        _append(ws, [who])
    _append(ws, [f"Period: {report.label}"])
    _append(ws, ["Saved timesheets only" if report.saved_only else "Saved timesheets plus weeks not yet saved"])
    _append(ws, [f"Generated {datetime.now():%d %b %Y %H:%M} by {APP_NAME}"])
    ws.append([])


def _widths(ws: Worksheet, widths: list[int]) -> None:
    for i, width in enumerate(widths, start=1):
        ws.column_dimensions[get_column_letter(i)].width = width


def _page_setup(ws: Worksheet, header_row: int, landscape: bool = True) -> None:
    ws.page_setup.orientation = "landscape" if landscape else "portrait"
    ws.page_setup.paperSize = ws.PAPERSIZE_A4
    ws.page_setup.fitToWidth = 1
    ws.page_setup.fitToHeight = 0
    ws.sheet_properties.pageSetUpPr.fitToPage = True
    ws.print_title_rows = f"{header_row}:{header_row}"
    ws.freeze_panes = ws.cell(header_row + 1, 3 if landscape else 1)


def _timesheet_sheet(ws: Worksheet, report: Report, settings: dict[str, str]) -> None:
    matrix = report.matrix
    _heading(ws, report, settings, "Timesheet")
    labels = [c.label + ("*" if c.partial else "") for c in matrix.columns]
    header = _append(ws, ["Code", "Project", *labels, "Total"], font=HEAD_FONT, fill=HEAD_FILL)
    for cell in ws[header][2:]:
        cell.alignment = Alignment(horizontal="right")
    first_col, last_col = 3, 2 + len(matrix.columns)
    total_col = last_col + 1
    first_data = header + 1
    for row in matrix.rows:
        r = _append(ws, [row.project.code, row.project.display, *[_hours(m) or None for m in row.cells]])
        for c in range(first_col, last_col + 1):
            ws.cell(r, c).number_format = HOURS
        _formula(ws, r, total_col, f"=SUM({get_column_letter(first_col)}{r}:{get_column_letter(last_col)}{r})",
                 font=BOLD)
    last_data = ws.max_row
    total_row = last_data + 1
    ws.cell(total_row, 2, "Total").font = BOLD
    for c in range(first_col, total_col + 1):
        letter = get_column_letter(c)
        formula = f"=SUM({letter}{first_data}:{letter}{last_data})" if matrix.rows else "=0"
        _formula(ws, total_row, c, formula, font=BOLD, fill=TOTAL_FILL)
    ws.cell(total_row, 1).fill = TOTAL_FILL
    ws.cell(total_row, 2).fill = TOTAL_FILL
    if matrix.has_partial:
        ws.cell(total_row + 2, 1, "* only part of this " + matrix.unit + " falls inside the period")
    _widths(ws, [12, 40, *([11] * len(matrix.columns)), 11])
    _page_setup(ws, header)


def report_xlsx(report: Report, settings: dict[str, str]) -> bytes:
    wb = Workbook()

    _timesheet_sheet(wb.active, report, settings)
    wb.active.title = "Timesheet"

    ws = wb.create_sheet("Summary")
    _heading(ws, report, settings, "Time per project")
    header = _append(ws, ["Code", "Project", "Hours", "Share"], font=HEAD_FONT, fill=HEAD_FILL)
    first = header + 1
    for p in report.projects:
        row = _append(ws, [p.code, p.display, _hours(p.minutes), p.share / 100])
        ws.cell(row, 3).number_format = HOURS
        ws.cell(row, 4).number_format = PERCENT
    last = ws.max_row
    total = last + 1
    ws.cell(total, 2, "Total").font = BOLD
    _formula(ws, total, 3, f"=SUM(C{first}:C{last})" if report.projects else "=0", font=BOLD)
    _formula(ws, total, 4, f"=SUM(D{first}:D{last})" if report.projects else "=0", fmt=PERCENT, font=BOLD)
    _widths(ws, [12, 46, 11, 9])
    _page_setup(ws, header, landscape=False)

    ws = wb.create_sheet("By week")
    _heading(ws, report, settings, "Hours by week")
    header = _append(ws, ["Week starting", "Week", "Status", "Hours"], font=HEAD_FONT, fill=HEAD_FILL)
    first = header + 1
    for w in report.weeks:
        status = "Saved" if w.saved else ("Not saved" if w.minutes else "")
        label = week_label(w.start) + (" (part)" if w.partial else "")
        row = _append(ws, [w.start, label, status, _hours(w.minutes)])
        ws.cell(row, 1).number_format = "yyyy-mm-dd"
        ws.cell(row, 4).number_format = HOURS
    last = ws.max_row
    ws.cell(last + 1, 2, "Total").font = BOLD
    _formula(ws, last + 1, 4, f"=SUM(D{first}:D{last})", font=BOLD)
    _widths(ws, [14, 30, 11, 10])
    _page_setup(ws, header, landscape=False)

    ws = wb.create_sheet("Daily detail")
    _heading(ws, report, settings, "Daily detail")
    header = _append(ws, ["Date", "Day", "Code", "Project (as recorded)", "Hours", "Status"],
                     font=HEAD_FONT, fill=HEAD_FILL)
    first = header + 1
    for line in report.lines:
        row = _append(ws, [line.day, DAY_NAMES[line.day.weekday()], line.code, line.name,
                           _hours(line.minutes), "Saved" if line.saved else "Not saved"])
        ws.cell(row, 1).number_format = "yyyy-mm-dd"
        ws.cell(row, 5).number_format = HOURS
    last = ws.max_row
    ws.cell(last + 1, 4, "Total").font = BOLD
    _formula(ws, last + 1, 5, f"=SUM(E{first}:E{last})" if report.lines else "=0", font=BOLD)
    _widths(ws, [12, 6, 12, 40, 10, 11])
    _page_setup(ws, header, landscape=False)

    wb.calculation.fullCalcOnLoad = True  # totals are computed as soon as the file opens
    buffer = io.BytesIO()
    wb.save(buffer)
    return buffer.getvalue()


# --------------------------------------------------------------------------- #
#  CSV
# --------------------------------------------------------------------------- #
def _safe(text: str) -> str:
    """Neutralise values a spreadsheet would treat as a formula."""
    return "'" + text if text.startswith(_FORMULA_CHARS) else text


def _csv(rows: list[list[str]]) -> bytes:
    buffer = io.StringIO()
    csv.writer(buffer).writerows(rows)
    return buffer.getvalue().encode("utf-8-sig")  # BOM so Excel reads accents correctly


def report_csv(report: Report) -> bytes:
    """Daily detail: one row per day and project."""
    rows = [["Date", "Day", "Project code", "Project", "Hours", "Status"]]
    for line in report.lines:
        rows.append([line.day.isoformat(), DAY_NAMES[line.day.weekday()], _safe(line.code), _safe(line.name),
                     f"{line.minutes / 60:.2f}", "Saved" if line.saved else "Not saved"])
    return _csv(rows)


def report_totals_csv(report: Report) -> bytes:
    """The totalled timesheet grid: projects x weeks/months with totals both ways."""
    matrix = report.matrix
    rows = [["Project code", "Project",
             *[f"{c.start.isoformat()} to {c.end.isoformat()}" for c in matrix.columns], "Total"]]
    for row in matrix.rows:
        rows.append([_safe(row.project.code), _safe(row.project.display),
                     *[f"{m / 60:.2f}" for m in row.cells], f"{row.total / 60:.2f}"])
    rows.append(["", "Total", *[f"{m / 60:.2f}" for m in matrix.column_totals], f"{matrix.total / 60:.2f}"])
    return _csv(rows)
