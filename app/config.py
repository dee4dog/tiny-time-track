"""Bootstrap configuration loaded from the environment / .env file.

These are the settings the app needs *before* it can talk to the database:
where the database file lives, the cookie-signing secret, and the network
bind address. Business settings that a manager changes at runtime (overhead
multiplier, overtime factor, SMTP credentials, reminder times) are stored in
the database instead - see ``app/settings_store.py``.
"""
from __future__ import annotations

from pathlib import Path

from pydantic_settings import BaseSettings, SettingsConfigDict


class Config(BaseSettings):
    # Read from a .env file in the working directory. Every variable is
    # prefixed with TIMETRACK_ so it can't clash with other software on the
    # server (e.g. TIMETRACK_DB_PATH).
    model_config = SettingsConfigDict(
        env_file=".env",
        env_prefix="TIMETRACK_",
        extra="ignore",
    )

    # Location of the single-file SQLite database.
    db_path: str = "timetrack.db"

    # Secret key used to sign the session cookie. MUST be overridden in
    # production with a long random value (see .env.example).
    secret_key: str = "dev-insecure-change-me-please"

    # Network bind for the web server.
    host: str = "0.0.0.0"
    port: int = 8000

    # IANA timezone for scheduling and week/date handling.
    timezone: str = "Africa/Johannesburg"

    # Destination folder for nightly database backups (used in Phase 6).
    backup_path: str = "backups"

    @property
    def database_url(self) -> str:
        """SQLAlchemy URL for the SQLite file (absolute path, forward slashes)."""
        absolute = Path(self.db_path).resolve()
        return f"sqlite:///{absolute.as_posix()}"


# A single shared instance imported across the app.
config = Config()
