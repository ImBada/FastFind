# FastFind 개발 문서

빌드, 테스트, 릴리스, 업데이트 동작, 개발용 도구를 정리합니다. 앱 기능은 [README.md](README.md),
Swift 버전에서 옮긴 기록은 [MIGRATION.md](MIGRATION.md)에 있습니다.

## 요구 사항

- macOS 26 이상, Apple Silicon ([NativeGPUI](https://github.com/Cafekery/NativeGPUI) 요구 사항)
- Rust 1.97 이상 — `rust-toolchain.toml`이 1.99.1을 고정합니다(`rustup`이 자동으로 사용)
- Xcode (Metal 셰이더 컴파일)

## 빌드 / 실행

```sh
scripts/bundle.sh            # release → dist/FastFind.app
scripts/bundle.sh --debug    # debug 빌드
scripts/bundle.sh --dmg      # release + dist/FastFindLatest.dmg
scripts/bundle.sh --zip      # release + dist/FastFind.zip (업데이트가 설치하는 파일)
open dist/FastFind.app
```

`cargo run`으로 띄운 번들 없는 바이너리는 키 윈도우가 되지 못해 텍스트 입력이 동작하지 않습니다(NativeGPUI 제약).
항상 번들로 실행하세요.

- 테스트: `cargo test --workspace`
- 포맷: `cargo fmt` (`rustfmt.toml`, 120자)
- 린트: `cargo clippy --workspace --all-targets`

## 구조

- `crates/fastfind-core` — 플랫폼과 무관한 로직: 인덱싱, 검색과 좁히기, 정렬, 이름 규칙 파싱, 텍스트 정규화
- `src/` — 앱: GPUI 화면과 NativeGPUI로 얹은 AppKit 컨트롤, 파일 작업, 설정, 업데이트. 메인 화면은
  `src/main_view/`에 관심사별 파일로 나뉘어 있고(`mod.rs` 머리 주석 참고), 디버그 도구(`debug.rs`)는 debug 빌드에만
  들어갑니다.
- `scripts/` — 번들·릴리스·아이콘 스크립트

파일별 역할과 Swift 원본의 대응은 [MIGRATION.md](MIGRATION.md#구조)에 있습니다.

## 앱 아이콘

코드로 그린 벡터 그림입니다. `scripts/render_icon.swift`를 고친 뒤 `scripts/make_icon.sh`를 실행하면 크기마다 직접
그려 `assets/AppIcon.iconset`을 다시 만듭니다.

## 업데이트

앱은 이 저장소(`ImBada/FastFind`)의 GitHub 릴리스로 업데이트합니다(`src/update.rs`). 설치는 사용자가 수락할 때만
합니다.

1. 실행 10초 뒤, 그 뒤로 6시간마다 최신 릴리스를 확인합니다.
2. 이 빌드보다 새 버전이면 릴리스의 `FastFind.zip`을 받아 확인합니다: GitHub가 주는 SHA-256 다이제스트, 번들 ID와
   버전(태그와 같아야 함), 코드 서명, 그리고 이 빌드가 팀 인증서로 서명돼 있으면 같은 팀인지.
3. 앱 옆 숨김 폴더(`.FastFind-update`)에 풀어 두고, FastFind가 맨 앞 앱이고 파일 작업(드롭·이름 변경·휴지통)이나
   시트가 없을 때 "업데이트하시겠습니까?"를 묻습니다.
   - **업데이트**: 새 앱을 한 번에 바꿔 넣고(`renamex_np` 교체) 다시 시작합니다.
   - **나중에**: 받은 파일을 지우고, 이번 실행 동안은 다시 묻지 않습니다. 다음 실행 때 다시 확인합니다.

debug 빌드, 앱이 쓰기 불가능한 위치(디스크 이미지 안, App Translocation 경로)에 있을 때는 동작하지 않습니다. 끄려면
`defaults write BadaGames.FastFind autoUpdate -bool NO`. 저장소가 비공개인 동안에는 로그인 없는 확인이 거부되므로
업데이트가 일어나지 않습니다.

## 릴리스

1. `Cargo.toml`의 `version`을 올리고 커밋·푸시합니다.
2. `scripts/release.sh` — 빌드한 뒤 `v<버전>` 태그로 GitHub 릴리스를 만들고 `FastFind.zip`과 DMG를 올립니다
   (`gh` 필요). 덧붙인 인자는 `gh release create`에 넘어갑니다(예: `--notes "…"`).

## 서명

- `scripts/bundle.sh`는 기본적으로 ad-hoc 서명만 합니다. 공증받지 않은 앱이라 다른 Mac에서 처음 열 때 Gatekeeper가
  막습니다(우클릭 → 열기).
- ad-hoc 서명은 빌드마다 서명이 달라서, 업데이트 후 macOS가 폴더 접근·Finder 제어 권한을 다시 묻습니다. 고정된
  인증서(Developer ID, 또는 키체인 접근에서 만든 코드 서명 인증서)를 `FASTFIND_CODESIGN_IDENTITY`로 지정해 빌드하면
  권한이 유지됩니다. 매끄럽게 배포하려면 Developer ID로 서명하고 공증(notarization)을 받으세요.
- 팀 인증서로 서명한 뒤에는 같은 팀으로 서명한 릴리스만 업데이트로 설치됩니다. ad-hoc 서명일 때는 같은 팀인지
  확인할 수 없어서, 업데이트가 진짜인지는 GitHub 계정과 HTTPS, GitHub가 주는 SHA-256 다이제스트에 기댑니다.

## 개발용 환경 변수 (debug 빌드 전용)

```sh
FASTFIND_DEBUG_DIR=/경로 FASTFIND_DEBUG_AUTOINDEX=1 FASTFIND_DEBUG_QUERY="megami no 카페" \
  dist/FastFind.app/Contents/MacOS/FastFind
```

폴더를 지정하고 실행 즉시 인덱싱한 뒤 검색어를 넣습니다. UI 확인용입니다. `FASTFIND_DEBUG_OFFER=1.1.0`을 더하면
실행 직후 그 버전의 업데이트 확인 창을 띄웁니다(답해도 아무것도 설치하지 않음).

실제 마우스를 건드리지 않고 포인터 동작을 확인하려면 (호버, 스크롤 등):

```sh
open -g -n dist/FastFind.app --stderr /tmp/ff.log \
  --env FASTFIND_DEBUG_BACKGROUND=1 --env FASTFIND_DEBUG_DIR=/경로 --env FASTFIND_DEBUG_AUTOINDEX=1 \
  --env FASTFIND_DEBUG_QUERY=02 --env "FASTFIND_DEBUG_POINTER=300,130;scroll:-88;exit"
```

`FASTFIND_DEBUG_BACKGROUND=1`은 창을 띄우되 키보드 포커스를 가져오지 않습니다. `FASTFIND_DEBUG_POINTER`는 다음 단계를
1.5초 간격으로 넣고 앱을 종료합니다.

| 단계 | 동작 |
|---|---|
| `x,y` | 창 좌표로 포인터 이동 |
| `click:x,y` | 그 위치 클릭 |
| `scroll:dy` | 포인터 위치에서 휠 스크롤 |
| `thumb:0~1` | 스크롤 막대 손잡이를 트랙의 그 비율로 끌기 |
| `exit` | 포인터가 창 밖으로 나감 |
| `query:텍스트` | 검색어 입력 |
| `sort:번호` | 정렬 변경(저장하지 않음) |
| `rename:찾을=바꿀` | 지금 결과로 일괄 이름 변경 미리보기 시트 열기(확인을 누르지 않으면 아무것도 바뀌지 않음) |

단계마다 로그에 `debug pointer: …`가 찍히므로, 그 시점에 `screencapture -l<창 번호>`로 찍으면 됩니다.
