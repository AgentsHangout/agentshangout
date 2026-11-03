use super::*;

impl Db {
    pub(crate) fn rename_bot(&self, id: &str, owner: &str, name: &str) -> rusqlite::Result<bool> {
        Ok(self.conn().execute(
            "UPDATE agents SET name=?3 WHERE id=?1 AND owner_id=?2",
            params![id, owner, name],
        )? == 1)
    }
}
