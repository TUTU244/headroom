from __future__ import annotations

import asyncio
import json

import pytest

from headroom.cache.compression_store import (
    get_compression_store,
    reset_compression_store,
)
from headroom.ccr import mcp_server


def test_shared_stats_work_without_fcntl(monkeypatch, tmp_path) -> None:
    monkeypatch.setattr(mcp_server, "_HAS_FCNTL", False)
    monkeypatch.setattr(mcp_server, "fcntl", None)
    monkeypatch.setattr(mcp_server, "SHARED_STATS_DIR", tmp_path)
    monkeypatch.setattr(mcp_server, "SHARED_STATS_FILE", tmp_path / "session_stats.jsonl")
    monkeypatch.setattr(mcp_server.os, "getpid", lambda: 4242)
    monkeypatch.setattr(mcp_server.time, "time", lambda: 1001.0)

    event = {"type": "compress", "timestamp": 1000.0}
    mcp_server._append_shared_event(event)

    raw_lines = mcp_server.SHARED_STATS_FILE.read_text(encoding="utf-8").splitlines()
    assert len(raw_lines) == 1
    assert json.loads(raw_lines[0]) == {"type": "compress", "timestamp": 1000.0, "pid": 4242}

    events = mcp_server._read_shared_events(window_seconds=60)
    assert events == [{"type": "compress", "timestamp": 1000.0, "pid": 4242}]


# --- Shared compression store wiring ---------------------------------------
# MCP's _get_local_store() must return the get_compression_store() singleton —
# the same instance the proxy and response_handler use — so content compressed
# on either side is retrievable in-process. These pin that wiring so a private
# store can't creep back.


@pytest.fixture
def fresh_store():
    reset_compression_store()
    yield
    reset_compression_store()


def test_mcp_uses_shared_singleton_store(fresh_store) -> None:
    """MCP's store is the global singleton, not a private instance."""
    pytest.importorskip("mcp", reason="MCP SDK required")
    server = mcp_server.HeadroomMCPServer(check_proxy=False)
    assert server._get_local_store() is get_compression_store()


def test_mcp_retrieves_proxy_stored_content(fresh_store) -> None:
    """Content stored via the singleton (as the proxy does) is retrievable
    through MCP's local-store path. The HTTP fallback is disabled so this
    passes only via the shared store."""
    pytest.importorskip("mcp", reason="MCP SDK required")
    original = '{"some": "original proxy-compressed content"}'
    hash_key = get_compression_store().store(original, '{"compressed": true}')

    server = mcp_server.HeadroomMCPServer(check_proxy=False)
    result = asyncio.run(server._retrieve_content(hash_key, query=None))

    assert result.get("source") == "local"
    assert result["original_content"] == original


def test_retrieve_with_query_no_match_returns_content_not_error(fresh_store) -> None:
    """headroom_retrieve with a query that matches nothing must NOT return
    'Content not found' when the hash is valid.  The entry exists; only the
    BM25 search came up empty.  Callers should receive the full original
    content plus a human-readable note — not a misleading error.  Regression
    test for issue #1213."""
    pytest.importorskip("mcp", reason="MCP SDK required")
    # Store highly repetitive content: BM25 scores will be near-zero for any
    # query term because IDF collapses on identical tokens.
    original = json.dumps([{"id": i, "val": "AAPL"} for i in range(30)])
    hash_key = get_compression_store().store(original, "[compressed]")

    server = mcp_server.HeadroomMCPServer(check_proxy=False)
    result = asyncio.run(server._retrieve_content(hash_key, query="id_that_never_matches_xyzzy"))

    # Must NOT be an error response
    assert "error" not in result, f"Got error for valid hash with query: {result}"
    # Must surface the full content
    assert result.get("source") == "local"
    assert "original_content" in result
    assert result["original_content"] == original
    # Must signal zero query matches, not a retrieval failure
    assert result.get("count") == 0
    assert "note" in result
