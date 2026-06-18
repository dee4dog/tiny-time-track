"""SQLAlchemy engine, session factory and declarative Base.

SQLite is used as a single local file. Two things worth knowing:

* ``check_same_thread=False`` is required because FastAPI may touch the
  connection from different worker threads.
* SQLite does NOT enforce foreign keys unless explicitly told to, so we
  switch ``PRAGMA foreign_keys=ON`` on every new connection. Without this,
  the FK constraints in the schema would be silently ignored.
"""
from __future__ import annotations

from collections.abc import Iterator

from sqlalchemy import create_engine, event
from sqlalchemy.engine import Engine
from sqlalchemy.orm import DeclarativeBase, Session, sessionmaker

from app.config import config

engine = create_engine(
    config.database_url,
    connect_args={"check_same_thread": False},
    echo=False,  # set True to log every SQL statement while debugging
)

SessionLocal = sessionmaker(
    bind=engine,
    autoflush=False,
    expire_on_commit=False,
)


class Base(DeclarativeBase):
    """Base class for all ORM models."""


@event.listens_for(Engine, "connect")
def _enable_sqlite_foreign_keys(dbapi_connection, _connection_record) -> None:
    """Turn on FK enforcement for each SQLite connection."""
    cursor = dbapi_connection.cursor()
    cursor.execute("PRAGMA foreign_keys=ON")
    cursor.close()


def get_db() -> Iterator[Session]:
    """FastAPI dependency that yields a database session per request."""
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()


def create_all_tables() -> None:
    """Create any missing tables. Safe to call repeatedly (idempotent)."""
    # Import models so they register on Base.metadata before create_all.
    from app import models  # noqa: F401  (import side effect)

    Base.metadata.create_all(bind=engine)
