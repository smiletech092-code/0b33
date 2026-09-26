# Reference Event Indexer

[`scripts/indexer.py`](../scripts/indexer.py) is a minimal, dependency-free reference
indexer for the subscription vault. It polls Soroban RPC `getEvents` and stores the
raw event envelope in a local SQLite database. Soroban contract events are served by
the Soroban RPC endpoint, not the classic Horizon REST API.

## Run

```sh
python3 scripts/indexer.py \
  --rpc-url https://soroban-testnet.stellar.org \
  --contract-id C... \
  --start-ledger 1 \
  --database events.sqlite3
```

Use `--once` for a single poll, which is useful for backfills and cron jobs. The
indexer resumes from `indexer_state.last_ledger` and can safely be restarted: event
IDs are the primary key, so replayed ledger ranges are ignored.

## Inspect events

The `topic_json` and `value_json` columns contain the RPC response values, including
their base64 XDR representation. Decode those values according to the stable event
schemas in [`events-schema-canonical.md`](events-schema-canonical.md). Keeping the raw
envelope allows consumers to migrate their projections when the schema version changes.

```sh
sqlite3 events.sqlite3 \
  'select ledger, event_type, topic_json, value_json from events order by ledger;'
```

For production use, add network-specific finality/reorg handling, metrics, and a
projection layer that decodes the XDR values. This example intentionally keeps the
capture layer small and loss-resistant.