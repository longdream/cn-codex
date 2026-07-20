-- SQLite fallback for SmartBrain entry form tools (offline validation).
-- Mirrors the MySQL fixture field coverage as closely as SQLite allows.

CREATE TABLE IF NOT EXISTS sb_entry_form_type_test (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  supplier_name TEXT NOT NULL,
  contract_no TEXT,
  amount REAL NOT NULL,
  quantity INTEGER DEFAULT 1,
  unit_price REAL,
  is_paid INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'draft',
  signed_date TEXT,
  signed_time TEXT,
  signed_at TEXT,
  created_ts TEXT DEFAULT CURRENT_TIMESTAMP,
  remark TEXT,
  meta_json TEXT
);

-- Optional sample:
-- INSERT INTO sb_entry_form_type_test
--   (supplier_name, contract_no, amount, quantity, unit_price, is_paid, status, signed_date, remark, meta_json)
-- VALUES
--   ('华为', 'HT-2026-001', 12000.00, 1, 12000.00, 0, 'signed', date('now'), '今天和华为签了12000元采购合同', '{"source":"manual"}');
