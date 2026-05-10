//! Application launcher service — scans `.desktop` files from XDG directories.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, AppContext, Entity};
use inotify::{Inotify, WatchMask};

#[derive(Debug, Clone)]
pub struct AppEntry {
    pub name: String,
    pub exec: String,
    pub icon: String,
    pub comment: String,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ApplicationsState {
    pub apps: Vec<AppEntry>,
}

/// Returns XDG application directories that exist on disk.
///
/// Reads `$XDG_DATA_DIRS` (defaults to `/usr/local/share:/usr/share` per spec),
/// appends `/applications` to each, then adds `$HOME/.local/share/applications/`.
/// User-local directory is last so it overrides system entries during dedup.
pub fn xdg_application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());

    for dir in data_dirs.split(':') {
        let dir = dir.trim();
        if !dir.is_empty() {
            let p = PathBuf::from(dir).join("applications");
            if p.is_dir() {
                dirs.push(p);
            }
        }
    }

    let user_data = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".local/share"))
        });

    if let Some(base) = user_data {
        let local = base.join("applications");
        if local.is_dir() {
            dirs.push(local);
        }
    }

    dirs
}

/// Parse a single `.desktop` file into an `AppEntry`.
///
/// Returns `None` if the entry is malformed, hidden, or not of type `Application`.
pub fn parse_desktop_entry(path: &Path) -> Option<AppEntry> {
    let content = std::fs::read_to_string(path).ok()?;

    let mut in_desktop_entry = false;
    let mut name = None;
    let mut exec = None;
    let mut icon = String::new();
    let mut comment = String::new();
    let mut keywords = Vec::new();
    let mut categories = Vec::new();
    let mut entry_type = None;
    let mut no_display = false;
    let mut hidden = false;

    for line in content.lines() {
        let line = line.trim();

        if line == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        }

        if line.starts_with('[') {
            if in_desktop_entry {
                // We've left the [Desktop Entry] section
                break;
            }
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "Name" => name = Some(value.to_string()),
                "Exec" => exec = Some(value.to_string()),
                "Icon" => icon = value.to_string(),
                "Comment" => comment = value.to_string(),
                "Type" => entry_type = Some(value.to_string()),
                "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
                "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
                "Keywords" => {
                    keywords = value
                        .split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                "Categories" => {
                    categories = value
                        .split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                _ => {}
            }
        }
    }

    // Filter conditions
    let name = name.filter(|n| !n.is_empty())?;
    let exec = exec.filter(|e| !e.is_empty())?;

    if entry_type.as_deref() != Some("Application") {
        return None;
    }

    if no_display || hidden {
        return None;
    }

    Some(AppEntry {
        name,
        exec,
        icon,
        comment,
        keywords,
        categories,
    })
}

/// Scan all XDG application directories and return the directories scanned
/// alongside a deduplicated, sorted list of app entries.
///
/// Deduplication is by desktop file basename (e.g. `firefox.desktop`); later
/// directories override earlier ones so user-local entries take precedence.
pub fn scan_desktop_dirs() -> (Vec<PathBuf>, Vec<AppEntry>) {
    let dirs = xdg_application_dirs();
    let mut map: HashMap<String, AppEntry> = HashMap::new();

    for dir in &dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            if !file_name.ends_with(".desktop") {
                continue;
            }

            if let Some(app) = parse_desktop_entry(&path) {
                map.insert(file_name.to_string(), app);
            }
        }
    }

    let mut apps: Vec<AppEntry> = map.into_values().collect();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    (dirs, apps)
}

/// Filter apps by case-insensitive substring match on name, comment, and keywords.
/// Results are sorted: name matches first, then comment/keyword matches.
pub fn search_apps(apps: &[AppEntry], query: &str) -> Vec<AppEntry> {
    let q = query.to_lowercase();
    let mut results: Vec<(usize, &AppEntry)> = apps
        .iter()
        .filter_map(|app| {
            if app.name.to_lowercase().contains(&q) {
                Some((0, app)) // name match = highest priority
            } else if app.comment.to_lowercase().contains(&q) {
                Some((1, app))
            } else if app.keywords.iter().any(|k| k.to_lowercase().contains(&q)) {
                Some((2, app))
            } else {
                None
            }
        })
        .collect();
    results.sort_by_key(|(priority, _)| *priority);
    results.into_iter().map(|(_, app)| app.clone()).collect()
}

