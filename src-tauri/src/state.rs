//! 탭 목록과 탭 그룹 상태. 상태의 원본은 호스트가 갖고, UI는 받아서 그리기만 한다 (spec 3장).
//! 목록 순서는 화면 순서와 같다: 0번이 맨 위. 같은 그룹의 탭은 늘 붙어 있다.

use serde::{Deserialize, Serialize};

/// 기억하는 닫은 탭 수 (spec 5.1)
pub const MAX_CLOSED: usize = 20;

pub type TabId = u64;
pub type GroupId = u64;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub url: String,
    /// PNG 데이터 URL. 없으면 UI가 기본 아이콘을 그린다.
    pub favicon: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub group: Option<GroupId>,
    /// 소리를 내는 중 (spec 4.5)
    pub audible: bool,
    /// 사용자가 "항상 유지"로 표시한 탭은 재우거나 버리지 않는다 (spec 7장)
    pub keep_alive: bool,
}

impl Tab {
    /// 페이지 정보를 지워 빈 탭(새 탭 화면)으로 만든다. 그룹과 "항상 유지"는 남는다.
    pub fn clear_page(&mut self) {
        self.url.clear();
        self.title.clear();
        self.favicon = None;
        self.loading = false;
        self.can_go_back = false;
        self.can_go_forward = false;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: GroupId,
    pub title: String,
    pub collapsed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub tabs: Vec<Tab>,
    pub groups: Vec<Group>,
    pub active: Option<TabId>,
}

/// 닫은 탭 (되살리기용)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClosedTab {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub favicon: Option<String>,
    /// 닫을 때의 자리
    pub index: usize,
}

/// 세션 파일 (spec 8장 state\session.json)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    pub version: u32,
    pub tabs: Vec<SavedTab>,
    pub groups: Vec<Group>,
    /// 선택된 탭의 자리
    pub active: Option<usize>,
    pub closed: Vec<ClosedTab>,
    pub sidebar_collapsed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedTab {
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
    pub group: Option<GroupId>,
    pub keep_alive: bool,
}

pub const SESSION_VERSION: u32 = 1;

#[derive(Debug, Default)]
pub struct Tabs {
    list: Vec<Tab>,
    groups: Vec<Group>,
    active: Option<TabId>,
    next_id: u64,
    /// 최근 닫은 탭. 마지막이 가장 최근 (spec 5.1)
    closed: Vec<ClosedTab>,
}

