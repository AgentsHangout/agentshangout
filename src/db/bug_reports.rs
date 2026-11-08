//! BG1 to BG3 (#830): in-game bug auto-reports and deduplication.
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

pub struct NewBugReport<'a> {
    pub signature: &'a str,
    pub surface: &'a str,
    pub action: &'a str,
    pub code: &'a str,
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
    pub code: String,
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
    /// Stores a report, or bumps the count of its signature's row. A new row needs the reporter
    /// to have first-filed fewer than `per_person_rows` rows; the count and the insert share one
    /// IMMEDIATE transaction, so two requests at once can't both take the last place. Returns
    /// None, with nothing written, when the reporter is at that cap.
    pub fn record_bug_report(
        &self,
        report: &NewBugReport,
        now: i64,
        per_person_rows: i64,
    ) -> rusqlite::Result<Option<BugReportRecord>> {
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
            return Ok(Some(BugReportRecord {
                id,
                signature: report.signature.to_string(),
                surface: report.surface.to_string(),
                action: report.action.to_string(),
                code: report.code.to_string(),
                error_message: report.error_message.to_string(),
                hangout_id: report.hangout_id.map(str::to_string),
                reporter_id: report.reporter_id.map(str::to_string),
                github_issue,
                created_at,
                updated_at: now,
                report_count: new_count,
                is_new: false,
            }));
        }

        if let Some(person) = report.reporter_id {
            let filed: i64 = tx.query_row(
                "SELECT COUNT(*) FROM auto_bug_reports WHERE reporter_id = ?1",
                params![person],
                |row| row.get(0),
            )?;
            if filed >= per_person_rows {
                return Ok(None);
            }
        }

        tx.execute(
            "INSERT INTO auto_bug_reports (
                 signature, surface, action, code, error_message,
                 hangout_id, reporter_id, github_issue,
                 created_at, updated_at, report_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?8, 1)",
            params![
                report.signature,
                report.surface,
                report.action,
                report.code,
                report.error_message,
                report.hangout_id,
                report.reporter_id,
                now,
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;

        Ok(Some(BugReportRecord {
            id,
            signature: report.signature.to_string(),
            surface: report.surface.to_string(),
            action: report.action.to_string(),
            code: report.code.to_string(),
            error_message: report.error_message.to_string(),
            hangout_id: report.hangout_id.map(str::to_string),
            reporter_id: report.reporter_id.map(str::to_string),
            github_issue: None,
            created_at: now,
            updated_at: now,
            report_count: 1,
            is_new: true,
        }))
    }

    /// Takes one of the day's GitHub issue slots for a new report, inside one IMMEDIATE
    /// transaction, so two requests at once can't both take the last slot. Returns false when
    /// the person or the site is at its cap for `day`; the report then stays in SQLite only.
    pub fn claim_bug_issue_slot(
        &self,
        id: i64,
        person_id: &str,
        day: i64,
        per_person: i64,
        per_site: i64,
    ) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mine: i64 = tx.query_row(
            "SELECT COUNT(*) FROM auto_bug_reports WHERE filed_day = ?1 AND filed_by = ?2",
            params![day, person_id],
            |row| row.get(0),
        )?;
        let all: i64 = tx.query_row(
            "SELECT COUNT(*) FROM auto_bug_reports WHERE filed_day = ?1",
            params![day],
            |row| row.get(0),
        )?;
        let won = mine < per_person && all < per_site;
        if won {
            tx.execute(
                "UPDATE auto_bug_reports SET filed_day = ?2, filed_by = ?3 WHERE id = ?1",
                params![id, day, person_id],
            )?;
        }
        tx.commit()?;
        Ok(won)
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
            "SELECT id, signature, surface, action, code, error_message, hangout_id,
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
                code: row.get(4)?,
                error_message: row.get(5)?,
                hangout_id: row.get(6)?,
                reporter_id: row.get(7)?,
                github_issue: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
                report_count: row.get(11)?,
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
