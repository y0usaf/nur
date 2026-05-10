//! MPRIS media player service via D-Bus.
//!
//! Polls running MPRIS players (bus names matching `org.mpris.MediaPlayer2.*`)
//! every 2 s using `playerctl`. Uses the "most recently active" heuristic:
//! the first player reported by `playerctl --list-all` that is Playing wins,
//! otherwise falls back to the first listed player.
//!
//! A dedicated OS thread runs the blocking `playerctl` calls; a GPUI task
//! drains the shared slot.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use gpui::{App, AppContext, Entity};

#[derive(Debug, Clone, Default)]
pub struct MprisState {
    /// Short player name, e.g. `"spotify"`, `"firefox"`. Empty when no player.
    pub player_name: String,
    /// `"Playing"`, `"Paused"`, `"Stopped"`, or `""` when no player.
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    /// Track length in seconds, 0 when unknown.
    pub length: u64,
    /// Playback position in seconds, 0 when unknown.
    pub position: u64,
    /// Volume 0.0–1.0, 1.0 when unknown.
    pub volume: f64,
}

pub struct MprisService;

static ACTIVE_PLAYER: OnceLock<Arc<Mutex<Option<String>>>> = OnceLock::new();

impl MprisService {
    pub fn start(cx: &mut App) -> Entity<MprisState> {
        let entity = cx.new(|_| MprisState::default());
        let weak = entity.downgrade();

        let slot: Arc<Mutex<Option<MprisState>>> = Arc::new(Mutex::new(None));
        let slot_writer = slot.clone();
        let slot_reader = slot.clone();

        std::thread::spawn(move || {
            loop {
                let state = read_mpris_state();
                if let Ok(mut guard) = slot_writer.lock() {
                    *guard = Some(state);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });

        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;

                let state = slot_reader.lock().ok().and_then(|mut g| g.take());
                if let Some(state) = state {
                    cx.update(|cx| {
                        if let Some(e) = weak.upgrade() {
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

        entity
    }
}

fn playerctl(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("playerctl")
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// Choose the active player: first Playing, else first available.
fn choose_player() -> Option<String> {
    let list = playerctl(&["--list-all"])?;
    if list.is_empty() {
        return None;
    }
    let players: Vec<&str> = list.lines().collect();
    // Prefer a playing player.
    for &p in &players {
        if playerctl(&["-p", p, "status"]).as_deref() == Some("Playing") {
            return Some(p.to_string());
        }
    }
    players.first().map(|s| s.to_string())
}

fn read_mpris_state() -> MprisState {
    let Some(player) = choose_player() else {
        set_active_player(None);
        return MprisState::default();
    };

    set_active_player(Some(player.clone()));

    let get = |field: &str| {
        playerctl(&["-p", &player, "metadata", "--format", field]).unwrap_or_default()
    };

    let status = playerctl(&["-p", &player, "status"]).unwrap_or_default();
    let title = get("{{xesam:title}}");
    let artist = get("{{xesam:artist}}");
    let album = get("{{xesam:album}}");
    let art_url = get("{{mpris:artUrl}}");

    // Length comes in microseconds from MPRIS; playerctl formats it as µs by default.
    let length: u64 = playerctl(&["-p", &player, "metadata", "mpris:length"])
        .and_then(|s| s.parse::<u64>().ok())
        .map(|us| us / 1_000_000)
        .unwrap_or(0);

    let position: u64 = playerctl(&["-p", &player, "position"])
        .and_then(|s| s.parse::<f64>().ok())
        .map(|f| f as u64)
        .unwrap_or(0);

    let volume: f64 = playerctl(&["-p", &player, "volume"])
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(1.0);

    MprisState {
        player_name: player,
        status,
        title,
        artist,
        album,
        art_url,
        length,
        position,
        volume,
    }
}

/// Send a playerctl command to the currently active player. Fire-and-forget.
fn playerctl_command(cmd: &'static str) {
    std::thread::spawn(move || {
        let mut command = std::process::Command::new("playerctl");
        if let Some(player) = active_player() {
            command.args(["-p", &player]);
        }

        match command.arg(cmd).status() {
            Ok(s) if !s.success() => tracing::warn!("playerctl {cmd} exited {s}"),
            Err(e) => tracing::warn!("playerctl {cmd} failed: {e}"),
            _ => {}
        }
    });
}

fn active_player() -> Option<String> {
    ACTIVE_PLAYER
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

fn set_active_player(player: Option<String>) {
    if let Ok(mut guard) = ACTIVE_PLAYER
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .lock()
    {
        *guard = player;
    }
}

pub fn play() {
    playerctl_command("play");
}
pub fn pause() {
    playerctl_command("pause");
}
pub fn play_pause() {
    playerctl_command("play-pause");
}
pub fn next() {
    playerctl_command("next");
}
pub fn previous() {
    playerctl_command("previous");
}
pub fn stop() {
    playerctl_command("stop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpris_state_default() {
        let s = MprisState::default();
        assert!(s.player_name.is_empty());
        assert!(s.status.is_empty());
        assert_eq!(s.volume, 1.0);
        assert_eq!(s.length, 0);
    }

    #[test]
    fn mpris_state_clone() {
        let a = MprisState {
            player_name: "spotify".into(),
            status: "Playing".into(),
            volume: 0.8,
            ..Default::default()
        };
        let b = a.clone();
        assert_eq!(b.player_name, "spotify");
        assert!((b.volume - 0.8).abs() < 1e-6);
    }

    #[test]
    fn play_does_not_panic_when_playerctl_missing() {
        play();
    }

    #[test]
    fn pause_does_not_panic_when_playerctl_missing() {
        pause();
    }

    #[test]
    fn next_does_not_panic_when_playerctl_missing() {
        next();
    }

    #[test]
    fn previous_does_not_panic_when_playerctl_missing() {
        previous();
    }
}
