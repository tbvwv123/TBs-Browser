//! 북마크 트리 (spec 5.3, 8장). 폴더를 겹칠 수 있고, 폴더와 구역 전체의 접힘 상태를 기억한다.
//! 파일: state\bookmarks.json

use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Node {
    Link {
        id: String,
        title: String,
        url: String,
        /// 추가할 때의 파비콘 (PNG 데이터 URL). 없을 수 있다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
    },
    Folder {
        id: String,
        title: String,
        #[serde(default = "yes")]
        open: bool,
        #[serde(default)]
        items: Vec<Node>,
    },
}

fn yes() -> bool {
    true
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Node::Link { id, .. } | Node::Folder { id, .. } => id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bookmarks {
    pub version: u32,
    /// 북마크 구역 전체가 접혔는지
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub items: Vec<Node>,
}

impl Default for Bookmarks {
    fn default() -> Self {
        Bookmarks { version: VERSION, collapsed: false, items: Vec::new() }
    }
}

const NOT_FOUND: &str = "북마크를 찾지 못했습니다. 목록이 바뀌었을 수 있으니 다시 시도해 주세요.";

impl Bookmarks {
    /// 옛 형식을 지금 형식으로 바꾼다. 지금은 버전 1뿐이다 (spec 8장).
    pub fn migrate(mut self) -> Self {
        self.version = VERSION;
        self
    }

    fn next_id(&self, prefix: char) -> String {
        fn max_num(items: &[Node], prefix: char, m: &mut u64) {
            for n in items {
                if let Some(v) = n.id().strip_prefix(prefix).and_then(|s| s.parse::<u64>().ok()) {
                    *m = (*m).max(v);
                }
                if let Node::Folder { items, .. } = n {
                    max_num(items, prefix, m);
                }
            }
        }
        let mut m = 0;
        max_num(&self.items, prefix, &mut m);
        format!("{prefix}{}", m + 1)
    }

    /// `parent` 폴더(없으면 맨 위)의 자식 목록
    fn children_mut(&mut self, parent: Option<&str>) -> Result<&mut Vec<Node>, String> {
        match parent {
            None => Ok(&mut self.items),
            Some(pid) => match find_mut(&mut self.items, pid) {
                Some(Node::Folder { items, .. }) => Ok(items),
                Some(_) => Err("북마크 안에는 넣을 수 없습니다. 폴더를 골라 주세요.".into()),
                None => Err(NOT_FOUND.into()),
            },
        }
    }

    /// `before` 앞에 넣는다. `before`가 없거나 못 찾으면 맨 끝에 넣는다.
    fn insert(&mut self, parent: Option<&str>, before: Option<&str>, node: Node) -> Result<(), String> {
        let list = self.children_mut(parent)?;
        let at = before.and_then(|b| list.iter().position(|n| n.id() == b)).unwrap_or(list.len());
        list.insert(at, node);
        Ok(())
    }

    pub fn add_link(&mut self, parent: Option<&str>, before: Option<&str>, title: String, url: String, icon: Option<String>) -> Result<String, String> {
        let id = self.next_id('b');
        let title = if title.trim().is_empty() { url.clone() } else { title };
        self.insert(parent, before, Node::Link { id: id.clone(), title, url, icon })?;
        Ok(id)
    }

    pub fn add_folder(&mut self, parent: Option<&str>, before: Option<&str>, title: String) -> Result<String, String> {
        let id = self.next_id('f');
        self.insert(parent, before, Node::Folder { id: id.clone(), title, open: true, items: Vec::new() })?;
        Ok(id)
    }

    pub fn rename(&mut self, id: &str, new_title: String) -> Result<(), String> {
        match find_mut(&mut self.items, id) {
            Some(Node::Link { title, .. } | Node::Folder { title, .. }) => {
                *title = new_title;
                Ok(())
            }
            None => Err(NOT_FOUND.into()),
        }
    }

    pub fn set_open(&mut self, id: &str, value: bool) -> Result<(), String> {
        match find_mut(&mut self.items, id) {
            Some(Node::Folder { open, .. }) => {
                *open = value;
                Ok(())
            }
            _ => Err(NOT_FOUND.into()),
        }
    }

    pub fn remove(&mut self, id: &str) -> Result<Node, String> {
        take(&mut self.items, id).ok_or_else(|| NOT_FOUND.to_string())
    }

    /// `id`를 `parent` 폴더의 `before` 앞으로 옮긴다. 폴더를 자기 안으로 옮길 수는 없다.
    pub fn move_node(&mut self, id: &str, parent: Option<&str>, before: Option<&str>) -> Result<(), String> {
        if before == Some(id) {
            return Ok(());
        }
        if let Some(pid) = parent {
            if pid == id || find(&self.items, id).is_some_and(|n| find(std::slice::from_ref(n), pid).is_some()) {
                return Err("폴더를 자기 안으로 옮길 수 없습니다.".into());
            }
        }
        let node = self.remove(id)?;
        let backup = node.clone();
        if let Err(e) = self.insert(parent, before, node) {
            // 옮길 곳이 없으면 맨 위 끝에 되돌려 놓아 북마크를 잃지 않는다
            self.items.push(backup);
            return Err(e);
        }
        Ok(())
    }

    /// 이 주소의 북마크 id (별 버튼이 채워진 별로 보이는지)
    pub fn find_url(&self, url: &str) -> Option<String> {
        fn walk(items: &[Node], url: &str) -> Option<String> {
            items.iter().find_map(|n| match n {
                Node::Link { id, url: u, .. } if u == url => Some(id.clone()),
                Node::Folder { items, .. } => walk(items, url),
                _ => None,
            })
        }
        walk(&self.items, url)
    }
}

fn find<'a>(items: &'a [Node], id: &str) -> Option<&'a Node> {
    for n in items {
        if n.id() == id {
            return Some(n);
        }
        if let Node::Folder { items, .. } = n {
            if let Some(f) = find(items, id) {
                return Some(f);
            }
        }
    }
    None
}

