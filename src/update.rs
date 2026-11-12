//! Updates from this project's GitHub releases (`ImBada/FastFind`), installed only when the user
//! says so.
//!
//! A release is a `vX.Y.Z` tag carrying `FastFind.zip`, the app zipped by `scripts/release.sh`.
//! Ten seconds after launch, then every six hours, the latest release is asked for. When it is newer
//! than this build, its zip is downloaded and checked: the SHA-256 digest GitHub gives for the asset,
//! the bundle identifier and the version inside, a valid code signature, and, when this build is
//! signed by a team, the same team. It is unpacked into a hidden folder beside the running app, and
//! once FastFind is the active app it asks whether to update. Update swaps the new app in and
//! relaunches; Later drops the download and asks again at the next launch.
//!
//! Off in debug builds, where the app cannot replace itself (a disk image, an App Translocation
//! path), and after `defaults write BadaGames.FastFind autoUpdate -bool NO`.

use crate::file_ops;
use crate::i18n::tr;
use crate::platform::{self, ns_string};
use crate::settings;
use block2::RcBlock;
use futures::channel::oneshot;
use gpui::{App, AsyncApp};
use objc2_foundation::{
    NSBundle, NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSURLResponse, NSURLSession, NSURL,
};
use std::cmp::Ordering;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

pub const REPO: &str = "ImBada/FastFind";
/// The release asset the app updates itself from.
pub const ASSET: &str = "FastFind.zip";
const FIRST_CHECK: Duration = Duration::from_secs(10);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// How often a ready update looks again for the user to come back to FastFind.
const ASK_POLL: Duration = Duration::from_secs(2);
/// Beside the app (same volume, so the swap is a rename): the new version waiting for an answer.
const STAGING: &str = ".FastFind-update";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    /// Lowercase hex SHA-256 of the asset, as GitHub reports it.
    pub sha256: Option<String>,
}

