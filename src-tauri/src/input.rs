//! 주소창 입력을 주소나 검색으로 바꾼다 (spec 5.2).

use tauri::Url;

use crate::settings::SearchEngine;

/// 내부 페이지 (UI 웹뷰가 카드 자리에 그린다, spec 3장)
pub const INTERNAL_PAGES: [&str; 5] = ["tbs://history", "tbs://downloads", "tbs://settings", "tbs://scripts", "tbs://extensions"];

pub fn is_internal_page(url: &str) -> bool {
    url.starts_with("tbs://")
}

/// 입력이 주소 모양이면 그 주소를, 아니면 검색 주소를 돌려준다. 빈 입력은 None.
///
/// 주소 모양: `http`로 시작하거나, 점이 있고 공백이 없을 때 (spec 5.2).
/// 그 밖에 `://`가 들어간 입력과 `localhost`도 주소로 본다.
pub fn resolve(input: &str, engine: SearchEngine) -> Option<Url> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if let Some(page) = INTERNAL_PAGES.iter().find(|p| lower.trim_end_matches('/') == **p) {
        return Url::parse(page).ok();
    }
    let no_space = !s.chars().any(char::is_whitespace);
    let looks_like_url = lower.starts_with("http")
        || (no_space && (s.contains('.') || s.contains("://") || is_localhost(&lower)));
    if looks_like_url {
        let candidate = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
        // localhost와 IP 주소는 보통 https가 없어서 http로 간다
        let candidate = if (is_localhost(&lower) || is_ip(&lower)) && !s.contains("://") { format!("http://{s}") } else { candidate };
        if let Ok(url) = Url::parse(&candidate) {
            if matches!(url.scheme(), "http" | "https" | "file") && url.host_str().map_or(url.scheme() == "file", |h| !h.is_empty()) {
                return Some(url);
            }
        }
    }
    let (base, param) = engine.endpoint();
    Url::parse_with_params(base, &[(param, s)]).ok()
}

/// 192.168.0.1, 127.0.0.1:8080/path 같은 IPv4 주소
fn is_ip(lower: &str) -> bool {
    let host = lower.split(['/', ':']).next().unwrap_or("");
    host.parse::<std::net::Ipv4Addr>().is_ok()
}

fn is_localhost(lower: &str) -> bool {
    lower == "localhost" || lower.starts_with("localhost:") || lower.starts_with("localhost/")
}

/// 콘텐츠 웹뷰가 가면 안 되는 앱 내부 주소인지 (spec 3장 보안 경계).
pub fn is_app_internal(url: &Url) -> bool {
    matches!(url.scheme(), "tauri" | "ipc" | "asset")
        || matches!(url.host_str(), Some("tauri.localhost" | "ipc.localhost" | "asset.localhost"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> String {
        resolve(s, SearchEngine::Google).map(|u| u.to_string()).unwrap_or_default()
    }

    #[test]
    fn other_engines_and_internal_pages() {
        assert_eq!(resolve("고양이", SearchEngine::Naver).unwrap().as_str(), "https://search.naver.com/search.naver?query=%EA%B3%A0%EC%96%91%EC%9D%B4");
        assert_eq!(r("tbs://history"), "tbs://history");
        assert_eq!(r("TBS://Settings/"), "tbs://settings");
        assert!(r("tbs://unknown").starts_with("https://www.google.com/search"));
    }

    #[test]
    fn domain_becomes_https() {
        assert_eq!(r("example.com"), "https://example.com/");
        assert_eq!(r("  naver.com/abc?x=1 "), "https://naver.com/abc?x=1");
    }

    #[test]
    fn full_urls_are_kept() {
        assert_eq!(r("http://example.com/a"), "http://example.com/a");
        assert_eq!(r("HTTPS://Example.com"), "https://example.com/");
    }

    #[test]
    fn localhost_uses_http() {
        assert_eq!(r("localhost:3000"), "http://localhost:3000/");
        assert_eq!(r("127.0.0.1:8766"), "http://127.0.0.1:8766/");
        assert_eq!(r("192.168.0.1/admin"), "http://192.168.0.1/admin");
    }

    #[test]
    fn words_become_search() {
        assert_eq!(r("러스트 타우리"), "https://www.google.com/search?q=%EB%9F%AC%EC%8A%A4%ED%8A%B8+%ED%83%80%EC%9A%B0%EB%A6%AC");
        assert_eq!(r("hello"), "https://www.google.com/search?q=hello");
        assert_eq!(r("node.js 설치 방법"), "https://www.google.com/search?q=node.js+%EC%84%A4%EC%B9%98+%EB%B0%A9%EB%B2%95");
    }

    #[test]
    fn scripts_are_not_urls() {
        assert!(r("javascript:alert(1.2)").starts_with("https://www.google.com/search?q="));
        assert!(r("data:text/html,a.b").starts_with("https://www.google.com/search?q="));
    }

    #[test]
    fn empty_is_none() {
        assert!(resolve("   ", SearchEngine::Google).is_none());
    }

    #[test]
    fn app_internal_urls() {
        assert!(is_app_internal(&Url::parse("http://tauri.localhost/index.html").unwrap()));
        assert!(is_app_internal(&Url::parse("http://ipc.localhost/x").unwrap()));
        assert!(!is_app_internal(&Url::parse("https://example.com/").unwrap()));
    }
}