fn find_mut<'a>(items: &'a mut [Node], id: &str) -> Option<&'a mut Node> {
    for n in items.iter_mut() {
        if n.id() == id {
            return Some(n);
        }
        if let Node::Folder { items, .. } = n {
            if let Some(f) = find_mut(items, id) {
                return Some(f);
            }
        }
    }
    None
}

fn take(items: &mut Vec<Node>, id: &str) -> Option<Node> {
    if let Some(i) = items.iter().position(|n| n.id() == id) {
        return Some(items.remove(i));
    }
    items.iter_mut().find_map(|n| match n {
        Node::Folder { items, .. } => take(items, id),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(items: &[Node]) -> Vec<&str> {
        items.iter().map(|n| n.id()).collect()
    }

    #[test]
    fn file_format_matches_spec() {
        let json = r#"{ "version": 1, "collapsed": false, "items": [
            { "type": "link", "id": "b1", "title": "메일", "url": "https://mail.example.com/" },
            { "type": "folder", "id": "f1", "title": "업무 도구", "open": true, "items": [
              { "type": "link", "id": "b2", "title": "문서", "url": "https://docs.example.com/" } ] } ] }"#;
        let b: Bookmarks = serde_json::from_str(json).unwrap();
        assert_eq!(b.items.len(), 2);
        let back: Bookmarks = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!(back, b);
        assert_eq!(b.find_url("https://docs.example.com/").as_deref(), Some("b2"));
    }

    #[test]
    fn add_rename_remove() {
        let mut b = Bookmarks::default();
        let f = b.add_folder(None, None, "폴더".into()).unwrap();
        let l = b.add_link(Some(&f), None, "".into(), "https://a.example/".into(), None).unwrap();
        b.rename(&l, "A".into()).unwrap();
        assert_eq!(b.find_url("https://a.example/"), Some(l.clone()));
        b.remove(&f).unwrap();
        assert!(b.items.is_empty());
        assert!(b.find_url("https://a.example/").is_none());
    }

    #[test]
    fn ids_are_unique() {
        let mut b = Bookmarks::default();
        let a = b.add_link(None, None, "a".into(), "u1".into(), None).unwrap();
        let c = b.add_link(None, None, "c".into(), "u2".into(), None).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn insert_before_and_move() {
        let mut b = Bookmarks::default();
        let x = b.add_link(None, None, "x".into(), "ux".into(), None).unwrap();
        let y = b.add_link(None, Some(&x), "y".into(), "uy".into(), None).unwrap();
        assert_eq!(ids(&b.items), vec![y.as_str(), x.as_str()]);
        let f = b.add_folder(None, None, "f".into()).unwrap();
        b.move_node(&x, Some(&f), None).unwrap();
        assert_eq!(ids(&b.items), vec![y.as_str(), f.as_str()]);
        b.move_node(&x, None, Some(&y)).unwrap();
        assert_eq!(ids(&b.items), vec![x.as_str(), y.as_str(), f.as_str()]);
    }

    #[test]
    fn folder_cannot_go_inside_itself() {
        let mut b = Bookmarks::default();
        let outer = b.add_folder(None, None, "o".into()).unwrap();
        let inner = b.add_folder(Some(&outer), None, "i".into()).unwrap();
        assert!(b.move_node(&outer, Some(&inner), None).is_err());
        assert!(b.move_node(&outer, Some(&outer), None).is_err());
        assert_eq!(ids(&b.items), vec![outer.as_str()]);
    }
}
