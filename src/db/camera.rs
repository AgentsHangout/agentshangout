use super::*;
use crate::camera::{CameraSubject, StoredPhoto};

impl Db {
    pub fn purge_expired_room_photos(&self, now: i64) -> rusqlite::Result<usize> {
        self.conn().execute("DELETE FROM room_photos WHERE expires_at<=?", [now])
    }

    pub fn camera_room_epoch(&self, room: &str) -> rusqlite::Result<String> {
        Ok(self
            .conn()
            .query_row("SELECT epoch FROM camera_room_epochs WHERE hangout_id=?", [room], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or_default())
    }
    pub fn camera_subject(&self, agent_id: &str) -> rusqlite::Result<Option<CameraSubject>> {
        self.conn().query_row("SELECT a.id,a.owner_id,c.grant_id FROM agents a JOIN camera_consents c ON c.agent_id=a.id AND c.owner_id=a.owner_id WHERE a.id=?",[agent_id],|r|Ok(CameraSubject{agent_id:r.get(0)?,owner_id:r.get(1)?,grant_id:r.get(2)?})).optional()
    }
    pub fn camera_consent(&self, agent_id: &str) -> rusqlite::Result<bool> {
        Ok(self.camera_subject(agent_id)?.is_some())
    }
    pub fn set_camera_consent(
        &self,
        agent_id: &str,
        owner: &str,
        allowed: bool,
        now: i64,
    ) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let owns: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM agents WHERE id=? AND owner_id=?)",
            params![agent_id, owner],
            |r| r.get(0),
        )?;
        if !owns {
            return Ok(false);
        }
        if allowed {
            tx.execute("INSERT INTO camera_consents(agent_id,owner_id,granted_at,grant_id) VALUES (?1,?2,?3,?4) ON CONFLICT(agent_id) DO UPDATE SET owner_id=excluded.owner_id,granted_at=excluded.granted_at,grant_id=excluded.grant_id WHERE camera_consents.owner_id!=excluded.owner_id",params![agent_id,owner,now,crate::auth::new_id()])?;
        } else {
            tx.execute(
                "DELETE FROM camera_consents WHERE agent_id=? AND owner_id=?",
                params![agent_id, owner],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn store_room_photo(
        &self,
        photo: &StoredPhoto,
        owner: &str,
        subjects: &[CameraSubject],
        now: i64,
    ) -> rusqlite::Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM room_photos WHERE expires_at<=?", [now])?;
        let room_public: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM floors WHERE hangout_id=? AND access='public')",
            [&photo.room_id],
            |r| r.get(0),
        )?;
        let epoch: String = tx.query_row(
            "SELECT COALESCE((SELECT epoch FROM camera_room_epochs WHERE hangout_id=?),'')",
            [&photo.room_id],
            |r| r.get(0),
        )?;
        if !room_public || epoch != photo.room_epoch || subjects.is_empty() {
            return Ok(false);
        }
        for subject in subjects {
            let allowed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM camera_consents c JOIN agents a ON a.id=c.agent_id AND a.owner_id=c.owner_id WHERE a.id=?1 AND a.owner_id=?2 AND c.grant_id=?3)",params![subject.agent_id,subject.owner_id,subject.grant_id],|r|r.get(0))?;
            if !allowed {
                return Ok(false);
            }
        }
        let total: i64 = tx.query_row("SELECT COUNT(*) FROM room_photos", [], |r| r.get(0))?;
        let own: i64 =
            tx.query_row("SELECT COUNT(*) FROM room_photos WHERE owner_id=?", [owner], |r| {
                r.get(0)
            })?;
        if total >= 200 || own >= 10 {
            return Ok(false);
        }
        tx.execute("INSERT INTO room_photos(id,hangout_id,owner_id,created_at,expires_at,png,metadata,room_epoch) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![photo.id,photo.room_id,owner,photo.created_at,photo.expires_at,photo.png,photo.metadata.to_string(),photo.room_epoch])?;
        for subject in subjects {
            tx.execute("INSERT INTO room_photo_subjects(photo_id,agent_id,owner_id,grant_id) VALUES (?,?,?,?)",params![photo.id,subject.agent_id,subject.owner_id,subject.grant_id])?;
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn room_photo(&self, id: &str, now: i64) -> rusqlite::Result<Option<StoredPhoto>> {
        let conn = self.conn();
        let photo=conn.query_row(
            "SELECT p.id,p.hangout_id,p.created_at,p.expires_at,p.png,p.metadata,p.room_epoch FROM room_photos p JOIN floors f ON f.hangout_id=p.hangout_id
             WHERE p.id=?1 AND p.expires_at>?2 AND f.access='public' AND p.room_epoch=COALESCE((SELECT epoch FROM camera_room_epochs WHERE hangout_id=p.hangout_id),'')
             AND NOT EXISTS(SELECT 1 FROM room_photo_subjects s LEFT JOIN agents a ON a.id=s.agent_id AND a.owner_id=s.owner_id LEFT JOIN camera_consents c ON c.agent_id=a.id AND c.owner_id=s.owner_id AND c.grant_id=s.grant_id WHERE s.photo_id=p.id AND (a.id IS NULL OR c.agent_id IS NULL))",
            params![id,now],|r|{let meta:String=r.get(5)?;Ok(StoredPhoto{id:r.get(0)?,room_epoch:r.get(6)?,room_id:r.get(1)?,created_at:r.get(2)?,expires_at:r.get(3)?,png:r.get(4)?,metadata:serde_json::from_str(&meta).unwrap_or_default()})}).optional()?;
        Ok(photo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Db {
        let db = Db::open(":memory:").unwrap();
        db.conn().execute_batch("INSERT INTO people VALUES('a','Ann','#8866cc','ka','fa',1),('b','Bo','#44aaff','kb','fb',1);
   INSERT INTO hangouts VALUES('r','Room','invite','a',1);
   INSERT INTO spaces(id,owner_id,kind,name,created_at) VALUES('r','a','third','Room',1);
   INSERT INTO floors(hangout_id,space_id,level,access) VALUES('r','r',0,'public');
   INSERT INTO agents(id,owner_id,name,token_hash,created_at) VALUES('bot','a','Fern','token',1);").unwrap();
        assert!(db.set_camera_consent("bot", "a", true, 100).unwrap());
        db
    }
    fn photo(id: &str) -> StoredPhoto {
        StoredPhoto {
            id: id.into(),
            room_epoch: String::new(),
            room_id: "r".into(),
            created_at: 100,
            expires_at: 86500,
            png: vec![1, 2, 3],
            metadata: serde_json::json!({}),
        }
    }
    #[test]
    fn revocation_and_reenable_never_resurrect_old_grants_or_images() {
        let db = fixture();
        let old = db.camera_subject("bot").unwrap().unwrap();
        assert!(db.store_room_photo(&photo("old"), "a", std::slice::from_ref(&old), 100).unwrap());
        db.set_camera_consent("bot", "a", false, 100).unwrap();
        db.set_camera_consent("bot", "a", true, 100).unwrap();
        let new = db.camera_subject("bot").unwrap().unwrap();
        assert_ne!(old.grant_id, new.grant_id);
        assert!(db.room_photo("old", 100).unwrap().is_none());
        assert!(!db
            .store_room_photo(&photo("late-old-render"), "a", std::slice::from_ref(&old), 100)
            .unwrap());
        assert!(db.store_room_photo(&photo("new"), "a", std::slice::from_ref(&new), 100).unwrap());
        db.conn().execute("UPDATE floors SET access='private' WHERE hangout_id='r'", []).unwrap();
        db.conn().execute("UPDATE floors SET access='public' WHERE hangout_id='r'", []).unwrap();
        assert!(db.room_photo("new", 100).unwrap().is_none());
        assert!(!db
            .store_room_photo(&photo("late-public-render"), "a", std::slice::from_ref(&new), 100)
            .unwrap());
    }
    #[test]
    fn expiry_cleanup_deletes_bytes_and_subjects_without_a_new_capture() {
        let db = fixture();
        let grant = db.camera_subject("bot").unwrap().unwrap();
        let expired = photo("expired");
        let mut live = photo("live");
        live.expires_at += 60;
        for p in [&expired, &live] {
            assert!(db.store_room_photo(p, "a", std::slice::from_ref(&grant), 100).unwrap());
        }
        assert_eq!(db.purge_expired_room_photos(86499).unwrap(), 0);
        assert_eq!(db.purge_expired_room_photos(86500).unwrap(), 1);
        let conn = db.conn();
        let stored: i64 =
            conn.query_row("SELECT COUNT(*) FROM room_photos", [], |r| r.get(0)).unwrap();
        let subjects: i64 =
            conn.query_row("SELECT COUNT(*) FROM room_photo_subjects", [], |r| r.get(0)).unwrap();
        assert_eq!((stored, subjects), (1, 1));
        drop(conn);
        assert!(db.room_photo("live", 86500).unwrap().is_some());
        assert_eq!(db.purge_expired_room_photos(86560).unwrap(), 1);
    }
    #[test]
    fn ownership_transfer_revokes_the_grant_even_if_transferred_back() {
        let db = fixture();
        let old = db.camera_subject("bot").unwrap().unwrap();
        assert!(db.store_room_photo(&photo("old"), "a", std::slice::from_ref(&old), 100).unwrap());
        db.conn().execute("UPDATE agents SET owner_id='b' WHERE id='bot'", []).unwrap();
        assert!(!db.set_camera_consent("bot", "a", true, 100).unwrap());
        db.conn().execute("UPDATE agents SET owner_id='a' WHERE id='bot'", []).unwrap();
        assert!(!db.camera_consent("bot").unwrap());
        assert!(db.room_photo("old", 100).unwrap().is_none());
    }
}
