import { DatabaseSync } from "node:sqlite";
import { closeSync, mkdirSync, openSync } from "node:fs";
import { dirname } from "node:path";
import type { ExecutionReceipts } from "./v2.js";
import type { Execution, Receipt } from "./generated/session-v2.js";

export class SQLiteReceipts implements ExecutionReceipts {
  private readonly db: DatabaseSync;

  constructor(path: string, readonly maximumPending = 1024) {
    if (!Number.isSafeInteger(maximumPending) || maximumPending < 1) throw new RangeError("maximumPending must be a positive safe integer");
    mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
    closeSync(openSync(path, "a", 0o600));
    this.db = new DatabaseSync(path);
    this.db.exec(`PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=5000;
      CREATE TABLE IF NOT EXISTS receipts(id TEXT PRIMARY KEY, execution TEXT NOT NULL, receipt TEXT)`);
  }

  private transaction<T>(work: () => T): T {
    this.db.exec("BEGIN IMMEDIATE");
    try {
      const value = work();
      this.db.exec("COMMIT");
      return value;
    } catch (error) {
      this.db.exec("ROLLBACK");
      throw error;
    }
  }

  private body(value: unknown): string {
    const body = JSON.stringify(value);
    if (Buffer.byteLength(body) > 262144) throw new RangeError("Durable receipt exceeds its byte limit");
    return body;
  }

  async begin(execution: Execution): Promise<void> {
    const body = this.body(execution);
    this.transaction(() => {
      const row = this.db.prepare("SELECT COUNT(*) AS count FROM receipts").get()!;
      if (Number(row.count) >= this.maximumPending) throw new Error("Receipt capacity exhausted; do not execute");
      this.db.prepare("INSERT INTO receipts(id,execution) VALUES(?,?)").run(execution.operation.id, body);
    });
  }

  async complete(execution: Execution, receipt: Receipt): Promise<void> {
    const body = this.body(receipt);
    this.transaction(() => {
      const row = this.db.prepare("SELECT receipt FROM receipts WHERE id=?").get(execution.operation.id);
      if (row === undefined || (row.receipt !== null && row.receipt !== body)) throw new Error("Missing claim or conflicting receipt");
      this.db.prepare("UPDATE receipts SET receipt=? WHERE id=?").run(body, execution.operation.id);
    });
  }

  async *pending(): AsyncGenerator<{ execution: Execution; receipt?: Receipt }> {
    let after = "";
    while (true) {
      // Finish each SQLite read before yielding to network I/O.
      const page = this.db.prepare("SELECT id,execution,receipt FROM receipts WHERE id>? ORDER BY id LIMIT 16").all(after);
      if (page.length === 0) return;
      for (const row of page) {
        after = String(row.id);
        yield { execution: JSON.parse(String(row.execution)) as Execution,
          ...(row.receipt === null ? {} : { receipt: JSON.parse(String(row.receipt)) as Receipt }) };
      }
    }
  }

  async acknowledged(operationId: string): Promise<void> {
    this.db.prepare("DELETE FROM receipts WHERE id=?").run(operationId);
  }

  close(): void { this.db.close(); }
}
