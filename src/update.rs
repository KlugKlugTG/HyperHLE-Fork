/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! In-app updater.
//!
//! On startup, touchHLE/HyperHLE pings GitHub for the latest commit built on
//! the `trunk` branch and compares it against the commit this build was made
//! from (see [crate::VERSION], which is `git describe --always`, i.e. the short
//! commit hash for CI builds). Pull-request builds are excluded: those are built
//! from the `<pr number>/merge` ref (that's what produces the
//! `Built from branch "<pr number>/merge"` line in their logs), so by only
//! looking at successful `push` runs on `trunk` we never offer a PR build as an
//! "update".
//!
//! When a newer commit is found, the user is asked (via an SDL message box)
//! whether they want to update. If they accept, the latest build artifacts are
//! downloaded from nightly.link and extracted over the top of the current
//! installation.

use crate::VERSION;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Repository to check for updates, in `owner/repo` form. Locked to the
/// upstream HyperHLE repository regardless of where this build was produced.
const REPO: &str = "HyperHLE/HyperHLE";

/// The branch official builds are produced from.
const BRANCH: &str = "trunk";

/// The build workflow's file name, without the `.yml` extension. Used both for
/// the GitHub Actions API and for nightly.link artifact URLs.
const WORKFLOW: &str = "HyperHLE_release";

/// The build artifact for the platform we're running on, or [None] on a
/// platform that has no published build. Only the host platform's artifact is
/// runnable here, so that's the only one downloaded.
fn host_artifact() -> Option<&'static str> {
    match std::env::consts::OS {
        "android" => Some("HyperHLE_Android_AArch64"),
        "linux" => Some("HyperHLE_Linux_x86_64"),
        "windows" => Some("HyperHLE_Windows_x86_64"),
        "macos" => Some("HyperHLE_macOS_x86_64"),
        _ => None,
    }
}

/// User-Agent sent with HTTP requests. GitHub's API rejects requests without
/// one.
const USER_AGENT: &str = "HyperHLE-Updater";

/// The commit this build was made from, if it can be determined.
///
/// [VERSION] is the output of `git describe --always`, which is the short
/// commit hash for CI builds (e.g. `8186660`), or something like
/// `v0.2.3-5-g8186660` when annotated tags are reachable. Either way, the
/// trailing component is the abbreviated commit hash. Returns [None] for
/// non-CI builds where the version isn't a commit hash (e.g.
/// `v0.2.3 (git rev. unknown)`), in which case we can't tell whether an update
/// is available and silently skip the check.
fn local_commit() -> Option<String> {
    let version = VERSION.trim();
    // `v0.2.3-5-g8186660` -> `8186660`; bare `8186660` is unchanged.
    let candidate = version.rsplit("-g").next().unwrap_or(version);
    let candidate = candidate.trim_start_matches('v');
    if candidate.len() >= 7 && candidate.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(candidate.to_ascii_lowercase())
    } else {
        None
    }
}

/// Perform an HTTP GET and return the response body as bytes. Follows redirects
/// (nightly.link responds with a redirect to the actual artifact).
fn http_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Ask GitHub for the latest commit built on `trunk`, excluding pull-request
/// builds. Returns the full commit SHA of the most recent successful `push`
/// build, if any.
fn latest_trunk_commit() -> Result<Option<String>, String> {
    // Only successful `push` runs on `trunk` are considered. `pull_request`
    // runs (which are built from the `<pr number>/merge` ref) are excluded by
    // the `event=push` filter, so a PR build is never mistaken for an update.
    let url = format!(
        "https://api.github.com/repos/{}/actions/workflows/{}.yml/runs\
         ?branch={}&event=push&status=success&per_page=1",
        REPO, WORKFLOW, BRANCH,
    );
    let body = http_get_bytes(&url)?;
    let json: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    Ok(json["workflow_runs"][0]["head_sha"]
        .as_str()
        .map(|s| s.to_ascii_lowercase()))
}

