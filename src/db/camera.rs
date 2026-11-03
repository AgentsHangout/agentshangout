use super::*;
use crate::camera::{CameraSubject, StoredPhoto};

/// CP2: a bot or person still in a photo. No row means photos are on with an empty stamp, and a
/// bot is only itself while the same owner has it.
const SUBJECT_OK: &str =
    "COALESCE((SELECT allowed FROM camera_settings WHERE kind=?1 AND subject_id=?2),1)=1
    AND COALESCE((SELECT stamp FROM camera_settings WHERE kind=?1 AND subject_id=?2),'')=?4
    AND CASE ?1 WHEN 'bot' THEN EXISTS(SELECT 1 FROM agents WHERE id=?2 AND owner_id=?3)
               WHEN 'person' THEN EXISTS(SELECT 1 FROM people WHERE id=?2 AND id=?3) ELSE 0 END";

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

    /// CP2: who this bot or person is in a photo right now, or None when photos are off for them.
    pub fn camera_subject(&self, kind: &str, id: &str) -> rusqlite::Result<Option<CameraSubject>> {
        let owner = match kind {
            "bot" => "SELECT owner_id FROM agents WHERE id=?1",
            "person" => "SELECT id FROM people WHERE id=?1",
            _ => return Ok(None),
        };
        let conn = self.conn();
        let Some(owner_id): Option<String> =
            conn.query_row(owner, [id], |r| r.get(0)).optional()?
        else {
            return Ok(None);
        };
        let setting: Option<(bool, String)> = conn
            .query_row(
                "SELECT allowed,stamp FROM camera_settings WHERE kind=? AND subject_id=?",
                params![kind, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(match setting {
            Some((false, _)) => None,
            setting => Some(CameraSubject {
                kind: kind.into(),
                subject_id: id.into(),
                owner_id,
                stamp: setting.map(|(_, stamp)| stamp).unwrap_or_default(),
            }),
        })
    }

    /// CP2, CP7: whether photos are on for this bot, person or room. On unless someone turned them off.
    pub fn camera_allowed(&self, kind: &str, id: &str) -> rusqlite::Result<bool> {
        Ok(self
            .conn()
            .query_row(
                "SELECT allowed FROM camera_settings WHERE kind=? AND subject_id=?",
                params![kind, id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(true))
    }

    /// CP2, CP7: a new choice gets a new stamp, and every stored photo it covers goes. A room's
    /// photos also get a new epoch, so one still rendering can't be stored.
    pub fn set_camera_allowed(&self, kind: &str, id: &str, allowed: bool) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO camera_settings(kind,subject_id,allowed,stamp) VALUES (?1,?2,?3,?4)
             ON CONFLICT(kind,subject_id) DO UPDATE SET allowed=excluded.allowed,stamp=excluded.stamp",
            params![kind, id, allowed, crate::auth::new_id()],
        )?;
        if kind == "room" {
            tx.execute("DELETE FROM room_photos WHERE hangout_id=?", [id])?;
            tx.execute(
                "INSERT INTO camera_room_epochs(hangout_id,epoch) VALUES(?1,?2)
                 ON CONFLICT(hangout_id) DO UPDATE SET epoch=excluded.epoch",
                params![id, crate::auth::new_id()],
            )?;
        } else {
            tx.execute(
                "DELETE FROM room_photos WHERE id IN
                 (SELECT photo_id FROM room_photo_figures WHERE kind=? AND subject_id=?)",
                params![kind, id],
            )?;
        }
        tx.commit()
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
        let room_open: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM floors WHERE hangout_id=?1 AND access='public')
             AND COALESCE((SELECT allowed FROM camera_settings WHERE kind='room' AND subject_id=?1),1)=1",
            [&photo.room_id],
            |r| r.get(0),
        )?;
        let epoch: String = tx.query_row(
            "SELECT COALESCE((SELECT epoch FROM camera_room_epochs WHERE hangout_id=?),'')",
            [&photo.room_id],
            |r| r.get(0),
        )?;
        if !room_open || epoch != photo.room_epoch || subjects.is_empty() {
            return Ok(false);
        }
        for subject in subjects {
            let still: bool = tx.query_row(
                &format!("SELECT {SUBJECT_OK}"),
                params![subject.kind, subject.subject_id, subject.owner_id, subject.stamp],
                |r| r.get(0),
            )?;
            if !still {
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
            tx.execute("INSERT INTO room_photo_figures(photo_id,kind,subject_id,owner_id,stamp) VALUES (?,?,?,?,?)",params![photo.id,subject.kind,subject.subject_id,subject.owner_id,subject.stamp])?;
            // An older binary, after a rollback, reads these. A stamp is never a consent's grant_id,
            // so it serves none of these photos.
            if subject.kind == "bot" {
                tx.execute("INSERT INTO room_photo_subjects(photo_id,agent_id,owner_id,grant_id) VALUES (?,?,?,?)",params![photo.id,subject.subject_id,subject.owner_id,subject.stamp])?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn room_photo(&self, id: &str, now: i64) -> rusqlite::Result<Option<StoredPhoto>> {
        let conn = self.conn();
        let photo = conn.query_row(
            "SELECT p.id,p.hangout_id,p.created_at,p.expires_at,p.png,p.metadata,p.room_epoch FROM room_photos p JOIN floors f ON f.hangout_id=p.hangout_id
             WHERE p.id=?1 AND p.expires_at>?2 AND f.access='public' AND p.room_epoch=COALESCE((SELECT epoch FROM camera_room_epochs WHERE hangout_id=p.hangout_id),'')
             AND COALESCE((SELECT allowed FROM camera_settings WHERE kind='room' AND subject_id=p.hangout_id),1)=1",
            params![id, now],
            |r| {
                let meta: String = r.get(5)?;
                Ok(StoredPhoto {
                    id: r.get(0)?,
                    room_epoch: r.get(6)?,
                    room_id: r.get(1)?,
                    created_at: r.get(2)?,
                    expires_at: r.get(3)?,
                    png: r.get(4)?,
                    metadata: serde_json::from_str(&meta).unwrap_or_default(),
                })
            },
        ).optional()?;
        let Some(photo) = photo else { return Ok(None) };
        let mut stmt = conn.prepare(&format!(
            "SELECT NOT ({SUBJECT_OK}) FROM room_photo_figures WHERE photo_id=?5 AND kind=?1 AND subject_id=?2"
        ))?;
        let subjects: Vec<(String, String, String, String)> = conn
            .prepare(
                "SELECT kind,subject_id,owner_id,stamp FROM room_photo_figures WHERE photo_id=?",
            )?
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        // A photo from before CP2 changed has no figures, and isn't served.
        if subjects.is_empty() {
            return Ok(None);
        }
        for (kind, subject, owner, stamp) in subjects {
            let gone: bool =
                stmt.query_row(params![kind, subject, owner, stamp, id], |r| r.get(0))?;
            if gone {
                return Ok(None);
            }
        }
        Ok(Some(photo))
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
    fn subject(db: &Db, kind: &str, id: &str) -> CameraSubject {
        db.camera_subject(kind, id).unwrap().unwrap()
    }
    #[test]
    fn photos_are_on_by_default_for_bots_people_and_rooms() {
        let db = fixture();
        assert!(db.camera_allowed("bot", "bot").unwrap());
        assert!(db.camera_allowed("person", "a").unwrap());
        assert!(db.camera_allowed("room", "r").unwrap());
        let both = [subject(&db, "bot", "bot"), subject(&db, "person", "a")];
        assert!(db.store_room_photo(&photo("p"), "a", &both, 100).unwrap());
        assert!(db.room_photo("p", 100).unwrap().is_some());
    }
    #[test]
    fn off_and_on_again_never_resurrects_old_stamps_or_images() {
        for kind in ["bot", "person"] {
            let db = fixture();
            let id = if kind == "bot" { "bot" } else { "a" };
            let old = subject(&db, kind, id);
            assert!(db
                .store_room_photo(&photo("old"), "a", std::slice::from_ref(&old), 100)
                .unwrap());
            db.set_camera_allowed(kind, id, false).unwrap();
            assert!(db.camera_subject(kind, id).unwrap().is_none());
            assert!(db.room_photo("old", 100).unwrap().is_none());
            db.set_camera_allowed(kind, id, true).unwrap();
            let new = subject(&db, kind, id);
            assert_ne!(old.stamp, new.stamp);
            assert!(db.room_photo("old", 100).unwrap().is_none());
            assert!(!db
                .store_room_photo(&photo("late-old-render"), "a", std::slice::from_ref(&old), 100)
                .unwrap());
            assert!(db
                .store_room_photo(&photo("new"), "a", std::slice::from_ref(&new), 100)
                .unwrap());
        }
    }
    #[test]
    fn a_room_with_photos_off_stores_and_serves_none_and_on_again_brings_none_back() {
        let db = fixture();
        let bot = subject(&db, "bot", "bot");
        assert!(db.store_room_photo(&photo("old"), "a", std::slice::from_ref(&bot), 100).unwrap());
        let before = db.camera_room_epoch("r").unwrap();
        db.set_camera_allowed("room", "r", false).unwrap();
        assert!(db.room_photo("old", 100).unwrap().is_none());
        let mut late = photo("while-off");
        late.room_epoch = db.camera_room_epoch("r").unwrap();
        assert!(!db.store_room_photo(&late, "a", std::slice::from_ref(&bot), 100).unwrap());
        db.set_camera_allowed("room", "r", true).unwrap();
        assert!(db.room_photo("old", 100).unwrap().is_none());
        assert!(!db
            .store_room_photo(&photo("started-before"), "a", std::slice::from_ref(&bot), 100)
            .unwrap());
        assert_ne!(before, db.camera_room_epoch("r").unwrap());
    }
    #[test]
    fn a_floor_closing_still_invalidates_photos_and_renders() {
        let db = fixture();
        let bot = subject(&db, "bot", "bot");
        assert!(db.store_room_photo(&photo("new"), "a", std::slice::from_ref(&bot), 100).unwrap());
        db.conn().execute("UPDATE floors SET access='private' WHERE hangout_id='r'", []).unwrap();
        db.conn().execute("UPDATE floors SET access='public' WHERE hangout_id='r'", []).unwrap();
        assert!(db.room_photo("new", 100).unwrap().is_none());
        assert!(!db
            .store_room_photo(&photo("late-public-render"), "a", std::slice::from_ref(&bot), 100)
            .unwrap());
    }
    #[test]
    fn photos_from_before_the_change_are_not_served_and_new_ones_fail_closed_on_an_older_binary() {
        let db = fixture();
        db.conn()
            .execute_batch(
                "INSERT INTO camera_consents VALUES('bot','a',1,'grant');
            INSERT INTO room_photos VALUES('before','r','a',100,86500,x'01','{}','');
            INSERT INTO room_photo_subjects VALUES('before','bot','a','grant');",
            )
            .unwrap();
        assert!(db.room_photo("before", 100).unwrap().is_none());
        let bot = subject(&db, "bot", "bot");
        assert!(db.store_room_photo(&photo("new"), "a", std::slice::from_ref(&bot), 100).unwrap());
        // What v0.1.231 asks before it serves a photo.
        let served: bool = db.conn().query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM room_photo_subjects s LEFT JOIN agents a ON a.id=s.agent_id AND a.owner_id=s.owner_id LEFT JOIN camera_consents c ON c.agent_id=a.id AND c.owner_id=s.owner_id AND c.grant_id=s.grant_id WHERE s.photo_id='new' AND (a.id IS NULL OR c.agent_id IS NULL))
             AND EXISTS(SELECT 1 FROM room_photo_subjects WHERE photo_id='new')",
            [], |r| r.get(0)).unwrap();
        assert!(!served);
    }
    #[test]
    fn expiry_cleanup_deletes_bytes_and_subjects_without_a_new_capture() {
        let db = fixture();
        let grant = subject(&db, "bot", "bot");
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
            conn.query_row("SELECT COUNT(*) FROM room_photo_figures", [], |r| r.get(0)).unwrap();
        assert_eq!((stored, subjects), (1, 1));
        drop(conn);
        assert!(db.room_photo("live", 86500).unwrap().is_some());
        assert_eq!(db.purge_expired_room_photos(86560).unwrap(), 1);
    }
    #[test]
    fn ownership_transfer_invalidates_old_photos_even_if_transferred_back() {
        let db = fixture();
        let old = subject(&db, "bot", "bot");
        assert!(db.store_room_photo(&photo("old"), "a", std::slice::from_ref(&old), 100).unwrap());
        db.conn().execute("UPDATE agents SET owner_id='b' WHERE id='bot'", []).unwrap();
        assert!(db.room_photo("old", 100).unwrap().is_none());
        db.set_camera_allowed("bot", "bot", false).unwrap();
        db.conn().execute("UPDATE agents SET owner_id='a' WHERE id='bot'", []).unwrap();
        // The new owner starts with photos on, whatever the last owner chose.
        assert!(db.camera_allowed("bot", "bot").unwrap());
        assert!(db.room_photo("old", 100).unwrap().is_none());
        assert!(!db
            .store_room_photo(&photo("late"), "a", std::slice::from_ref(&old), 100)
            .unwrap());
    }
}
