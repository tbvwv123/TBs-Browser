//! 방문 기록과 다운로드 기록 (spec 5.4, 8장). 파일: state\history.sqlite

use std::{path::Path, sync::Mutex};

use rusqlite::{
    params,
    types::{FromSql, FromSqlResult, ToSqlOutput, ValueRef},
    Connection, OptionalExtension, ToSql,
};
use serde::{Deserialize, Serialize};
use tauri::Url;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Visit {
    pub id: i64,
    pub url: String,
    pub title: String,
    /// 유닉스 시각 (밀리초)
    pub time: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub url: String,
    pub title: String,
    /// "bookmark" 또는 "history"
    pub kind: &'static str,
}

/// 새 탭 화면의 "자주 가는 곳" 한 칸 (spec 4.7)
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopSite {
    /// 그 사이트의 첫 화면 주소 (https://host/)
    pub url: String,
    /// 첫 화면의 제목. 첫 화면에 간 적이 없으면 호스트 이름
    pub title: String,
    pub favicon: Option<String>,
}

/// 다운로드의 상태. UI에 보내는 글자와 DB의 state 칸(TEXT)에 쓰는 글자가 같다 (as_str)
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadState {
    Progress,
    Done,
    Cancelled,
    Failed,
}

impl DownloadState {
    pub fn as_str(self) -> &'static str {
        match self {
            DownloadState::Progress => "progress",
            DownloadState::Done => "done",
            DownloadState::Cancelled => "cancelled",
            DownloadState::Failed => "failed",
        }
    }
}

