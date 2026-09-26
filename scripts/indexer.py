#!/usr/bin/env python3
"""Minimal Soroban contract event indexer backed by SQLite."""

import argparse
import json
import sqlite3
import time
import urllib.error
import urllib.request
from typing import Any


def rpc_call(rpc_url: str, request_id: int, params: dict[str, Any]) -> dict[str, Any]:
    payload = json.dumps(
        {"jsonrpc": "2.0", "id": request_id, "method": "getEvents", "params": params}
    ).encode("utf-8")
    request = urllib.request.Request(
        rpc_url,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.load(response)
    if "error" in result:
        raise RuntimeError(f"Soroban RPC error: {result['error']}")
    return result["result"]


def initialize_database(database: sqlite3.Connection) -> None:
    database.executescript(
        """
        CREATE TABLE IF NOT EXISTS events (
            event_id TEXT PRIMARY KEY,
            ledger INTEGER NOT NULL,
            ledger_closed_at TEXT,
            contract_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            paging_token TEXT,
            topic_json TEXT NOT NULL,
            value_json TEXT NOT NULL,
            raw_json TEXT NOT NULL,
            indexed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS events_ledger_idx ON events(ledger);
        CREATE INDEX IF NOT EXISTS events_type_idx ON events(event_type);
        CREATE TABLE IF NOT EXISTS indexer_state (
            name TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        """
    )
    database.commit()


def get_checkpoint(database: sqlite3.Connection, start_ledger: int) -> int:
    row = database.execute(
        "SELECT value FROM indexer_state WHERE name = 'last_ledger'"
    ).fetchone()
    return max(start_ledger, int(row[0]) if row else start_ledger)


def save_events(
    database: sqlite3.Connection, events: list[dict[str, Any]], latest_ledger: int
) -> int:
    for event in events:
        database.execute(
            """
            INSERT OR IGNORE INTO events (
                event_id, ledger, ledger_closed_at, contract_id, event_type,
                paging_token, topic_json, value_json, raw_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                event["id"],
                event["ledger"],
                event.get("ledgerClosedAt"),
                event["contractId"],
                event.get("type", "contract"),
                event.get("pagingToken"),
                json.dumps(event.get("topic")),
                json.dumps(event.get("value")),
                json.dumps(event, sort_keys=True),
            ),
        )
    checkpoint = max(
        latest_ledger, max((event["ledger"] for event in events), default=latest_ledger)
    )
    database.execute(
        "INSERT OR REPLACE INTO indexer_state(name, value) VALUES ('last_ledger', ?)",
        (str(checkpoint),),
    )
    database.commit()
    return len(events)


def index_once(
    database: sqlite3.Connection,
    rpc_url: str,
    contract_id: str,
    start_ledger: int,
    limit: int,
    request_id: int,
) -> tuple[int, int]:
    checkpoint = get_checkpoint(database, start_ledger)
    all_events: list[dict[str, Any]] = []
    cursor: str | None = None
    latest_ledger = checkpoint
    while True:
        pagination: dict[str, Any] = {"limit": limit}
        if cursor is not None:
            pagination["cursor"] = cursor
        result = rpc_call(
            rpc_url,
            request_id,
            {
                "startLedger": checkpoint,
                "filters": [{"type": "contract", "contractIds": [contract_id]}],
                "pagination": pagination,
            },
        )
        request_id += 1
        events = result.get("events", [])
        all_events.extend(events)
        latest_ledger = max(latest_ledger, result.get("latestLedger", checkpoint))
        if len(events) < limit:
            break
        cursor = events[-1].get("pagingToken")
        if not cursor:
            raise RuntimeError("getEvents returned a full page without a paging token")
    count = save_events(database, all_events, latest_ledger)
    return count, get_checkpoint(database, start_ledger)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rpc-url", required=True, help="Soroban RPC endpoint")
    parser.add_argument("--contract-id", required=True, help="Vault contract address")
    parser.add_argument("--database", default="events.sqlite3")
    parser.add_argument("--start-ledger", type=int, default=1)
    parser.add_argument("--limit", type=int, default=100, help="Events per RPC request")
    parser.add_argument("--poll-seconds", type=float, default=5.0)
    parser.add_argument("--once", action="store_true", help="Poll once and exit")
    args = parser.parse_args()

    with sqlite3.connect(args.database) as database:
        initialize_database(database)
        request_id = 1
        while True:
            try:
                count, checkpoint = index_once(
                    database,
                    args.rpc_url,
                    args.contract_id,
                    args.start_ledger,
                    args.limit,
                    request_id,
                )
                print(f"indexed {count} event(s), checkpoint ledger {checkpoint}", flush=True)
                request_id += 1
                if args.once:
                    return
                time.sleep(args.poll_seconds)
            except (OSError, urllib.error.URLError, RuntimeError, KeyError, ValueError) as error:
                if args.once:
                    raise SystemExit(f"indexing failed: {error}") from error
                print(f"indexing failed: {error}; retrying in {args.poll_seconds:g}s", flush=True)
                time.sleep(args.poll_seconds)


if __name__ == "__main__":
    main()