/// `String.compare(_:options: .numeric)` for dotted versions.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let mut ia = a.split('.');
    let mut ib = b.split('.');
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return Ordering::Equal,
            (x, y) => {
                let (x, y) = (x.unwrap_or("0"), y.unwrap_or("0"));
                let o = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    _ => x.cmp(y),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

/// The release in a `releases/latest` response, if it carries the app.
pub fn parse_release(json: &[u8]) -> Option<Release> {
    let json: serde_json::Value = serde_json::from_slice(json).ok()?;
    let version = json
        .get("tag_name")?
        .as_str()?
        .trim_start_matches(['v', 'V'])
        .to_string();
    let asset = json
        .get("assets")?
        .as_array()?
        .iter()
        .find(|asset| asset.get("name").and_then(|n| n.as_str()) == Some(ASSET))?;
    let url = asset.get("browser_download_url")?.as_str()?.to_string();
    let sha256 = asset
        .get("digest")
        .and_then(|d| d.as_str())
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(str::to_ascii_lowercase);
    Some(Release { version, url, sha256 })
}

/// Starts checking for updates (see the module docs).
pub fn start(cx: &mut App) {
    if cfg!(debug_assertions) || !settings::auto_update() {
        return;
    }
    let Some(app) = replaceable_bundle() else { return };
    // A download a previous run left unanswered.
    let _ = std::fs::remove_dir_all(staging_dir(&app));

    cx.spawn(async move |cx| {
        let mut wait = FIRST_CHECK;
        // The version turned down in this run: not offered again until the next launch.
        let mut declined: Option<String> = None;
        loop {
            cx.background_executor().timer(wait).await;
            wait = CHECK_EVERY;
            let (version, new_app) = match check_and_stage(&app, declined.as_deref(), cx).await {
                Ok(Some(ready)) => ready,
                Ok(None) => continue,
                Err(e) => {
                    eprintln!("Update check failed: {e}");
                    continue;
                }
            };
            // Asked when the user is here, not over whatever app they are in, and not while a file
            // operation runs or a sheet is up (the app could not quit to restart).
            while !platform::app_is_active() || file_ops::busy() || platform::sheet_open() {
                cx.background_executor().timer(ASK_POLL).await;
            }
            if !offer(&version) {
                declined = Some(version);
                let _ = std::fs::remove_dir_all(staging_dir(&app));
                continue;
            }
            // One that was still finishing in the background ends before the app quits.
            while file_ops::busy() {
                cx.background_executor().timer(Duration::from_millis(200)).await;
            }
            match install(&new_app, &app) {
                Ok(()) => {
                    relaunch(&app);
                    cx.update(|cx| cx.quit());
                    return;
                }
                Err(e) => {
                    eprintln!("Update not installed: {e}");
                    platform::inform(tr("The update could not be installed."), &e);
                }
            }
        }
    })
    .detach();
}

/// "FastFind 1.1.0 is available. Update now?" — true for Update.
pub(crate) fn offer(version: &str) -> bool {
    let message = tr("FastFind {version} is available.").replace("{version}", version);
    // One sentence per line: AppKit wraps Korean anywhere, even inside a word.
    let detail = tr("Update now?\nFastFind restarts with the new version.\n(You have {current}.)")
        .replace("{current}", &platform::app_version());
    platform::ask(&message, &detail, tr("Update"), tr("Later"))
}

/// Opens `app` again once this process has quit (it now holds the new version).
fn relaunch(app: &Path) {
    let script = format!(
        "while /bin/kill -0 {} 2>/dev/null; do /bin/sleep 0.2; done; /usr/bin/open \"$0\"",
        std::process::id()
    );
    if let Err(e) = Command::new("/bin/sh").arg("-c").arg(script).arg(app).spawn() {
        eprintln!("Could not relaunch: {e}");
    }
}

/// The newer release (unless it is `declined`), downloaded, checked and unpacked beside `app`: its
/// version and the new app. `None` when there is none.
async fn check_and_stage(
    app: &Path,
    declined: Option<&str>,
    cx: &AsyncApp,
) -> Result<Option<(String, PathBuf)>, String> {
    let latest = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = match fetch(&latest, "application/vnd.github+json", 15.0).await {
        Ok(Ok(body)) => body,
        // No release yet (or the repository is private).
        Ok(Err(Fetch::Status(404))) => return Ok(None),
        Ok(Err(e)) => return Err(e.to_string()),
        Err(_) => return Err("request dropped".into()),
    };
    let Some(release) = parse_release(&body) else {
        return Ok(None);
    };
    if compare_versions(&release.version, &platform::app_version()) != Ordering::Greater
        || declined == Some(release.version.as_str())
    {
        return Ok(None);
    }
    let zip = match fetch(&release.url, "application/octet-stream", 120.0).await {
        Ok(Ok(zip)) => zip,
        Ok(Err(e)) => return Err(e.to_string()),
        Err(_) => return Err("download dropped".into()),
    };
    let app = app.to_path_buf();
    let version = release.version.clone();
    let new_app = cx
        .background_executor()
        .spawn(async move { stage(&zip, &release, &app) })
        .await?;
    Ok(Some((version, new_app)))
}

#[derive(Debug)]
enum Fetch {
    Failed(String),
    Status(isize),
}

impl std::fmt::Display for Fetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fetch::Failed(e) => f.write_str(e),
            Fetch::Status(code) => write!(f, "HTTP {code}"),
        }
    }
}

/// An HTTPS GET; the body of a 200 response.
fn fetch(url: &str, accept: &str, timeout: f64) -> oneshot::Receiver<Result<Vec<u8>, Fetch>> {
    let (tx, rx) = oneshot::channel();
    let Some(url) = NSURL::URLWithString(&ns_string(url)) else {
        let _ = tx.send(Err(Fetch::Failed("bad URL".into())));
        return rx;
    };
    let request = NSMutableURLRequest::requestWithURL(&url);
    request.setValue_forHTTPHeaderField(Some(&ns_string(accept)), &ns_string("Accept"));
    request.setTimeoutInterval(timeout);
    let tx = Mutex::new(Some(tx));
    let handler = RcBlock::new(
        move |data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError| {
            let result = if let Some(error) = unsafe { error.as_ref() } {
                Err(Fetch::Failed(error.localizedDescription().to_string()))
            } else {
                let status = unsafe { response.as_ref() }
                    .and_then(|r| r.downcast_ref::<NSHTTPURLResponse>())
                    .map_or(0, |r| r.statusCode());
                if status == 200 {
                    Ok(unsafe { data.as_ref() }.map(|d| d.to_vec()).unwrap_or_default())
                } else {
                    Err(Fetch::Status(status))
                }
            };
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(result);
            }
        },
    );
    let task = unsafe { NSURLSession::sharedSession().dataTaskWithRequest_completionHandler(&request, &handler) };
    task.resume();
    rx
}