impl ToSql for DownloadState {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for DownloadState {
    /// 모르는 글자(다른 판이 쓴 것 등)는 실패로 읽는다. 기록을 버리거나 멈추지 않는다
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        Ok([DownloadState::Progress, DownloadState::Done, DownloadState::Cancelled]
            .into_iter()
            .find(|s| s.as_str() == text)
            .unwrap_or(DownloadState::Failed))
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: i64,
    pub url: String,
    pub path: String,
    pub state: DownloadState,
    pub received: i64,
    /// 모르면 -1
    pub total: i64,
    pub time: i64,
}

pub struct History(Mutex<Connection>);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS visits (id INTEGER PRIMARY KEY, url TEXT NOT NULL, title TEXT NOT NULL DEFAULT '', time INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS visits_time ON visits(time);
CREATE INDEX IF NOT EXISTS visits_url ON visits(url);
CREATE TABLE IF NOT EXISTS downloads (id INTEGER PRIMARY KEY, url TEXT NOT NULL, path TEXT NOT NULL, state TEXT NOT NULL,
  received INTEGER NOT NULL DEFAULT 0, total INTEGER NOT NULL DEFAULT -1, time INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS favicons (site TEXT PRIMARY KEY, data TEXT NOT NULL);
INSERT OR IGNORE INTO meta (key, value) VALUES ('version', '1');
";

fn err(e: rusqlite::Error) -> String {
    format!("방문 기록 파일을 다루지 못했습니다({e}). 디스크 공간과 폴더 권한을 확인해 주세요.")
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl History {
    /// 파일을 연다. 열지 못하면 `.bak`으로 남기고 새로 만든다 (spec 8장). 두 번째 값은 알림.
    pub fn open(path: &Path) -> (History, Option<String>) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let try_open = |p: &Path| -> rusqlite::Result<Connection> {
            let c = Connection::open(p)?;
            c.execute_batch(SCHEMA)?;
            Ok(c)
        };
        match try_open(path) {
            Ok(c) => (History(Mutex::new(c)), None),
            Err(e) => {
                let mut bak = path.with_extension("sqlite.bak");
                if bak.exists() {
                    bak = path.with_extension(format!("sqlite.{}.bak", now_ms()));
                }
                let moved = std::fs::rename(path, &bak).is_ok();
                let note = if moved {
                    format!("방문 기록 파일을 읽지 못해 새로 시작했습니다({e}). 원래 파일은 {}에 남겨 두었습니다.", bak.display())
                } else {
                    format!("방문 기록 파일을 열지 못했습니다({e}). 이번 실행에서는 기록을 저장하지 않습니다.")
                };
                let conn = if moved { try_open(path) } else { Err(e) };
                let conn = conn.unwrap_or_else(|_| {
                    let c = Connection::open_in_memory().expect("메모리 DB");
                    c.execute_batch(SCHEMA).expect("스키마");
                    c
                });
                (History(Mutex::new(conn)), Some(note))
            }
        }
    }

    #[cfg(test)]
    pub fn in_memory() -> History {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        History(Mutex::new(c))
    }

    pub fn add_visit(&self, url: &str, title: &str, time: i64) -> Result<i64, String> {
        let c = self.0.lock().unwrap();
        c.execute("INSERT INTO visits (url, title, time) VALUES (?1, ?2, ?3)", params![url, title, time]).map_err(err)?;
        Ok(c.last_insert_rowid())
    }

    pub fn set_title(&self, id: i64, title: &str) -> Result<(), String> {
        self.0.lock().unwrap().execute("UPDATE visits SET title = ?1 WHERE id = ?2", params![title, id]).map(|_| ()).map_err(err)
    }

    /// 최근 순. `query`가 있으면 제목이나 주소에 그 글자가 들어간 것만.
    pub fn search(&self, query: &str, limit: u32, before: Option<i64>) -> Result<Vec<Visit>, String> {
        let c = self.0.lock().unwrap();
        let like = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let mut st = c
            .prepare(
                "SELECT id, url, title, time FROM visits
                 WHERE (?1 = '' OR url LIKE ?2 ESCAPE '\\' OR title LIKE ?2 ESCAPE '\\') AND time < ?3
                 ORDER BY time DESC, id DESC LIMIT ?4",
            )
            .map_err(err)?;
        let rows = st
            .query_map(params![query, like, before.unwrap_or(i64::MAX), limit], |r| {
                Ok(Visit { id: r.get(0)?, url: r.get(1)?, title: r.get(2)?, time: r.get(3)? })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    pub fn delete(&self, id: i64) -> Result<(), String> {
        self.0.lock().unwrap().execute("DELETE FROM visits WHERE id = ?1", params![id]).map(|_| ()).map_err(err)
    }

    pub fn clear(&self) -> Result<(), String> {
        self.0.lock().unwrap().execute("DELETE FROM visits", []).map(|_| ()).map_err(err)
    }

    /// 주소창 자동완성: 자주, 최근에 간 주소 순
    pub fn suggest(&self, query: &str, limit: u32) -> Result<Vec<Suggestion>, String> {
        let c = self.0.lock().unwrap();
        let like = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let mut st = c
            .prepare(
                "SELECT url, (SELECT title FROM visits v2 WHERE v2.url = v.url ORDER BY time DESC LIMIT 1) AS title
                 FROM visits v WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
                 GROUP BY url ORDER BY COUNT(*) DESC, MAX(time) DESC LIMIT ?2",
            )
            .map_err(err)?;
        let rows = st
            .query_map(params![like, limit], |r| Ok(Suggestion { url: r.get(0)?, title: r.get::<_, Option<String>>(1)?.unwrap_or_default(), kind: "history" }))
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    /// 사이트(scheme://host)마다 마지막으로 본 파비콘을 남긴다. 새 탭 화면의 바로가기 아이콘에 쓴다.
    pub fn set_favicon(&self, url: &str, data: &str) -> Result<(), String> {
        let Some(site) = site_of(url) else { return Ok(()) };
        self.0
            .lock()
            .unwrap()
            .execute("INSERT INTO favicons (site, data) VALUES (?1, ?2) ON CONFLICT(site) DO UPDATE SET data = ?2", params![site, data])
            .map(|_| ())
            .map_err(err)
    }

    /// 자주 가는 곳: 최근 방문 5000개를 사이트별로 세어 많은 순 (spec 4.7)
    pub fn top_sites(&self, limit: usize) -> Result<Vec<TopSite>, String> {
        let c = self.0.lock().unwrap();
        let mut st = c.prepare("SELECT url, title FROM visits ORDER BY time DESC, id DESC LIMIT 5000").map_err(err)?;
        let rows: Vec<(String, String)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        // (사이트, 방문 수, 처음 나온 순서 = 최근 순, 첫 화면 제목)
        let mut sites: Vec<(String, u32, usize, Option<String>)> = Vec::new();
        for (i, (url, title)) in rows.iter().enumerate() {
            let Some(site) = site_of(url) else { continue };
            let root = Url::parse(url).map(|u| u.path() == "/" && u.query().is_none()).unwrap_or(false);
            let entry = match sites.iter_mut().find(|s| s.0 == site) {
                Some(e) => e,
                None => {
                    sites.push((site, 0, i, None));
                    sites.last_mut().unwrap()
                }
            };
            entry.1 += 1;
            if root && entry.3.is_none() && !title.trim().is_empty() {
                entry.3 = Some(title.trim().to_string());
            }
        }
        sites.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
        sites.truncate(limit);
        let mut icon = c.prepare("SELECT data FROM favicons WHERE site = ?1").map_err(err)?;
        Ok(sites
            .into_iter()
            .map(|(site, _, _, title)| {
                let favicon = icon.query_row(params![site], |r| r.get::<_, String>(0)).optional().ok().flatten();
                let host = site.split("://").nth(1).unwrap_or(&site);
                let title = title.unwrap_or_else(|| host.strip_prefix("www.").unwrap_or(host).to_string());
                TopSite { url: format!("{site}/"), title, favicon }
            })
            .collect())
    }

    // ───── 다운로드 ─────

    pub fn add_download(&self, url: &str, path: &str, total: i64) -> Result<i64, String> {
        let c = self.0.lock().unwrap();
        c.execute(
            "INSERT INTO downloads (url, path, state, received, total, time) VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            params![url, path, DownloadState::Progress, total, now_ms()],
        )
        .map_err(err)?;
        Ok(c.last_insert_rowid())
    }

    pub fn update_download(&self, id: i64, state: DownloadState, received: i64, total: i64, path: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .execute(
                "UPDATE downloads SET state = ?1, received = ?2, total = ?3, path = ?4 WHERE id = ?5",
                params![state, received, total, path, id],
            )
            .map(|_| ())
            .map_err(err)
    }

    pub fn download(&self, id: i64) -> Option<Download> {
        let c = self.0.lock().unwrap();
        c.query_row("SELECT id, url, path, state, received, total, time FROM downloads WHERE id = ?1", params![id], row_download)
            .optional()
            .ok()
            .flatten()
    }

    pub fn downloads(&self, limit: u32) -> Result<Vec<Download>, String> {
        let c = self.0.lock().unwrap();
        let mut st = c
            .prepare("SELECT id, url, path, state, received, total, time FROM downloads ORDER BY time DESC, id DESC LIMIT ?1")
            .map_err(err)?;
        let rows = st.query_map(params![limit], row_download).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    /// 지난 실행에서 끝나지 않은 다운로드는 실패로 표시한다
    pub fn mark_unfinished_failed(&self) {
        let _ = self.0.lock().unwrap().execute(
            "UPDATE downloads SET state = ?1 WHERE state = ?2",
            params![DownloadState::Failed, DownloadState::Progress],
        );
    }
}

/// "https://www.example.com:8080/a?b" → "https://www.example.com:8080". 웹 주소가 아니면 None.
fn site_of(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    let host = u.host_str()?;
    Some(match u.port() {
        Some(p) => format!("{}://{host}:{p}", u.scheme()),
        None => format!("{}://{host}", u.scheme()),
    })
}

fn row_download(r: &rusqlite::Row) -> rusqlite::Result<Download> {
    Ok(Download { id: r.get(0)?, url: r.get(1)?, path: r.get(2)?, state: r.get(3)?, received: r.get(4)?, total: r.get(5)?, time: r.get(6)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visits_search_delete_clear() {
        let h = History::in_memory();
        let a = h.add_visit("https://a.example/", "에이", 1).unwrap();
        h.add_visit("https://b.example/x", "", 2).unwrap();
        h.set_title(a, "에이 사이트").unwrap();
        let all = h.search("", 10, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].url, "https://b.example/x"); // 최근 순
        assert_eq!(h.search("사이트", 10, None).unwrap().len(), 1);
        assert_eq!(h.search("100%", 10, None).unwrap().len(), 0);
        h.delete(a).unwrap();
        assert_eq!(h.search("", 10, None).unwrap().len(), 1);
        h.clear().unwrap();
        assert!(h.search("", 10, None).unwrap().is_empty());
    }

    #[test]
    fn suggestions_prefer_frequent() {
        let h = History::in_memory();
        h.add_visit("https://rare.example/", "rare", 10).unwrap();
        for t in 0..3 {
            h.add_visit("https://often.example/", "often", t).unwrap();
        }
        let s = h.suggest("example", 6).unwrap();
        assert_eq!(s[0].url, "https://often.example/");
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn top_sites_group_by_site() {
        let h = History::in_memory();
        h.add_visit("https://news.example/a", "기사 A", 1).unwrap();
        h.add_visit("https://news.example/b", "기사 B", 2).unwrap();
        h.add_visit("https://news.example/", "뉴스", 3).unwrap();
        h.add_visit("https://www.shop.example/x", "물건", 4).unwrap();
        h.add_visit("tbs://history", "", 5).unwrap();
        h.set_favicon("https://news.example/zzz", "data:image/png;base64,AA").unwrap();
        let t = h.top_sites(8).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0], TopSite { url: "https://news.example/".into(), title: "뉴스".into(), favicon: Some("data:image/png;base64,AA".into()) });
        assert_eq!(t[1].title, "shop.example");
        assert_eq!(t[1].url, "https://www.shop.example/");
    }

    #[test]
    fn downloads_round_trip() {
        let h = History::in_memory();
        let id = h.add_download("https://x/file.zip", "C:\\d\\file.zip", 100).unwrap();
        h.update_download(id, DownloadState::Done, 100, 100, "C:\\d\\file.zip").unwrap();
        let d = h.download(id).unwrap();
        assert_eq!(d.state, DownloadState::Done);
        let id2 = h.add_download("https://x/b.zip", "C:\\d\\b.zip", -1).unwrap();
        h.mark_unfinished_failed();
        assert_eq!(h.download(id2).unwrap().state, DownloadState::Failed);
        assert_eq!(h.downloads(10).unwrap().len(), 2);
    }

    #[test]
    fn download_state_strings() {
        let h = History::in_memory();
        let id = h.add_download("https://x/a.zip", "C:\\d\\a.zip", -1).unwrap();
        let raw = |h: &History| {
            let c = h.0.lock().unwrap();
            c.query_row("SELECT state FROM downloads WHERE id = ?1", params![id], |r| r.get::<_, String>(0)).unwrap()
        };
        assert_eq!(raw(&h), "progress");
        for (s, text) in [
            (DownloadState::Progress, "progress"),
            (DownloadState::Done, "done"),
            (DownloadState::Cancelled, "cancelled"),
            (DownloadState::Failed, "failed"),
        ] {
            assert_eq!(serde_json::to_value(s).unwrap(), text);
            assert_eq!(serde_json::from_value::<DownloadState>(text.into()).unwrap(), s);
            h.update_download(id, s, 0, -1, "C:\\d\\a.zip").unwrap();
            assert_eq!(raw(&h), text);
            assert_eq!(h.download(id).unwrap().state, s);
        }
        // 모르는 글자는 실패로 읽고 기록은 남긴다
        h.0.lock().unwrap().execute("UPDATE downloads SET state = 'paused' WHERE id = ?1", params![id]).unwrap();
        assert_eq!(h.download(id).unwrap().state, DownloadState::Failed);
        assert_eq!(h.downloads(10).unwrap().len(), 1);
    }

    #[test]
    fn broken_file_is_kept_and_replaced() {
        let dir = std::env::temp_dir().join(format!("tbs-history-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("history.sqlite");
        std::fs::write(&p, b"this is not a sqlite database at all, just text............................................").unwrap();
        let (h, note) = History::open(&p);
        assert!(note.is_some());
        assert!(p.with_extension("sqlite.bak").exists());
        h.add_visit("https://a/", "a", 1).unwrap();
    }
}