/// Strip freedesktop Exec field codes (%f, %F, %u, %U, etc.) from a command string.
/// Per the spec, `%%` is an escape for a literal `%`.
pub fn strip_field_codes(exec: &str) -> String {
    let mut result = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            if let Some(&next) = chars.peek() {
                if next == '%' {
                    // %% -> literal %
                    chars.next();
                    result.push('%');
                    continue;
                } else if "fFuUdDnNickvm".contains(next) {
                    // Strip known field codes
                    chars.next();
                    continue;
                }
            }
        }
        result.push(c);
    }
    // Collapse multiple spaces left behind by stripped codes
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub struct ApplicationsService;

impl ApplicationsService {
    /// Start the service. Returns a GPUI entity with the application list.
    ///
    /// The initial scan runs on the background executor. An OS thread with
    /// inotify watches the application directories for changes and triggers
    /// automatic re-scans via the shared slot pattern.
    pub fn start(cx: &mut App) -> Entity<ApplicationsState> {
        let entity = cx.new(|_| ApplicationsState::default());
        let weak = entity.downgrade();

        // Initial scan on background executor
        cx.spawn(async move |cx| {
            let (dirs, apps) = cx
                .background_executor()
                .spawn(async { scan_desktop_dirs() })
                .await;
            cx.update(|cx| {
                if let Some(e) = weak.upgrade() {
                    e.update(cx, |state, cx| {
                        state.apps = apps;
                        cx.notify();
                    });
                }
            });

            // Set up inotify watching after initial scan completes
            let slot: Arc<Mutex<Option<ApplicationsState>>> = Arc::new(Mutex::new(None));
            let slot_writer = Arc::clone(&slot);

            // OS thread for blocking inotify reads (Inotify is !Send)
            std::thread::spawn(move || {
                let Ok(mut inotify) = Inotify::init() else {
                    return;
                };
                for dir in &dirs {
                    let _ = inotify.watches().add(
                        dir,
                        WatchMask::CREATE
                            | WatchMask::DELETE
                            | WatchMask::MODIFY
                            | WatchMask::MOVED_TO
                            | WatchMask::MOVED_FROM,
                    );
                }
                let mut buffer = [0u8; 4096];
                loop {
                    match inotify.read_events_blocking(&mut buffer) {
                        Ok(_events) => {
                            // Debounce: sleep briefly so burst writes settle
                            std::thread::sleep(Duration::from_millis(500));
                            // Full rescan
                            let (_dirs, apps) = scan_desktop_dirs();
                            let new_state = ApplicationsState { apps };
                            *slot_writer.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(new_state);
                        }
                        Err(_) => break,
                    }
                }
            });

            // GPUI task drains the slot and updates the entity
            let weak2 = weak;
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    let new_state = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
                    if let Some(state) = new_state {
                        cx.update(|cx| {
                            if let Some(e) = weak2.upgrade() {
                                e.update(cx, |s, cx| {
                                    *s = state;
                                    cx.notify();
                                });
                            }
                        });
                    }
                }
            })
            .detach();
        })
        .detach();

        entity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_desktop_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn parse_valid_desktop_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_desktop_file(
            dir.path(),
            "test.desktop",
            "[Desktop Entry]\nType=Application\nName=Test App\nExec=test-app\nIcon=test-icon\nComment=A test\n",
        );
        let entry = parse_desktop_entry(&path).unwrap();
        assert_eq!(entry.name, "Test App");
        assert_eq!(entry.exec, "test-app");
        assert_eq!(entry.icon, "test-icon");
        assert_eq!(entry.comment, "A test");
    }

    #[test]
    fn parse_missing_name_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_desktop_file(
            dir.path(),
            "noname.desktop",
            "[Desktop Entry]\nType=Application\nExec=something\n",
        );
        assert!(parse_desktop_entry(&path).is_none());
    }

    #[test]
    fn parse_missing_exec_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_desktop_file(
            dir.path(),
            "noexec.desktop",
            "[Desktop Entry]\nType=Application\nName=NoExec\n",
        );
        assert!(parse_desktop_entry(&path).is_none());
    }

    #[test]
    fn parse_no_display_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_desktop_file(
            dir.path(),
            "nodisplay.desktop",
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=hidden\nNoDisplay=true\n",
        );
        assert!(parse_desktop_entry(&path).is_none());
    }

    #[test]
    fn parse_hidden_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_desktop_file(
            dir.path(),
            "hidden.desktop",
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=hidden\nHidden=true\n",
        );
        assert!(parse_desktop_entry(&path).is_none());
    }

    #[test]
    fn parse_type_link_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_desktop_file(
            dir.path(),
            "link.desktop",
            "[Desktop Entry]\nType=Link\nName=A Link\nExec=link\n",
        );
        assert!(parse_desktop_entry(&path).is_none());
    }

    #[test]
    fn parse_keywords_and_categories() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_desktop_file(
            dir.path(),
            "kw.desktop",
            "[Desktop Entry]\nType=Application\nName=KW\nExec=kw\nKeywords=foo;bar;baz;\nCategories=Utility;Editor;\n",
        );
        let entry = parse_desktop_entry(&path).unwrap();
        assert_eq!(entry.keywords, vec!["foo", "bar", "baz"]);
        assert_eq!(entry.categories, vec!["Utility", "Editor"]);
    }

    #[test]
    fn xdg_dirs_includes_controlled_dir() {
        let dir = tempfile::tempdir().unwrap();
        let apps_dir = dir.path().join("applications");
        std::fs::create_dir(&apps_dir).unwrap();
        // Note: env var tests can be flaky in parallel; this is acceptable for unit tests
        unsafe {
            std::env::set_var("XDG_DATA_DIRS", dir.path().to_str().unwrap());
        }
        let dirs = xdg_application_dirs();
        assert!(dirs.contains(&apps_dir));
    }

    // --- search_apps ---

    fn make_app(name: &str, comment: &str, keywords: &[&str]) -> AppEntry {
        AppEntry {
            name: name.to_string(),
            exec: format!("{name}-bin"),
            icon: String::new(),
            comment: comment.to_string(),
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
            categories: Vec::new(),
        }
    }

    #[test]
    fn search_empty_query_returns_all() {
        let apps = vec![make_app("Firefox", "", &[]), make_app("Chrome", "", &[])];
        let results = search_apps(&apps, "");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_by_name() {
        let apps = vec![
            make_app("Firefox", "Web browser", &[]),
            make_app("Thunar", "File manager", &[]),
        ];
        let results = search_apps(&apps, "fire");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Firefox");
    }

    #[test]
    fn search_by_comment() {
        let apps = vec![
            make_app("Firefox", "Web browser", &[]),
            make_app("Thunar", "File manager", &[]),
        ];
        let results = search_apps(&apps, "manager");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Thunar");
    }

    #[test]
    fn search_by_keyword() {
        let apps = vec![
            make_app("Firefox", "", &["internet", "web"]),
            make_app("Thunar", "", &["files"]),
        ];
        let results = search_apps(&apps, "internet");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Firefox");
    }

    #[test]
    fn search_no_match_returns_empty() {
        let apps = vec![make_app("Firefox", "Web browser", &["internet"])];
        let results = search_apps(&apps, "spreadsheet");
        assert!(results.is_empty());
    }

    #[test]
    fn search_name_matches_before_comment() {
        let apps = vec![
            make_app("Editor", "A text editor", &[]),
            make_app("Vim", "Editor for terminals", &[]),
        ];
        // "Editor" matches by name (priority 0), "Vim" matches by comment (priority 1)
        let results = search_apps(&apps, "editor");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "Editor");
        assert_eq!(results[1].name, "Vim");
    }

    // --- strip_field_codes ---

    #[test]
    fn strip_single_code() {
        assert_eq!(strip_field_codes("firefox %u"), "firefox");
    }

    #[test]
    fn strip_multiple_codes() {
        assert_eq!(strip_field_codes("gimp %f %U"), "gimp");
    }

    #[test]
    fn strip_no_codes() {
        assert_eq!(strip_field_codes("app"), "app");
    }

    #[test]
    fn strip_escaped_percent() {
        assert_eq!(strip_field_codes("100%% done"), "100% done");
    }

    #[test]
    fn strip_preserves_flags() {
        assert_eq!(
            strip_field_codes("app --flag %u --other"),
            "app --flag --other"
        );
    }
}
