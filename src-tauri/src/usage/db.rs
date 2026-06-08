//! SQLite 数据库管理：建表、迁移、CRUD 操作

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 单条用量记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: i64,
    /// 供应商标识（provider type，如 "openai", "deepseek"）
    pub provider: String,
    /// 模型名称
    pub model: String,
    /// 线程 ID
    pub thread_id: String,
    /// 输入 token 数
    pub prompt_tokens: u64,
    /// 输出 token 数
    pub completion_tokens: u64,
    /// 总 token 数
    pub total_tokens: u64,
    /// 费用（美元，按照 pricing 计算）
    pub cost_usd: f64,
    /// 时间戳（Unix 秒）
    pub timestamp: i64,
}

/// 汇总统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    /// 总请求次数
    pub total_requests: u64,
    /// 总输入 token
    pub total_prompt_tokens: u64,
    /// 总输出 token
    pub total_completion_tokens: u64,
    /// 总 token
    pub total_tokens: u64,
    /// 总费用
    pub total_cost_usd: f64,
}

/// 按天分组的用量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyUsage {
    /// 日期（YYYY-MM-DD）
    pub date: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

/// 按模型分组的用量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub provider: String,
    pub model: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

pub struct UsageDb {
    conn: Mutex<Connection>,
}

impl UsageDb {
    /// 打开或创建数据库
    pub fn open(db_path: &Path) -> AppResult<Self> {
        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Custom(format!("Failed to create db dir: {e}")))?;
        }

        let conn = Connection::open(db_path)
            .map_err(|e| AppError::Custom(format!("Failed to open usage db: {e}")))?;

        // 启用 WAL 模式提升并发性能
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| AppError::Custom(format!("Failed to set pragma: {e}")))?;

        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }

    /// 执行数据库迁移
    fn migrate(&self) -> AppResult<()> {
        let conn = self.conn.lock().map_err(|e| AppError::Custom(format!("Lock error: {e}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                thread_id TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL NOT NULL DEFAULT 0.0,
                timestamp INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_records(timestamp);
            CREATE INDEX IF NOT EXISTS idx_usage_provider ON usage_records(provider);
            CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records(model);
            CREATE INDEX IF NOT EXISTS idx_usage_thread ON usage_records(thread_id);

            CREATE TABLE IF NOT EXISTS pricing (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                model_pattern TEXT NOT NULL UNIQUE,
                prompt_price_per_1m REAL NOT NULL DEFAULT 0.0,
                completion_price_per_1m REAL NOT NULL DEFAULT 0.0,
                updated_at INTEGER NOT NULL
            );"
        ).map_err(|e| AppError::Custom(format!("Migration failed: {e}")))?;
        Ok(())
    }

    /// 插入一条用量记录
    pub fn insert_record(
        &self,
        provider: &str,
        model: &str,
        thread_id: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
        cost_usd: f64,
    ) -> AppResult<i64> {
        let conn = self.conn.lock().map_err(|e| AppError::Custom(format!("Lock error: {e}")))?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO usage_records (provider, model, thread_id, prompt_tokens, completion_tokens, total_tokens, cost_usd, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![provider, model, thread_id, prompt_tokens as i64, completion_tokens as i64, total_tokens as i64, cost_usd, timestamp],
        ).map_err(|e| AppError::Custom(format!("Insert failed: {e}")))?;

        Ok(conn.last_insert_rowid())
    }

    /// 获取全局汇总统计
    pub fn get_stats(&self, since_timestamp: Option<i64>) -> AppResult<UsageStats> {
        let conn = self.conn.lock().map_err(|e| AppError::Custom(format!("Lock error: {e}")))?;
        let (sql, param): (&str, i64) = if let Some(since) = since_timestamp {
            ("SELECT COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0), COALESCE(SUM(cost_usd),0.0) FROM usage_records WHERE timestamp >= ?1", since)
        } else {
            ("SELECT COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0), COALESCE(SUM(cost_usd),0.0) FROM usage_records WHERE 1=1 OR ?1=0", 0)
        };

        let mut stmt = conn.prepare(sql)
            .map_err(|e| AppError::Custom(format!("Prepare failed: {e}")))?;
        let row = stmt.query_row(params![param], |row| {
            Ok(UsageStats {
                total_requests: row.get::<_, i64>(0)? as u64,
                total_prompt_tokens: row.get::<_, i64>(1)? as u64,
                total_completion_tokens: row.get::<_, i64>(2)? as u64,
                total_tokens: row.get::<_, i64>(3)? as u64,
                total_cost_usd: row.get(4)?,
            })
        }).map_err(|e| AppError::Custom(format!("Query failed: {e}")))?;

        Ok(row)
    }

    /// 获取按天分组的用量（最近 N 天）
    pub fn get_daily_usage(&self, days: u32) -> AppResult<Vec<DailyUsage>> {
        let conn = self.conn.lock().map_err(|e| AppError::Custom(format!("Lock error: {e}")))?;
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
            - (days as i64 * 86400);

        let mut stmt = conn.prepare(
            "SELECT date(timestamp, 'unixepoch', 'localtime') as day,
                    COUNT(*),
                    SUM(prompt_tokens),
                    SUM(completion_tokens),
                    SUM(total_tokens),
                    SUM(cost_usd)
             FROM usage_records
             WHERE timestamp >= ?1
             GROUP BY day
             ORDER BY day ASC"
        ).map_err(|e| AppError::Custom(format!("Prepare failed: {e}")))?;

        let rows = stmt.query_map(params![since], |row| {
            Ok(DailyUsage {
                date: row.get(0)?,
                requests: row.get::<_, i64>(1)? as u64,
                prompt_tokens: row.get::<_, i64>(2)? as u64,
                completion_tokens: row.get::<_, i64>(3)? as u64,
                total_tokens: row.get::<_, i64>(4)? as u64,
                cost_usd: row.get(5)?,
            })
        }).map_err(|e| AppError::Custom(format!("Query failed: {e}")))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| AppError::Custom(format!("Row error: {e}")))?);
        }
        Ok(results)
    }

    /// 获取按模型分组的用量统计
    pub fn get_model_usage(&self, since_timestamp: Option<i64>) -> AppResult<Vec<ModelUsage>> {
        let conn = self.conn.lock().map_err(|e| AppError::Custom(format!("Lock error: {e}")))?;
        let since = since_timestamp.unwrap_or(0);

        let mut stmt = conn.prepare(
            "SELECT provider, model, COUNT(*), SUM(prompt_tokens), SUM(completion_tokens), SUM(total_tokens), SUM(cost_usd)
             FROM usage_records
             WHERE timestamp >= ?1
             GROUP BY provider, model
             ORDER BY SUM(total_tokens) DESC"
        ).map_err(|e| AppError::Custom(format!("Prepare failed: {e}")))?;

        let rows = stmt.query_map(params![since], |row| {
            Ok(ModelUsage {
                provider: row.get(0)?,
                model: row.get(1)?,
                requests: row.get::<_, i64>(2)? as u64,
                prompt_tokens: row.get::<_, i64>(3)? as u64,
                completion_tokens: row.get::<_, i64>(4)? as u64,
                total_tokens: row.get::<_, i64>(5)? as u64,
                cost_usd: row.get(6)?,
            })
        }).map_err(|e| AppError::Custom(format!("Query failed: {e}")))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| AppError::Custom(format!("Row error: {e}")))?);
        }
        Ok(results)
    }

    /// 获取最近 N 条用量记录
    pub fn get_recent_records(&self, limit: u32) -> AppResult<Vec<UsageRecord>> {
        let conn = self.conn.lock().map_err(|e| AppError::Custom(format!("Lock error: {e}")))?;

        let mut stmt = conn.prepare(
            "SELECT id, provider, model, thread_id, prompt_tokens, completion_tokens, total_tokens, cost_usd, timestamp
             FROM usage_records
             ORDER BY timestamp DESC
             LIMIT ?1"
        ).map_err(|e| AppError::Custom(format!("Prepare failed: {e}")))?;

        let rows = stmt.query_map(params![limit], |row| {
            Ok(UsageRecord {
                id: row.get(0)?,
                provider: row.get(1)?,
                model: row.get(2)?,
                thread_id: row.get(3)?,
                prompt_tokens: row.get::<_, i64>(4)? as u64,
                completion_tokens: row.get::<_, i64>(5)? as u64,
                total_tokens: row.get::<_, i64>(6)? as u64,
                cost_usd: row.get(7)?,
                timestamp: row.get(8)?,
            })
        }).map_err(|e| AppError::Custom(format!("Query failed: {e}")))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| AppError::Custom(format!("Row error: {e}")))?);
        }
        Ok(results)
    }
}
