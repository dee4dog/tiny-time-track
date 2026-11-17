"""SQLite engine/session setup.

One SQLite file holds everything. Per-connection pragmas switch on foreign
keys (off by default in SQLite) and a busy timeout. The classic rollback
journal (journal_mode=DELETE) is used rather than WAL: with one user the
speed difference is irrelevant, and it keeps the database a single,
self-contained file - so copying a backup over it can never be undone by a
leftover ``-wal`` file.
"""
from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

from fastapi import Request
from sqlalchemy import Engine, create_engine, event
from sqlalchemy.orm import DeclarativeBase, Session, sessionmaker


class Base(DeclarativeBase):
    """Base class for all tables."""


def init_db(db_path: Path) -> tuple[Engine, sessionmaker[Session]]:
    """Open (creating if needed) the database file and return engine + session factory."""
    engine = create_engine(
        f"sqlite:///{db_path}", connect_args={"check_same_thread": False}
    )

    @event.listens_for(engine, "connect")
    def _configure(dbapi_connection, _record) -> None:
        cursor = dbapi_connection.cursor()
        cursor.execute("PRAGMA foreign_keys=ON")
        cursor.execute("PRAGMA journal_mode=DELETE")
        cursor.execute("PRAGMA synchronous=FULL")
        cursor.execute("PRAGMA busy_timeout=5000")
        cursor.close()

    from ttlite import models  # noqa: F401  (registers the tables)

    Base.metadata.create_all(engine)
    return engine, sessionmaker(bind=engine, autoflush=False, expire_on_commit=False)


def get_db(request: Request) -> Iterator[Session]:
    """FastAPI dependency: one session per request."""
    db = request.app.state.SessionLocal()
    try:
        yield db
    finally:
        db.close()
