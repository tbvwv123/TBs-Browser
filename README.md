# TB's Browser

세로 사이드바에 탭과 북마크를 두는 Windows용 개인 브라우저입니다. 렌더링은 Microsoft Edge WebView2가 하고, 창과 탭 관리는 Rust(Tauri 2), 화면은 Svelte 5로 만들었습니다.

- 탭과 북마크를 한 사이드바에서 같이 스크롤, 탭 그룹, 탭 검색, 접힌 사이드바
- 안 보는 탭을 재우고 버려 메모리를 아끼는 정책
- 확장 프로그램 대신 사이트별 스크립트·스타일 (`scripts-examples/`)
- 라이트·다크 테마, 자동 업데이트

설계와 단계별 기록은 `docs/spec.md`에 있습니다.

## 설치

[Releases](https://github.com/tbvwv123/TBs-Browser/releases/latest)에서 `TBs-Browser_<버전>_x64-setup.exe`를 받아 실행합니다.

- 코드 서명 인증서가 없어서 처음 실행할 때 Windows SmartScreen이 "Windows의 PC 보호" 창을 띄울 수 있습니다. "추가 정보" → "실행"을 누르면 설치됩니다.
- WebView2 런타임이 없는 PC에서는 설치 파일이 내려받아 설치합니다(인터넷 필요).
- 새 버전이 나오면 사이드바 맨 아래 점 3개 버튼에 작은 점이 생깁니다. 메뉴의 "업데이트 설치"를 누르면 설치하고 다시 시작합니다.
- 데이터(탭, 북마크, 설정, 방문 기록, 로그인 정보)는 `%LOCALAPPDATA%\TBs Browser\`에 있습니다. 문제가 생기면 그 안의 `logs\tbs.log`를 보내 주세요.

## 개발

```bash
npm install
npm run app          # UI를 만들고 개발용으로 실행
npm run check        # UI 형식 검사
npm run test:host    # 호스트(Rust) 시험
```

새 버전 올리기: `.\tools\release.ps1 -Version 0.1.2 -Notes "바뀐 점"` (업데이트 서명 키가 `%USERPROFILE%\.tauri\tbs-browser.key`에 있어야 합니다).
