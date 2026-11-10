<#
  새 버전을 만들어 GitHub Releases에 올린다 (spec 5.7).

  쓰는 법 (PowerShell, 저장소 맨 위 폴더에서):
    .\tools\release.ps1 -Version 0.1.1 -Notes "바뀐 점 한 줄"
    .\tools\release.ps1 -Version 0.1.1 -NoUpload     # 설치 파일만 만들고 올리지 않는다

  하는 일:
    1. package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml의 버전을 바꾼다
    2. 업데이트 서명 키로 서명한 설치 파일(-setup.exe)과 서명(.sig)을 만든다
    3. latest.json(앱이 새 버전을 확인하는 파일)을 만든다
    4. GitHub에 v<버전> 릴리스를 만들고 설치 파일, 서명, latest.json을 올린다

  서명 키: %USERPROFILE%\.tauri\tbs-browser.key (저장소 밖. 잃어버리면 이미 설치된 앱에 업데이트를 보낼 수 없다)
#>
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [string]$Notes = "",
  [switch]$NoUpload
)
$ErrorActionPreference = "Stop"
$repo = "tbvwv123/TBs-Browser"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "버전은 0.1.2 같은 모양이어야 합니다: $Version" }
$key = Join-Path $env:USERPROFILE ".tauri\tbs-browser.key"
if (-not (Test-Path $key)) { throw "서명 키가 없습니다: $key. 키를 백업해 둔 곳에서 이 자리로 복사해 주세요." }

function Set-Version([string]$path, [string]$pattern, [string]$replacement) {
  $text = [IO.File]::ReadAllText($path)
  $new = [regex]::Replace($text, $pattern, $replacement, [Text.RegularExpressions.RegexOptions]::Multiline)
  if ($new -eq $text -and $text -notmatch [regex]::Escape($Version)) { throw "버전을 바꾸지 못했습니다: $path" }
  [IO.File]::WriteAllText($path, $new, (New-Object Text.UTF8Encoding $false))
}
Set-Version "package.json" '^(  "version": )"[^"]+"' "`$1`"$Version`""
Set-Version "src-tauri\tauri.conf.json" '^(  "version": )"[^"]+"' "`$1`"$Version`""
Set-Version "src-tauri\Cargo.toml" '^(version = )"[^"]+"' "`$1`"$Version`""

$env:TAURI_SIGNING_PRIVATE_KEY = $key
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
# PowerShell 5.1은 외부 프로그램이 stderr에 쓴 평범한 진행 메시지도 오류로 바꾼다. cmd 안에서 합쳐 넘긴다.
cmd /c "npx tauri build 2>&1"
if ($LASTEXITCODE -ne 0) { throw "빌드에 실패했습니다. 위의 오류를 확인해 주세요." }

$bundle = "src-tauri\target\release\bundle\nsis"
$built = Get-ChildItem $bundle -Filter "*_${Version}_x64-setup.exe" | Select-Object -First 1
if (-not $built) { throw "설치 파일을 찾지 못했습니다: $bundle" }
$sig = "$($built.FullName).sig"
if (-not (Test-Path $sig)) { throw "서명 파일이 없습니다: $sig. tauri.conf.json의 createUpdaterArtifacts를 확인해 주세요." }

# GitHub는 파일 이름의 빈칸을 바꿔 버리므로 빈칸 없는 이름으로 올린다
$out = Join-Path $root "release\v$Version"
New-Item -ItemType Directory -Force $out | Out-Null
$setupName = "TBs-Browser_${Version}_x64-setup.exe"
Copy-Item $built.FullName (Join-Path $out $setupName) -Force
Copy-Item $sig (Join-Path $out "$setupName.sig") -Force

$latest = [ordered]@{
  version   = $Version
  notes     = $Notes
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = ([IO.File]::ReadAllText($sig)).Trim()
      url       = "https://github.com/$repo/releases/download/v$Version/$setupName"
    }
  }
}
[IO.File]::WriteAllText((Join-Path $out "latest.json"), ($latest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding $false))
Write-Host "만든 파일: $out"

if ($NoUpload) { return }
$title = "TB's Browser $Version"
$body = if ($Notes) { $Notes } else { "TB's Browser $Version" }
$ErrorActionPreference = "Continue"
gh release create "v$Version" (Join-Path $out $setupName) (Join-Path $out "$setupName.sig") (Join-Path $out "latest.json") --repo $repo --title $title --notes $body
if ($LASTEXITCODE -ne 0) { throw "GitHub에 올리지 못했습니다. gh auth status로 로그인 상태를 확인해 주세요." }
Write-Host "올림: https://github.com/$repo/releases/tag/v$Version"
