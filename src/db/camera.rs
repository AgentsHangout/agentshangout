use super::*;
use crate::camera::{CameraSubject, PhotoStore, StoredPhoto, PHOTOS_ON_SITE, PHOTOS_PER_OWNER};

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

    /// CP15, CP16: stores a photo, or says why not. An owner already keeping PHOTOS_PER_OWNER loses
    /// their oldest in the same transaction, so captures storing at once can't keep more or take
    /// down the same one twice. A full site refuses rather than taking down anyone's photo.
    pub fn store_room_photo(
        &self,
        photo: &StoredPhoto,
        owner: &str,
        subjects: &[CameraSubject],
        now: i64,
    ) -> rusqlite::Result<PhotoStore> {
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
            return Ok(PhotoStore::AccessChanged);
        }
        for subject in subjects {
            let still: bool = tx.query_row(
                &format!("SELECT {SUBJECT_OK}"),
                params![subject.kind, subject.subject_id, subject.owner_id, subject.stamp],
                |r| r.get(0),
            )?;
            if !still {
                return Ok(PhotoStore::AccessChanged);
            }
        }
        let total: i64 = tx.query_row("SELECT COUNT(*) FROM room_photos", [], |r| r.get(0))?;
        let own: i64 =
            tx.query_row("SELECT COUNT(*) FROM room_photos WHERE owner_id=?", [owner], |r| {
                r.get(0)
            })?;
        let mut rotated = None;
        if own >= PHOTOS_PER_OWNER {
            // CP16: the oldest go, so the owner keeps PHOTOS_PER_OWNER with the new one.
            let oldest: Vec<String> = tx
                .prepare(
                    "SELECT id FROM room_photos WHERE owner_id=? ORDER BY expires_at,created_at,id LIMIT ?",
                )?
                .query_map(params![owner, own - PHOTOS_PER_OWNER + 1], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in &oldest {
                tx.execute("DELETE FROM room_photos WHERE id=?", [id])?;
            }
            rotated = oldest.into_iter().next();
        } else if total >= PHOTOS_ON_SITE {
            let next_frees_at: i64 =
                tx.query_row("SELECT MIN(expires_at) FROM room_photos", [], |r| r.get(0))?;
            return Ok(PhotoStore::SiteFull { next_frees_at });
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
        Ok(PhotoStore::Stored { rotated })
    }

    /// CP17: takes down one of this owner's unexpired photos, with its bytes and subjects. False
    /// when there's no such photo of theirs.
    pub fn delete_room_photo(&self, id: &str, owner: &str, now: i64) -> rusqlite::Result<bool> {
        Ok(self.conn().execute(
            "DELETE FROM room_photos WHERE id=? AND owner_id=? AND expires_at>?",
            params![id, owner, now],
        )? > 0)
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
        assert!(db.store_room_photo(&photo("p"), "a", &both, 100).unwrap().is_stored());
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
                .unwrap()
                .is_stored());
            db.set_camera_allowed(kind, id, false).unwrap();
            assert!(db.camera_subject(kind, id).unwrap().is_none());
            assert!(db.room_photo("old", 100).unwrap().is_none());
            db.set_camera_allowed(kind, id, true).unwrap();
            let new = subject(&db, kind, id);
            assert_ne!(old.stamp, new.stamp);
            assert!(db.room_photo("old", 100).unwrap().is_none());
            assert!(!db
                .store_room_photo(&photo("late-old-render"), "a", std::slice::from_ref(&old), 100)
                .unwrap()
                .is_stored());
            assert!(db
                .store_room_photo(&photo("new"), "a", std::slice::from_ref(&new), 100)
                .unwrap()
                .is_stored());
        }
    }
    #[test]
    fn a_room_with_photos_off_stores_and_serves_none_and_on_again_brings_none_back() {
        let db = fixture();
        let bot = subject(&db, "bot", "bot");
        assert!(db
            .store_room_photo(&photo("old"), "a", std::slice::from_ref(&bot), 100)
            .unwrap()
            .is_stored());
        let before = db.camera_room_epoch("r").unwrap();
        db.set_camera_allowed("room", "r", false).unwrap();
        assert!(db.room_photo("old", 100).unwrap().is_none());
        let mut late = photo("while-off");
        late.room_epoch = db.camera_room_epoch("r").unwrap();
        assert!(!db
            .store_room_photo(&late, "a", std::slice::from_ref(&bot), 100)
            .unwrap()
            .is_stored());
        db.set_camera_allowed("room", "r", true).unwrap();
        assert!(db.room_photo("old", 100).unwrap().is_none());
        assert!(!db
            .store_room_photo(&photo("started-before"), "a", std::slice::from_ref(&bot), 100)
            .unwrap()
            .is_stored());
        assert_ne!(before, db.camera_room_epoch("r").unwrap());
    }
    #[test]
    fn a_floor_closing_still_invalidates_photos_and_renders() {
        let db = fixture();
        let bot = subject(&db, "bot", "bot");
        assert!(db
            .store_room_photo(&photo("new"), "a", std::slice::from_ref(&bot), 100)
            .unwrap()
            .is_stored());
        db.conn().execute("UPDATE floors SET access='private' WHERE hangout_id='r'", []).unwrap();
        db.conn().execute("UPDATE floors SET access='public' WHERE hangout_id='r'", []).unwrap();
        assert!(db.room_photo("new", 100).unwrap().is_none());
        assert!(!db
            .store_room_photo(&photo("late-public-render"), "a", std::slice::from_ref(&bot), 100)
            .unwrap()
            .is_stored());
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
        assert!(db
            .store_room_photo(&photo("new"), "a", std::slice::from_ref(&bot), 100)
            .unwrap()
            .is_stored());
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
            assert!(db
                .store_room_photo(p, "a", std::slice::from_ref(&grant), 100)
                .unwrap()
                .is_stored());
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
        assert!(db
            .store_room_photo(&photo("old"), "a", std::slice::from_ref(&old), 100)
            .unwrap()
            .is_stored());
        db.conn().execute("UPDATE agents SET owner_id='b' WHERE id='bot'", []).unwrap();
        assert!(db.room_photo("old", 100).unwrap().is_none());
        db.set_camera_allowed("bot", "bot", false).unwrap();
        db.conn().execute("UPDATE agents SET owner_id='a' WHERE id='bot'", []).unwrap();
        // The new owner starts with photos on, whatever the last owner chose.
        assert!(db.camera_allowed("bot", "bot").unwrap());
        assert!(db.room_photo("old", 100).unwrap().is_none());
        assert!(!db
            .store_room_photo(&photo("late"), "a", std::slice::from_ref(&old), 100)
            .unwrap()
            .is_stored());
    }
    fn stored_photo(db: &Db, id: &str, owner: &str, expires_at: i64) -> PhotoStore {
        let mut p = photo(id);
        p.expires_at = expires_at;
        db.store_room_photo(&p, owner, &[subject(db, "bot", "bot")], 100).unwrap()
    }
    fn count(db: &Db, sql: &str) -> i64 {
        db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }
    #[test]
    fn a_change_during_capture_is_access_changed() {
        let db = fixture();
        let bot = subject(&db, "bot", "bot");
        db.set_camera_allowed("bot", "bot", false).unwrap();
        let refused = db.store_room_photo(&photo("p"), "a", std::slice::from_ref(&bot), 100);
        assert_eq!(refused.unwrap(), PhotoStore::AccessChanged);
    }
    #[test]
    fn the_eleventh_photo_takes_down_the_oldest_with_its_bytes_and_subjects() {
        let db = fixture();
        for n in 0..10 {
            let stored = stored_photo(&db, &format!("p{n}"), "a", 90000 - n);
            assert_eq!(stored, PhotoStore::Stored { rotated: None });
        }
        assert_eq!(
            stored_photo(&db, "p10", "a", 95000),
            PhotoStore::Stored { rotated: Some("p9".into()) }
        );
        assert!(db.room_photo("p9", 100).unwrap().is_none());
        assert_eq!(count(&db, "SELECT COUNT(*) FROM room_photos WHERE owner_id='a'"), 10);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM room_photo_figures WHERE photo_id='p9'"), 0);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM room_photo_subjects WHERE photo_id='p9'"), 0);
        // Someone else's photos are never the ones that go.
        assert!(stored_photo(&db, "b0", "b", 80000).is_stored());
        assert_eq!(
            stored_photo(&db, "p11", "a", 95000),
            PhotoStore::Stored { rotated: Some("p8".into()) }
        );
        assert!(db.room_photo("b0", 100).unwrap().is_some());
    }
    #[test]
    fn a_full_site_refuses_with_when_the_soonest_expires_but_a_full_owner_still_rotates() {
        let db = fixture();
        let conn = db.conn();
        for p in 0..19 {
            conn.execute("INSERT INTO people VALUES(?1,?1,'#000000',?1,?1,1)", [format!("o{p}")])
                .unwrap();
        }
        drop(conn);
        for p in 0..19 {
            for n in 0..10 {
                let expires = 90000 + p * 100 + n;
                assert!(
                    stored_photo(&db, &format!("o{p}-{n}"), &format!("o{p}"), expires).is_stored()
                );
            }
        }
        for n in 0..9 {
            assert!(stored_photo(&db, &format!("b{n}"), "b", 70000 + n).is_stored());
        }
        assert!(stored_photo(&db, "a0", "a", 89000).is_stored());
        assert_eq!(count(&db, "SELECT COUNT(*) FROM room_photos"), 200);
        assert_eq!(
            stored_photo(&db, "b9", "b", 99000),
            PhotoStore::SiteFull { next_frees_at: 70000 }
        );
        assert_eq!(count(&db, "SELECT COUNT(*) FROM room_photos"), 200);
        // o0 keeps ten, so its next one takes the place of its own oldest and the site stays at 200.
        assert_eq!(
            stored_photo(&db, "o0-new", "o0", 99000),
            PhotoStore::Stored { rotated: Some("o0-0".into()) }
        );
        assert_eq!(count(&db, "SELECT COUNT(*) FROM room_photos"), 200);
    }
    #[test]
    fn only_the_owner_deletes_a_photo_and_only_before_it_expires() {
        let db = fixture();
        assert!(stored_photo(&db, "p", "a", 1000).is_stored());
        assert!(!db.delete_room_photo("p", "b", 100).unwrap());
        assert!(!db.delete_room_photo("p", "a", 1000).unwrap());
        assert!(db.delete_room_photo("p", "a", 999).unwrap());
        assert!(!db.delete_room_photo("p", "a", 999).unwrap());
        assert_eq!(count(&db, "SELECT COUNT(*) FROM room_photo_figures"), 0);
    }
}
