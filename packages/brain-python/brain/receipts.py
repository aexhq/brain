import json
import os
import sqlite3
from pathlib import Path


class SQLiteReceipts:
    """Durable application receipts; claims without receipts are never re-executed."""

    def __init__(self, path: str | Path, maximum_pending: int = 1024):
        if not isinstance(maximum_pending, int) or maximum_pending < 1:
            raise ValueError("maximum_pending must be a positive integer")
        path = Path(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        descriptor = os.open(path, os.O_CREAT | os.O_RDWR, 0o600)
        os.close(descriptor)
        self.db = sqlite3.connect(path)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.execute("CREATE TABLE IF NOT EXISTS receipts(id TEXT PRIMARY KEY, execution TEXT NOT NULL, receipt TEXT)")
        self.maximum_pending = maximum_pending

    def begin(self, execution):
        body = json.dumps(execution, allow_nan=False, separators=(",", ":"))
        if len(body.encode()) > 262144:
            raise ValueError("execution exceeds receipt-store bound")
        with self.db:
            if self.db.execute("SELECT COUNT(*) FROM receipts").fetchone()[0] >= self.maximum_pending:
                raise RuntimeError("receipt-store pending capacity exhausted; do not execute")
            self.db.execute("INSERT INTO receipts(id,execution) VALUES(?,?)", (execution["operation"]["id"], body))

    def complete(self, execution, receipt):
        body = json.dumps(receipt, allow_nan=False, separators=(",", ":"))
        if len(body.encode()) > 262144:
            raise ValueError("receipt exceeds storage bound; completion remains externally possible")
        with self.db:
            row = self.db.execute("SELECT receipt FROM receipts WHERE id=?", (execution["operation"]["id"],)).fetchone()
            if row is None or (row[0] is not None and row[0] != body):
                raise RuntimeError("missing claim or conflicting receipt")
            self.db.execute("UPDATE receipts SET receipt=? WHERE id=?", (body, execution["operation"]["id"]))

    def pending(self):
        after = ""
        while True:
            page = self.db.execute("SELECT id,execution,receipt FROM receipts WHERE id>? ORDER BY id LIMIT 16", (after,)).fetchall()
            if not page:
                return
            for operation_id, execution, receipt in page:
                after = operation_id
                yield json.loads(execution), json.loads(receipt) if receipt is not None else None

    def acknowledged(self, operation_id):
        with self.db:
            self.db.execute("DELETE FROM receipts WHERE id=?", (operation_id,))

    def close(self):
        self.db.close()