/// The running app, when it is a bundle in a folder this user may write.
fn replaceable_bundle() -> Option<PathBuf> {
    let app = PathBuf::from(NSBundle::mainBundle().bundlePath().to_string());
    if app.extension()? != "app" || app.to_string_lossy().contains("/AppTranslocation/") {
        return None;
    }
    let parent = CString::new(app.parent()?.as_os_str().as_bytes()).ok()?;
    (unsafe { libc::access(parent.as_ptr(), libc::W_OK) } == 0).then_some(app)
}

fn staging_dir(app: &Path) -> PathBuf {
    app.with_file_name(STAGING)
}

/// Checks a downloaded release (see the module docs) and unpacks it beside `app`. Returns the new
/// app, ready for [`install`].
pub fn stage(zip: &[u8], release: &Release, app: &Path) -> Result<PathBuf, String> {
    if let Some(expected) = &release.sha256 {
        let actual = sha256_hex(zip)?;
        if &actual != expected {
            return Err(format!("{ASSET} does not match its digest"));
        }
    }
    let staging = staging_dir(app);
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let archive = staging.join(ASSET);
    std::fs::write(&archive, zip).map_err(|e| e.to_string())?;
    let unpacked = staging.join("unpacked");
    let unzipped = run(
        "/usr/bin/ditto",
        &["-x".as_ref(), "-k".as_ref(), archive.as_os_str(), unpacked.as_os_str()],
    );
    let _ = std::fs::remove_file(&archive);
    if !unzipped {
        return Err(format!("could not unpack {ASSET}"));
    }
    let new_app = std::fs::read_dir(&unpacked)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|e| e == "app"))
        .ok_or_else(|| format!("no app in {ASSET}"))?;

    let id = |app: &Path| plist_value(app, "CFBundleIdentifier");
    if id(&new_app).is_none() || id(&new_app) != id(app) {
        return Err("the release is a different app".into());
    }
    if plist_value(&new_app, "CFBundleShortVersionString").as_deref() != Some(release.version.as_str()) {
        return Err(format!("the release does not hold version {}", release.version));
    }
    let signed = run(
        "/usr/bin/codesign",
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            new_app.as_os_str(),
        ],
    );
    if !signed {
        return Err("the release's code signature is not valid".into());
    }
    if let Some(team) = team_of(app) {
        if team_of(&new_app).as_deref() != Some(team.as_str()) {
            return Err("the release is signed by a different team".into());
        }
    }
    Ok(new_app)
}

/// Puts the staged app in `app`'s place in one step, then removes the old one.
pub fn install(staged: &Path, app: &Path) -> Result<(), String> {
    let path = |p: &Path| CString::new(p.as_os_str().as_bytes()).map_err(|e| e.to_string());
    let (new, old) = (path(staged)?, path(app)?);
    if unsafe { libc::renamex_np(new.as_ptr(), old.as_ptr(), libc::RENAME_SWAP) } != 0 {
        let swap_error = std::io::Error::last_os_error();
        // Volumes without an atomic swap: move the old app aside, then the new one in.
        let aside = staging_dir(app).join("previous.app");
        std::fs::rename(app, &aside).map_err(|_| swap_error.to_string())?;
        if let Err(e) = std::fs::rename(staged, app) {
            let _ = std::fs::rename(&aside, app);
            return Err(e.to_string());
        }
    }
    // The old app is now inside the staging folder.
    let _ = std::fs::remove_dir_all(staging_dir(app));
    Ok(())
}