impl Tabs {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { tabs: self.list.clone(), groups: self.groups.clone(), active: self.active }
    }

    pub fn active(&self) -> Option<TabId> {
        self.active
    }

    pub fn ids(&self) -> Vec<TabId> {
        self.list.iter().map(|t| t.id).collect()
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.list.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.list.iter_mut().find(|t| t.id == id)
    }

    pub fn index_of(&self, id: TabId) -> Option<usize> {
        self.list.iter().position(|t| t.id == id)
    }

    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// 빈 탭을 `index` 자리에 넣고 id를 돌려준다. 활성화는 하지 않는다.
    pub fn insert_new(&mut self, index: usize, group: Option<GroupId>) -> TabId {
        let id = self.new_id();
        let index = index.min(self.list.len());
        let group = group.filter(|g| self.groups.iter().any(|x| x.id == *g));
        self.list.insert(index, Tab { id, group, ..Default::default() });
        self.normalize();
        id
    }

    pub fn activate(&mut self, id: TabId) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        self.active = Some(id);
        // 접힌 그룹 안의 탭을 고르면 그룹을 편다
        if let Some(g) = self.get(id).and_then(|t| t.group) {
            if let Some(group) = self.groups.iter_mut().find(|x| x.id == g) {
                group.collapsed = false;
            }
        }
        true
    }

    /// 탭을 닫는다. 선택된 탭이었다면 바로 아래 탭, 없으면 위 탭을 선택한다 (spec 5.1).
    /// 목록이 비면 `active`는 None이 되고, 새 탭을 여는 일은 호출하는 쪽이 한다.
    pub fn close(&mut self, id: TabId) -> Option<Tab> {
        let index = self.index_of(id)?;
        let tab = self.list.remove(index);
        if !tab.url.is_empty() {
            self.closed.push(ClosedTab { url: tab.url.clone(), title: tab.title.clone(), favicon: tab.favicon.clone(), index });
            if self.closed.len() > MAX_CLOSED {
                self.closed.remove(0);
            }
        }
        if self.active == Some(id) {
            let next = if index < self.list.len() {
                Some(self.list[index].id)
            } else if index > 0 {
                Some(self.list[index - 1].id)
            } else {
                None
            };
            self.active = None;
            if let Some(n) = next {
                self.activate(n);
            }
        }
        self.normalize();
        Some(tab)
    }

    /// 가장 최근에 닫은 탭을 원래 자리에 되살린다. 새 탭 id와 주소를 돌려준다 (spec 5.1).
    pub fn reopen_closed(&mut self) -> Option<(TabId, String)> {
        let c = self.closed.pop()?;
        let id = self.insert_new(c.index, None);
        let t = self.get_mut(id)?;
        t.url = c.url.clone();
        t.title = c.title;
        t.favicon = c.favicon;
        self.activate(id);
        Some((id, c.url))
    }

    /// 목록에서 `id` 다음(또는 앞) 탭 (Ctrl+Tab, Ctrl+Shift+Tab)
    pub fn neighbor(&self, id: TabId, forward: bool) -> Option<TabId> {
        let i = self.index_of(id)?;
        let n = self.list.len();
        if n < 2 {
            return None;
        }
        let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
        Some(self.list[j].id)
    }

    pub fn to_session(&self, sidebar_collapsed: bool) -> Session {
        Session {
            version: SESSION_VERSION,
            tabs: self
                .list
                .iter()
                .map(|t| SavedTab { url: t.url.clone(), title: t.title.clone(), favicon: t.favicon.clone(), group: t.group, keep_alive: t.keep_alive })
                .collect(),
            groups: self.groups.clone(),
            active: self.active.and_then(|a| self.index_of(a)),
            closed: self.closed.clone(),
            sidebar_collapsed,
        }
    }

    /// 저장한 세션으로 목록을 만든다. 웹뷰는 아직 없다 (누를 때 불러온다, spec 5.4).
    pub fn from_session(s: Session) -> Tabs {
        let mut t = Tabs::default();
        let mut group_map = Vec::new();
        for g in s.groups {
            let id = t.new_id();
            group_map.push((g.id, id));
            t.groups.push(Group { id, ..g });
        }
        for saved in s.tabs {
            let id = t.new_id();
            let group = saved.group.and_then(|old| group_map.iter().find(|(o, _)| *o == old).map(|(_, n)| *n));
            t.list.push(Tab { id, title: saved.title, url: saved.url, favicon: saved.favicon, group, keep_alive: saved.keep_alive, ..Default::default() });
        }
        t.closed = s.closed;
        t.closed.truncate(MAX_CLOSED);
        t.normalize();
        let active = s.active.and_then(|i| t.list.get(i)).or(t.list.first()).map(|x| x.id);
        if let Some(a) = active {
            t.activate(a);
        }
        t
    }

    /// 탭을 `before` 탭 앞(없으면 맨 끝)으로 옮기고 `group`에 넣는다 (None이면 그룹 밖).
    pub fn move_tab(&mut self, id: TabId, before: Option<TabId>, group: Option<GroupId>) -> bool {
        if before == Some(id) {
            if let Some(t) = self.get_mut(id) {
                t.group = group;
            }
            self.normalize();
            return true;
        }
        let Some(from) = self.index_of(id) else { return false };
        let mut tab = self.list.remove(from);
        tab.group = group.filter(|g| self.groups.iter().any(|x| x.id == *g));
        let to = before.and_then(|b| self.index_of(b)).unwrap_or(self.list.len());
        self.list.insert(to, tab);
        self.normalize();
        true
    }

    /// 탭 하나로 새 그룹을 만든다.
    pub fn create_group(&mut self, tab: TabId, title: String) -> Option<GroupId> {
        self.index_of(tab)?;
        let id = self.new_id();
        self.groups.push(Group { id, title, collapsed: false });
        self.get_mut(tab)?.group = Some(id);
        self.normalize();
        Some(id)
    }

    pub fn rename_group(&mut self, id: GroupId, title: String) -> bool {
        self.groups.iter_mut().find(|g| g.id == id).map(|g| g.title = title).is_some()
    }

    pub fn set_group_collapsed(&mut self, id: GroupId, collapsed: bool) -> bool {
        self.groups.iter_mut().find(|g| g.id == id).map(|g| g.collapsed = collapsed).is_some()
    }

    /// 그룹을 푼다. 탭은 그 자리에 남는다.
    pub fn ungroup(&mut self, id: GroupId) {
        for t in self.list.iter_mut().filter(|t| t.group == Some(id)) {
            t.group = None;
        }
        self.normalize();
    }

    /// 탭 하나를 그룹에서 뺀다. 그룹 바로 아래로 나온다.
    pub fn remove_from_group(&mut self, tab: TabId) {
        let Some(g) = self.get(tab).and_then(|t| t.group) else { return };
        let after_group = self.list.iter().rposition(|t| t.group == Some(g)).map(|i| i + 1);
        let before = after_group.and_then(|i| self.list.get(i)).map(|t| t.id);
        self.move_tab(tab, before, None);
    }

    /// 같은 그룹의 탭을 첫 탭 자리에 모으고, 빈 그룹을 지운다.
    fn normalize(&mut self) {
        let mut out: Vec<Tab> = Vec::with_capacity(self.list.len());
        let mut seen: Vec<GroupId> = Vec::new();
        let list = std::mem::take(&mut self.list);
        for t in &list {
            match t.group {
                None => out.push(t.clone()),
                Some(g) if !seen.contains(&g) => {
                    seen.push(g);
                    out.extend(list.iter().filter(|x| x.group == Some(g)).cloned());
                }
                Some(_) => {}
            }
        }
        self.list = out;
        self.groups.retain(|g| seen.contains(&g.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three() -> (Tabs, TabId, TabId, TabId) {
        let mut t = Tabs::default();
        let c = t.insert_new(0, None);
        let b = t.insert_new(0, None);
        let a = t.insert_new(0, None);
        (t, a, b, c) // 화면 순서: a, b, c
    }

    #[test]
    fn new_tab_goes_on_top() {
        let (t, a, b, c) = three();
        assert_eq!(t.ids(), vec![a, b, c]);
    }

    #[test]
    fn closing_active_selects_tab_below() {
        let (mut t, a, b, c) = three();
        t.activate(b);
        t.close(b);
        assert_eq!(t.active(), Some(c));
        assert_eq!(t.ids(), vec![a, c]);
    }

    #[test]
    fn closing_last_active_selects_tab_above() {
        let (mut t, _a, b, c) = three();
        t.activate(c);
        t.close(c);
        assert_eq!(t.active(), Some(b));
    }

    #[test]
    fn closing_inactive_keeps_selection() {
        let (mut t, a, b, _c) = three();
        t.activate(a);
        t.close(b);
        assert_eq!(t.active(), Some(a));
    }

    #[test]
    fn closing_only_tab_leaves_nothing_selected() {
        let mut t = Tabs::default();
        let a = t.insert_new(0, None);
        t.activate(a);
        t.close(a);
        assert_eq!(t.active(), None);
        assert!(t.ids().is_empty());
    }

    #[test]
    fn insert_above_current() {
        let (mut t, a, b, c) = three();
        let idx = t.index_of(b).unwrap();
        let d = t.insert_new(idx, None);
        assert_eq!(t.ids(), vec![a, d, b, c]);
    }

    #[test]
    fn move_before_and_to_end() {
        let (mut t, a, b, c) = three();
        t.move_tab(c, Some(a), None);
        assert_eq!(t.ids(), vec![c, a, b]);
        t.move_tab(c, None, None);
        assert_eq!(t.ids(), vec![a, b, c]);
    }

    #[test]
    fn group_members_stay_together() {
        let (mut t, a, b, c) = three();
        let g = t.create_group(a, "G".into()).unwrap();
        // c를 그룹에 넣으면 a 바로 뒤로 모인다
        t.move_tab(c, None, Some(g));
        assert_eq!(t.ids(), vec![a, c, b]);
        assert_eq!(t.get(c).unwrap().group, Some(g));
    }

    #[test]
    fn remove_from_group_lands_below_group() {
        let (mut t, a, b, c) = three();
        let g = t.create_group(a, "G".into()).unwrap();
        t.move_tab(b, Some(c), Some(g));
        assert_eq!(t.ids(), vec![a, b, c]);
        t.remove_from_group(a);
        assert_eq!(t.ids(), vec![b, a, c]);
        assert_eq!(t.get(a).unwrap().group, None);
    }

    #[test]
    fn empty_group_disappears() {
        let (mut t, a, _b, _c) = three();
        let g = t.create_group(a, "G".into()).unwrap();
        t.close(a);
        assert!(!t.snapshot().groups.iter().any(|x| x.id == g));
        let (mut t, a, _, _) = three();
        let g = t.create_group(a, "G".into()).unwrap();
        t.ungroup(g);
        assert!(t.snapshot().groups.is_empty());
    }

    #[test]
    fn activating_tab_expands_its_group() {
        let (mut t, a, b, _c) = three();
        let g = t.create_group(a, "G".into()).unwrap();
        t.set_group_collapsed(g, true);
        t.activate(b);
        assert!(t.snapshot().groups[0].collapsed);
        t.activate(a);
        assert!(!t.snapshot().groups[0].collapsed);
    }

    #[test]
    fn closed_tabs_reopen_in_reverse_order_at_their_place() {
        let (mut t, a, b, c) = three();
        t.get_mut(a).unwrap().url = "https://a/".into();
        t.get_mut(b).unwrap().url = "https://b/".into();
        t.close(b);
        t.close(a);
        let (_, url) = t.reopen_closed().unwrap();
        assert_eq!(url, "https://a/");
        let (nb, url) = t.reopen_closed().unwrap();
        assert_eq!(url, "https://b/");
        assert_eq!(t.index_of(nb), Some(1));
        assert_eq!(t.active(), Some(nb));
        assert!(t.reopen_closed().is_none());
        let _ = c;
    }

    #[test]
    fn only_twenty_closed_tabs_are_kept() {
        let mut t = Tabs::default();
        for i in 0..25 {
            let id = t.insert_new(0, None);
            t.get_mut(id).unwrap().url = format!("https://x/{i}");
            t.close(id);
        }
        let mut n = 0;
        while t.reopen_closed().is_some() {
            n += 1;
        }
        assert_eq!(n, MAX_CLOSED);
    }

    #[test]
    fn session_round_trip() {
        let (mut t, a, b, c) = three();
        for (id, u) in [(a, "https://a/"), (b, "https://b/"), (c, "https://c/")] {
            t.get_mut(id).unwrap().url = u.into();
        }
        let g = t.create_group(b, "G".into()).unwrap();
        t.move_tab(c, None, Some(g));
        t.activate(c);
        let s = t.to_session(false);
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        let r = Tabs::from_session(back);
        let snap = r.snapshot();
        let urls: Vec<_> = snap.tabs.iter().map(|x| x.url.as_str()).collect();
        assert_eq!(urls, vec!["https://a/", "https://b/", "https://c/"]);
        assert_eq!(snap.groups.len(), 1);
        assert_eq!(snap.tabs[1].group, snap.tabs[2].group);
        assert!(snap.tabs[1].group.is_some());
        assert_eq!(r.get(snap.active.unwrap()).unwrap().url, "https://c/");
    }

    #[test]
    fn next_and_previous_tab_wrap() {
        let (t, a, b, c) = three();
        assert_eq!(t.neighbor(a, true), Some(b));
        assert_eq!(t.neighbor(c, true), Some(a));
        assert_eq!(t.neighbor(a, false), Some(c));
    }

    #[test]
    fn new_tab_in_opener_group() {
        let (mut t, a, b, _c) = three();
        let g = t.create_group(a, "G".into()).unwrap();
        t.move_tab(b, None, Some(g));
        let idx = t.index_of(b).unwrap();
        let d = t.insert_new(idx, Some(g));
        assert_eq!(t.get(d).unwrap().group, Some(g));
        assert_eq!(&t.ids()[..3], &[a, d, b]);
    }
}
