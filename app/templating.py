"""Shared Jinja2 template environment.

Kept in its own module so every router imports the *same* configured
``templates`` instance (with our global helpers registered once).
"""
from __future__ import annotations

from pathlib import Path

from fastapi.templating import Jinja2Templates

TEMPLATES_DIR = Path(__file__).parent / "templates"

templates = Jinja2Templates(directory=str(TEMPLATES_DIR))


def format_money(amount, symbol: str = "R") -> str:
    """Format a number as ``R 1 234 567`` (space thousands separators).

    Negative values are shown in parentheses, e.g. ``(R 1 234)``.
    """
    if amount is None:
        amount = 0
    negative = amount < 0
    whole = int(abs(round(amount)))
    grouped = f"{whole:,}".replace(",", " ")
    text = f"{symbol} {grouped}"
    return f"({text})" if negative else text


def format_hours(value) -> str:
    """Format an hours value for a grid input: blank for zero, else trimmed.

    Examples: 0 -> "", 8.0 -> "8", 0.5 -> "0.5", 7.5 -> "7.5".
    """
    if value is None:
        return ""
    number = float(value)
    if number == 0:
        return ""
    return f"{number:g}"


def format_pct(fraction) -> str:
    """Format a fraction (0.25) as a percent string (``25%``). Blank if None."""
    if fraction is None:
        return "—"
    return f"{float(fraction) * 100:.0f}%"


def sparkline(values, *, width: int = 80, height: int = 22) -> str:
    """Return a tiny inline SVG line for a list of numbers (no JS needed).

    Used for the last-4-weeks burn column on the projects table. Renders
    offline and scales the series to fit the little box.
    """
    nums = [float(v) for v in values] if values else []
    if not nums or max(nums) == min(nums):
        # Flat line in the middle when there's no variation.
        y = height / 2
        return (
            f'<svg class="spark" width="{width}" height="{height}" viewBox="0 0 {width} {height}">'
            f'<line x1="0" y1="{y:.1f}" x2="{width}" y2="{y:.1f}" '
            f'stroke="currentColor" stroke-width="1.5" fill="none"/></svg>'
        )
    lo, hi = min(nums), max(nums)
    span = hi - lo
    n = len(nums)
    pad = 2
    pts = []
    for i, v in enumerate(nums):
        x = pad + (width - 2 * pad) * (i / (n - 1)) if n > 1 else width / 2
        y = height - pad - (height - 2 * pad) * ((v - lo) / span)
        pts.append(f"{x:.1f},{y:.1f}")
    points = " ".join(pts)
    return (
        f'<svg class="spark" width="{width}" height="{height}" viewBox="0 0 {width} {height}">'
        f'<polyline points="{points}" stroke="currentColor" stroke-width="1.5" '
        f'fill="none" stroke-linecap="round" stroke-linejoin="round"/></svg>'
    )


# Make the helpers available inside all templates.
templates.env.globals["money"] = format_money
templates.env.globals["hrs"] = format_hours
templates.env.globals["pct"] = format_pct
templates.env.globals["sparkline"] = sparkline
