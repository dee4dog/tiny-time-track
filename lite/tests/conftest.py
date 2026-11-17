from __future__ import annotations

import pytest
from fastapi.testclient import TestClient

from ttlite.web import create_app


@pytest.fixture
def app(tmp_path):
    return create_app(tmp_path)


@pytest.fixture
def client(app):
    # Host 127.0.0.1 so the local-only guard accepts the requests.
    with TestClient(app, base_url="http://127.0.0.1:8100") as test_client:
        yield test_client


@pytest.fixture
def db(app):
    session = app.state.SessionLocal()
    yield session
    session.close()
