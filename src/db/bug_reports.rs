//! BG1 to BG3 (#830): in-game bug auto-reports and deduplication.
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

pub struct NewBugReport<'a> {
    pub signature: &'a str,
    pub surface: &'a str,
    pub action: &'a str,
    pub error_message: &'a str,
    pub hangout_id: Option<&'a str>,
    pub reporter_id: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BugReportRecord {
    pub id: i64,
    pub signature: String,
    pub surface: String,
    pub action: String,
    pub error_message: String,
    pub hangout_id: Option<String>,
    pub reporter_id: Option<String>,
    pub github_issue: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub report_count: i64,
    pub is_new: bool,
}

impl crate::db::Db {
    pub fn record_bug_report(
        &self,
        report: &NewBugReport,
        now: i64,
    ) -> rusqlite::Result<BugReportRecord> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        let existing: Option<(i64, Option<i64>, i64, i64)> = tx
            .query_row(
                "SELECT id, github_issue, created_at, report_count
                 FROM auto_bug_reports
                 WHERE signature = ?1",
                params![report.signature],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        if let Some((id, github_issue, created_at, count)) = existing {
            let new_count = count + 1;
            tx.execute(
                "UPDATE auto_bug_reports
                 SET report_count = ?2, updated_at = ?3
                 WHERE id = ?1",
                params![id, new_count, now],
            )?;
            tx.commit()?;
            return Ok(BugReportRecord {
                id,
                signature: report.signature.to_string(),
                surface: report.surface.to_string(),
                action: report.action.to_string(),
                error_message: report.error_message.to_string(),
                hangout_id: report.hangout_id.map(str::to_string),
                reporter_id: report.reporter_id.map(str::to_string),
                github_issue,
                created_at,
                updated_at: now,
                report_count: new_count,
                is_new: false,
            });
        }

        tx.execute(
            "INSERT INTO auto_bug_reports (
                 signature, surface, action, error_message,
                 hangout_id, reporter_id, github_issue,
                 created_at, updated_at, report_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?7, 1)",
            params![
                report.signature,
                report.surface,
                report.action,
                report.error_message,
                report.hangout_id,
                report.reporter_id,
                now,
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;

        Ok(BugReportRecord {
            id,
            signature: report.signature.to_string(),
            surface: report.surface.to_string(),
            action: report.action.to_string(),
            error_message: report.error_message.to_string(),
            hangout_id: report.hangout_id.map(str::to_string),
            reporter_id: report.reporter_id.map(str::to_string),
            github_issue: None,
            created_at: now,
            updated_at: now,
            report_count: 1,
            is_new: true,
        })
    }

    pub fn set_bug_report_github_issue(&self, id: i64, issue_number: i64) -> rusqlite::Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE auto_bug_reports SET github_issue = ?2 WHERE id = ?1",
            params![id, issue_number],
        )?;
        Ok(())
    }

    pub fn list_bug_reports(&self, limit: i64) -> rusqlite::Result<Vec<BugReportRecord>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, signature, surface, action, error_message, hangout_id,
                    reporter_id, github_issue, created_at, updated_at, report_count
             FROM auto_bug_reports
             ORDER BY id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(BugReportRecord {
                id: row.get(0)?,
                signature: row.get(1)?,
                surface: row.get(2)?,
                action: row.get(3)?,
                error_message: row.get(4)?,
                hangout_id: row.get(5)?,
                reporter_id: row.get(6)?,
                github_issue: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                report_count: row.get(10)?,
                is_new: false,
            })
        })?;
        let mut list = Vec::new();
        for row in rows {
            list.push(row?);
        }
        Ok(list)
    }
}