fn run(tool: &str, args: &[&std::ffi::OsStr]) -> bool {
    Command::new(tool)
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

fn plist_value(app: &Path, key: &str) -> Option<String> {
    let out = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The signing team, or `None` for ad-hoc or unsigned code.
fn team_of(app: &Path) -> Option<String> {
    let out = Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=2"])
        .arg(app)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| *team != "not set")
        .map(str::to_string)
}

fn sha256_hex(bytes: &[u8]) -> Result<String, String> {
    extern "C" {
        // CommonCrypto, part of libSystem.
        fn CC_SHA256(data: *const std::ffi::c_void, len: u32, md: *mut u8) -> *mut u8;
    }
    let len = u32::try_from(bytes.len()).map_err(|_| format!("{ASSET} is too large"))?;
    let mut digest = [0u8; 32];
    unsafe { CC_SHA256(bytes.as_ptr().cast(), len, digest.as_mut_ptr()) };
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn numeric_compare() {
        assert_eq!(compare_versions("4.0.10", "4.0.9"), Ordering::Greater);
        assert_eq!(compare_versions("4.1", "4.1.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.0.0", "1.0.1"), Ordering::Less);
    }

    #[test]
    fn reads_the_latest_release() {
        let json = br#"{"tag_name":"v1.2.0","assets":[
            {"name":"FastFindLatest.dmg","browser_download_url":"https://example.com/a.dmg"},
            {"name":"FastFind.zip","browser_download_url":"https://example.com/FastFind.zip",
             "digest":"sha256:ABCDEF"}]}"#;
        let release = parse_release(json).unwrap();
        assert_eq!(release.version, "1.2.0");
        assert_eq!(release.url, "https://example.com/FastFind.zip");
        assert_eq!(release.sha256.as_deref(), Some("abcdef"));
        assert!(parse_release(br#"{"tag_name":"v1.2.0","assets":[]}"#).is_none());
    }

    #[test]
    fn sha256_matches_known_digest() {
        assert_eq!(
            sha256_hex(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// A minimal signed app bundle, as `scripts/bundle.sh` makes (ad-hoc signed).
    fn make_app(dir: &Path, version: &str) -> PathBuf {
        let app = dir.join("FastFind.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::copy("/usr/bin/true", app.join("Contents/MacOS/FastFind")).unwrap();
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>BadaGames.FastFind</string>
<key>CFBundleExecutable</key><string>FastFind</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
</dict></plist>"#
        );
        fs::write(app.join("Contents/Info.plist"), plist).unwrap();
        assert!(run(
            "/usr/bin/codesign",
            &["--force".as_ref(), "--sign".as_ref(), "-".as_ref(), app.as_os_str()]
        ));
        app
    }

    #[test]
    fn a_release_is_checked_staged_and_swapped_in() {
        let tmp = tempfile::tempdir().unwrap();
        let installed = make_app(&tmp.path().join("Applications"), "1.0.0");
        let build = make_app(&tmp.path().join("build"), "1.0.1");
        let zip = tmp.path().join(ASSET);
        assert!(run(
            "/usr/bin/ditto",
            &[
                "-c".as_ref(),
                "-k".as_ref(),
                "--keepParent".as_ref(),
                build.as_os_str(),
                zip.as_os_str()
            ]
        ));
        let bytes = fs::read(&zip).unwrap();
        let release = Release {
            version: "1.0.1".into(),
            url: String::new(),
            sha256: Some(sha256_hex(&bytes).unwrap()),
        };

        // Refused: a corrupted download, or a release whose app is not the tagged version.
        let corrupt = Release {
            sha256: Some("00".repeat(32)),
            ..release.clone()
        };
        assert!(stage(&bytes, &corrupt, &installed).is_err());
        let mislabeled = Release {
            version: "1.0.2".into(),
            ..release.clone()
        };
        assert!(stage(&bytes, &mislabeled, &installed).is_err());

        let staged = stage(&bytes, &release, &installed).unwrap();
        assert_eq!(
            plist_value(&installed, "CFBundleShortVersionString").as_deref(),
            Some("1.0.0")
        );
        install(&staged, &installed).unwrap();
        assert_eq!(
            plist_value(&installed, "CFBundleShortVersionString").as_deref(),
            Some("1.0.1")
        );
        assert!(run(
            "/usr/bin/codesign",
            &["--verify".as_ref(), "--strict".as_ref(), installed.as_os_str()]
        ));
        assert!(
            !staging_dir(&installed).exists(),
            "the old app and the staging folder are gone"
        );
    }
}