/// The directory of the running touchHLE installation (the "main folder"),
/// into which updated files are extracted.
fn main_folder() -> PathBuf {
    // SDL2's base path is the directory containing the executable. This is the
    // root of the bundle on the desktop platforms where self-updating makes
    // sense.
    sdl2::filesystem::base_path()
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Download and extract a single nightly.link artifact `.zip` into `dest`,
/// overwriting any existing files.
fn download_and_extract_artifact(artifact: &str, dest: &Path) -> Result<(), String> {
    // Format: https://nightly.link/<owner>/<repo>/workflows/<workflow>/<branch>/<artifact>.zip
    let url = format!(
        "https://nightly.link/{}/workflows/{}/{}/{}.zip",
        REPO, WORKFLOW, BRANCH, artifact,
    );
    echo!("Downloading {}...", url);
    let bytes = http_get_bytes(&url)?;

    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // Use the sanitised path to avoid zip-slip writes outside `dest`.
        let Some(relative) = entry.enclosed_name() else {
            log!("Warning: skipping unsafe path in {} artifact", artifact);
            continue;
        };
        let out_path = dest.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // File::create truncates, so existing files are overwritten.
        let mut out = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    echo!("Extracted {} into {}", artifact, dest.display());
    Ok(())
}

/// Download this platform's build artifact from nightly.link and extract it
/// over the current installation.
fn perform_update() -> Result<(), String> {
    let artifact = host_artifact().ok_or_else(|| {
        format!(
            "no HyperHLE build is published for {}",
            std::env::consts::OS
        )
    })?;
    let dest = main_folder();
    echo!("Updating HyperHLE in {}...", dest.display());
    download_and_extract_artifact(artifact, &dest)
}

/// Ask the user, via an SDL message box, whether they want to update to
/// `new_commit`. Returns `true` if they chose to update.
fn prompt_user(new_commit: &str) -> bool {
    use sdl2::messagebox;
    let buttons = [
        messagebox::ButtonData {
            flags: messagebox::MessageBoxButtonFlag::RETURNKEY_DEFAULT,
            button_id: 1,
            text: "Update now",
        },
        messagebox::ButtonData {
            flags: messagebox::MessageBoxButtonFlag::ESCAPEKEY_DEFAULT,
            button_id: 0,
            text: "Later",
        },
    ];
    let short = &new_commit[..new_commit.len().min(7)];
    let message = format!(
        "A new version of HyperHLE is available (commit {short}).\n\
         You are currently running {VERSION}.\n\n\
         Would you like to download and install the update now?"
    );
    match messagebox::show_message_box(
        messagebox::MessageBoxFlag::INFORMATION,
        &buttons,
        "HyperHLE update available",
        &message,
        None::<&sdl2::video::Window>,
        None,
    ) {
        Ok(messagebox::ClickedButton::CustomButton(button)) => button.button_id == 1,
        // Closing the dialog or any error means "don't update".
        _ => false,
    }
}

/// Check for an update and, if one is available and the user accepts, perform
/// it. Best-effort: any failure (no network, rate limiting, etc.) is logged and
/// otherwise ignored so it never blocks startup.
pub fn check_for_update() {
    let Some(local) = local_commit() else {
        log_dbg!(
            "Skipping update check: build version {:?} is not a commit hash.",
            VERSION
        );
        return;
    };

    let latest = match latest_trunk_commit() {
        Ok(Some(latest)) => latest,
        Ok(None) => {
            log!("Update check: no successful trunk build found.");
            return;
        }
        Err(e) => {
            log!("Update check failed: {}", e);
            return;
        }
    };

    if latest.starts_with(&local) {
        log!("HyperHLE is up to date ({}).", local);
        return;
    }

    echo!(
        "A new HyperHLE version is available: {} (you have {}).",
        &latest[..latest.len().min(7)],
        local
    );

    if !prompt_user(&latest) {
        echo!("Update declined.");
        return;
    }

    match perform_update() {
        Ok(()) => {
            echo!("Update complete. Please restart HyperHLE to use the new version.");
            notify_result(
                true,
                "Update complete!\n\nPlease restart HyperHLE to use the new version.",
            );
        }
        Err(e) => {
            log!("Update failed: {}", e);
            notify_result(false, &format!("The update failed:\n\n{e}"));
        }
    }
}

/// Show an informational/error message box reporting the update result.
fn notify_result(success: bool, message: &str) {
    use sdl2::messagebox;
    let flag = if success {
        messagebox::MessageBoxFlag::INFORMATION
    } else {
        messagebox::MessageBoxFlag::ERROR
    };
    let buttons = [messagebox::ButtonData {
        flags: messagebox::MessageBoxButtonFlag::RETURNKEY_DEFAULT,
        button_id: 0,
        text: "OK",
    }];
    let _ = messagebox::show_message_box(
        flag,
        &buttons,
        "HyperHLE update",
        message,
        None::<&sdl2::video::Window>,
        None,
    );
}
