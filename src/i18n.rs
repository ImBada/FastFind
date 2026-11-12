//! Localization. Mirrors `ko.lproj/Localizable.strings` of SearchEverything; keys that the Swift app
//! never localized (e.g. "Start Indexing") stay English in both languages, exactly as before.

use objc2_foundation::NSLocale;
use std::sync::OnceLock;

static KOREAN: OnceLock<bool> = OnceLock::new();

/// The first of `ko`/`en` in the user's preferred languages decides, like bundle localization does.
pub fn is_korean() -> bool {
    *KOREAN.get_or_init(|| {
        for language in NSLocale::preferredLanguages().iter() {
            let language = language.to_string();
            if language.starts_with("ko") {
                return true;
            }
            if language.starts_with("en") {
                return false;
            }
        }
        false
    })
}

/// `"key".localize()` / SwiftUI `LocalizedStringKey` lookup.
pub fn tr(key: &'static str) -> &'static str {
    if !is_korean() {
        return key;
    }
    korean(key).unwrap_or(key)
}

fn korean(key: &str) -> Option<&'static str> {
    Some(match key {
        "Show all files in matching folders" => "찾은 폴더 내 모든 파일 표시",
        "Hide folder-info tags under file names" => "경로 정보 비표시",
        "Select a path" => "경로 선택",
        "Reverse" => "반전",
        "Search ready. " => "검색 대기 중 ",
        "Search complete. " => "검색 완료 ",
        "Searching Now... " => "검색 중... ",
        "No path" => "경로 선택되지 않음.",
        "A-Z" => "가나다순",
        "File-Folder [A-Z]" => "파일-폴더순 [가나다순 고정]",
        "File-Folder [Z-A]" => "파일-폴더순 [가나다순 반전 고정]",
        "Size [Folder bottom]" => "크기순 [폴더 맨아래 가나다순 고정]",
        "Please select a path first" => "탐색할 경로를 먼저 선택해주세요.",
        "Could not read this folder. Check its permissions." => "폴더를 읽을 수 없습니다. 접근 권한을 확인해주세요.",
        "FastFind {version} is available." => "FastFind {version} 버전이 나왔습니다.",
        "Update now?\nFastFind restarts with the new version.\n(You have {current}.)" => {
            "지금 업데이트하시겠습니까?\n새 버전으로 다시 시작됩니다.\n(현재 {current})"
        }
        "Update" => "업데이트",
        "Later" => "나중에",
        "The update could not be installed." => "업데이트를 설치하지 못했습니다.",
        "Select" => "선택",
        "Search" => "검색",
        "results." => "항목 발견",
        "Options" => "설정",
        "About" => "정보",
        "Replace with" => "바꾸기",
        "Find" => "찾기",
        "CHANGE" => "바꾸기",
        "Preview Changes" => "예정된 작업 리스트",
        "Cancel" => "취소",
        "Confirm" => "확인",
        "NONE" => "정렬 없음",
        "Open" => "열기",
        "Reveal in Finder" => "Finder에서 보기",
        "Copy Full Path" => "전체 경로 복사",
        "Copy Filename Only" => "파일 이름만 복사",
        "Show Info" => "정보 가져오기",
        "Move to Trash" => "휴지통으로 이동",
        "Original Filename" => "원본 파일명",
        "Are you sure you want to move the selected file(s) to the Trash?" => {
            "정말로 파일을 지우시겠습니까?\n되돌릴 수 없습니다."
        }
        "File Overwrite" => "파일 덮어쓰기",
        "Files already exists, are you sure you want to overwrite it?" => {
            "아래 파일이 이미 존재합니다. 덮어쓰시겠습니까?"
        }
        "Overwrite" => "덮어쓰기",
        "Copying..." => "복사 중...",
        "Convert Full-width characters to ascii" => "일본어 전각→반각문자 변환 검색",
        "Rename" => "이름 변경",
        "Deep folder search" => "깊은 폴더 검색",
        _ => return None,
    })
}

/// Korean titles for the standard menu bar `install_menus` builds (AppKit localizes these for a
/// SwiftUI app with a `ko.lproj`; NativeGPUI builds them in English).
pub fn menu_wording(title: &str) -> Option<String> {
    // English keeps AppKit's own titles ("About FastFind", …).
    if !is_korean() {
        return None;
    }
    let app = crate::APP_NAME;
    if title == format!("About {app}") {
        return Some(tr("About").to_string());
    }
    if let Some(rest) = title.strip_prefix("Hide ").filter(|rest| *rest == app) {
        return Some(format!("{rest} 가리기"));
    }
    if let Some(rest) = title.strip_prefix("Quit ").filter(|rest| *rest == app) {
        return Some(format!("{rest} 종료"));
    }
    if let Some(rest) = title.strip_suffix(" Help").filter(|rest| *rest == app) {
        return Some(format!("{rest} 도움말"));
    }
    Some(
        match title {
            "Services" => "서비스",
            "Hide Others" => "기타 가리기",
            "Show All" => "모두 보기",
            "File" => "파일",
            "New Tab" => "새로운 탭",
            "Close" => "닫기",
            "Edit" => "편집",
            "Undo" => "실행 취소",
            "Redo" => "실행 복귀",
            "Cut" => "오려두기",
            "Copy" => "복사하기",
            "Paste" => "붙여넣기",
            "Paste and Match Style" => "붙여넣고 스타일 일치시키기",
            "Delete" => "삭제",
            "Select All" => "전체 선택",
            "View" => "보기",
            "Show Toolbar" => "도구 막대 보기",
            "Customize Toolbar…" => "도구 막대 사용자화…",
            "Show Tab Bar" => "탭 막대 보기",
            "Show All Tabs" => "모든 탭 보기",
            "Show Sidebar" => "사이드바 보기",
            "Enter Full Screen" => "전체 화면 시작",
            "Window" => "윈도우",
            "Minimize" => "최소화",
            "Zoom" => "확대/축소",
            "Show Previous Tab" => "이전 탭 보기",
            "Show Next Tab" => "다음 탭 보기",
            "Move Tab to New Window" => "탭을 새로운 윈도우로 이동",
            "Merge All Windows" => "모든 윈도우 병합",
            "Bring All to Front" => "모두 앞으로 가져오기",
            "Help" => "도움말",
            _ => return None,
        }
        .to_string(),
    )
}
