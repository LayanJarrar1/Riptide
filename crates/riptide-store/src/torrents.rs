//! The `torrents` table: what only a BitTorrent download has (see `sql/v4.sql`).

use rusqlite::{params, OptionalExtension, Row};

use riptide_core::{DownloadId, TorrentFileInfo, TorrentInfo};

use crate::store::Store;
use crate::{Error, Result};

const COLUMNS: &str =
    "download_id, info_hash, name, files, selected, sequential, seeding, uploaded, downloaded, peers";

fn json_err(what: &'static str) -> impl Fn(serde_json::Error) -> Error {
    move |source| Error::Json { what, source }
}

fn read(row: &Row<'_>) -> Result<TorrentInfo> {
    let files: String = row.get("files")?;
    let files: Vec<TorrentFileInfo> =
        serde_json::from_str(&files).map_err(json_err("torrents.files"))?;
    let selected: Option<String> = row.get("selected")?;
    let selected = selected
        .map(|s| serde_json::from_str::<Vec<u32>>(&s))
        .transpose()
        .map_err(json_err("torrents.selected"))?;
    let nonneg = |v: i64, what: &str| {
        u64::try_from(v).map_err(|_| Error::Corrupt(format!("torrents.{what} is {v}")))
    };
    Ok(TorrentInfo {
        download_id: DownloadId(row.get("download_id")?),
        info_hash: row.get("info_hash")?,
        name: row.get("name")?,
        files,
        selected,
        sequential: row.get("sequential")?,
        seeding: row.get("seeding")?,
        uploaded: nonneg(row.get("uploaded")?, "uploaded")?,
        downloaded: nonneg(row.get("downloaded")?, "downloaded")?,
        peers: row.get("peers")?,
    })
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

impl Store {
    /// Stores the torrent facts of a download (kind `torrent`). Fails if the download does not
    /// exist or the info-hash is already in the list.
    pub fn insert_torrent(&mut self, t: &TorrentInfo) -> Result<()> {
        let files = serde_json::to_string(&t.files).map_err(json_err("torrents.files"))?;
        let selected = t
            .selected
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(json_err("torrents.selected"))?;
        self.conn.execute(
            "INSERT INTO torrents (download_id, info_hash, name, files, selected, sequential, \
             seeding, uploaded, downloaded, peers) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                t.download_id.0,
                t.info_hash,
                t.name,
                files,
                selected,
                t.sequential,
                t.seeding,
                to_i64(t.uploaded),
                to_i64(t.downloaded),
                t.peers
            ],
        )?;
        Ok(())
    }

    /// The torrent facts of a download, if it is a torrent.
    pub fn get_torrent(&self, id: DownloadId) -> Result<Option<TorrentInfo>> {
        let sql = format!("SELECT {COLUMNS} FROM torrents WHERE download_id = ?1");
        self.conn
            .query_row(&sql, [id.0], |r| Ok(read(r)))
            .optional()?
            .transpose()
    }

    /// The torrent with this info-hash, if it is in the list.
    pub fn torrent_by_hash(&self, info_hash: &str) -> Result<Option<TorrentInfo>> {
        let sql = format!("SELECT {COLUMNS} FROM torrents WHERE info_hash = ?1");
        self.conn
            .query_row(&sql, [info_hash], |r| Ok(read(r)))
            .optional()?
            .transpose()
    }

    /// Every torrent, in download-id order.
    pub fn list_torrents(&self) -> Result<Vec<TorrentInfo>> {
        let sql = format!("SELECT {COLUMNS} FROM torrents ORDER BY download_id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| Ok(read(r)))?;
        rows.map(|r| r?).collect()
    }

    /// Records the numbers that must survive a restart. `uploaded` never goes down.
    pub fn update_torrent_stats(
        &mut self,
        id: DownloadId,
        seeding: bool,
        uploaded: u64,
        downloaded: u64,
        peers: u32,
    ) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE torrents SET seeding = ?2, uploaded = MAX(uploaded, ?3), downloaded = ?4, \
             peers = ?5 WHERE download_id = ?1",
            params![id.0, seeding, to_i64(uploaded), to_i64(downloaded), peers],
        )?;
        if n == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    /// Reopens a `completed` torrent whose data no longer verifies (a re-check found damage):
    /// the one place where `completed` changes back, to `downloading`. Anything else is refused.
    pub fn reopen_completed(&mut self, id: DownloadId) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE downloads SET state = 'downloading', retryable = NULL, error = NULL, \
             completed_at = NULL WHERE id = ?1 AND state = 'completed' AND kind = 'torrent'",
            [id.0],
        )?;
        if n == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    /// Replaces the file selection.
    pub fn set_torrent_selection(
        &mut self,
        id: DownloadId,
        selected: Option<&[u32]>,
    ) -> Result<()> {
        let json = selected
            .map(serde_json::to_string)
            .transpose()
            .map_err(json_err("torrents.selected"))?;
        let n = self.conn.execute(
            "UPDATE torrents SET selected = ?2 WHERE download_id = ?1",
            params![id.0, json],
        )?;
        if n == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riptide_core::{Download, DownloadKind};

    fn torrent_download(store: &mut Store) -> DownloadId {
        let mut d = Download::new(DownloadId(0), "magnet:?xt=urn:btih:x", "name", ".", 1);
        d.kind = DownloadKind::Torrent;
        store.insert_download(&d).unwrap()
    }

    fn info(id: DownloadId, hash: &str) -> TorrentInfo {
        TorrentInfo {
            download_id: id,
            info_hash: hash.into(),
            name: "name".into(),
            files: vec![
                TorrentFileInfo {
                    index: 0,
                    path: "name/a.bin".into(),
                    len: 10,
                },
                TorrentFileInfo {
                    index: 1,
                    path: "name/b.bin".into(),
                    len: 20,
                },
            ],
            selected: Some(vec![1]),
            sequential: true,
            seeding: false,
            uploaded: 0,
            downloaded: 0,
            peers: 0,
        }
    }

    #[test]
    fn torrent_kind_and_facts_round_trip() {
        let mut s = Store::open_in_memory().unwrap();
        let id = torrent_download(&mut s);
        let hash = "a".repeat(40);
        s.insert_torrent(&info(id, &hash)).unwrap();
        assert_eq!(
            s.get_download(id).unwrap().unwrap().kind,
            DownloadKind::Torrent
        );
        assert_eq!(s.get_torrent(id).unwrap().unwrap(), info(id, &hash));
        assert_eq!(s.torrent_by_hash(&hash).unwrap().unwrap().download_id, id);
        assert!(s.torrent_by_hash(&"b".repeat(40)).unwrap().is_none());
        assert_eq!(s.list_torrents().unwrap().len(), 1);
    }

    #[test]
    fn stats_update_and_upload_total_never_goes_down() {
        let mut s = Store::open_in_memory().unwrap();
        let id = torrent_download(&mut s);
        s.insert_torrent(&info(id, &"c".repeat(40))).unwrap();
        s.update_torrent_stats(id, true, 500, 20, 3).unwrap();
        s.update_torrent_stats(id, true, 100, 20, 0).unwrap();
        let t = s.get_torrent(id).unwrap().unwrap();
        assert_eq!(
            (t.uploaded, t.downloaded, t.peers, t.seeding),
            (500, 20, 0, true)
        );
        s.set_torrent_selection(id, None).unwrap();
        assert_eq!(s.get_torrent(id).unwrap().unwrap().selected, None);
        assert!(matches!(
            s.update_torrent_stats(DownloadId(99), false, 0, 0, 0),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn the_same_info_hash_cannot_be_listed_twice_and_removal_cascades() {
        let mut s = Store::open_in_memory().unwrap();
        let hash = "d".repeat(40);
        let a = torrent_download(&mut s);
        s.insert_torrent(&info(a, &hash)).unwrap();
        let b = torrent_download(&mut s);
        assert!(s.insert_torrent(&info(b, &hash)).is_err());
        s.remove_download(a).unwrap();
        assert!(s.get_torrent(a).unwrap().is_none());
    }
}
