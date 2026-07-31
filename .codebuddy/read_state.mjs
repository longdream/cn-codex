import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync("e:/work/RustWorks/cn-codex/codey/usage.db", { readOnly: true });
const tables = db.prepare("SELECT name FROM sqlite_master WHERE type='table'").all();
console.log("TABLES:", JSON.stringify(tables));
for (const t of tables) {
  if (/state/i.test(t.name)) {
    const cols = db.prepare(`PRAGMA table_info(${t.name})`).all();
    console.log(`COLS ${t.name}:`, JSON.stringify(cols.map((c) => c.name)));
    const rows = db.prepare(`SELECT * FROM ${t.name}`).all();
    for (const r of rows) {
      const k = r.key ?? r.name ?? "";
      if (/provider|model/i.test(String(k))) {
        const v = String(r.value ?? "");
        console.log(`KEY=${k} LEN=${v.length}`);
        console.log(v.slice(0, 4000));
        console.log("---");
      }
    }
  }
}
db.close();
