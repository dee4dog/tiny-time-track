"""Start Tiny Time Track Lite and open it in the browser.

    python -m ttlite [--port 8100] [--data-dir PATH] [--no-browser]

If the app is already running it just opens the browser (single instance).
"""
from __future__ import annotations

import argparse
import json
import socket
import sys
import threading
import urllib.request
import webbrowser

from ttlite import APP_NAME, __version__
from ttlite.config import HOST, default_data_dir, default_port


def _already_running(url: str) -> bool:
    try:
        with urllib.request.urlopen(url + "healthz", timeout=1.5) as resp:  # noqa: S310 - fixed local URL
            return json.load(resp).get("app") == APP_NAME
    except (OSError, ValueError):
        return False


def _port_free(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        try:
            sock.bind((HOST, port))
        except OSError:
            return False
    return True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m ttlite", description=f"{APP_NAME} {__version__}")
    parser.add_argument("--port", type=int, default=default_port(), help="local port (default 8100)")
    parser.add_argument("--data-dir", default=None, help="folder for the database and backups")
    parser.add_argument("--no-browser", action="store_true", help="don't open a browser window")
    args = parser.parse_args(argv)
    url = f"http://{HOST}:{args.port}/"

    if _already_running(url):
        print(f"{APP_NAME} is already running - opening {url}")
        if not args.no_browser:
            webbrowser.open(url)
        return 0
    if not _port_free(args.port):
        print(f"Port {args.port} is used by another program. Start with a different port, "
              f"e.g.  python -m ttlite --port {args.port + 1}", file=sys.stderr)
        return 1

    import uvicorn

    from ttlite.web import create_app

    app = create_app(args.data_dir or default_data_dir())
    print(f"{APP_NAME} {__version__} is running at {url}")
    print(f"Your data is stored in: {app.state.data_dir}")
    print("Keep this window open while you work; close it to stop the app.")
    if not args.no_browser:
        threading.Timer(1.0, webbrowser.open, args=(url,)).start()
    uvicorn.run(app, host=HOST, port=args.port, log_level="warning")
    return 0


if __name__ == "__main__":
    sys.exit(main())
