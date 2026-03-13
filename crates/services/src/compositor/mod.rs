//! Compositor IPC — workspaces, active window, etc.
//!
//! Auto-detects the running compositor (Hyprland / Niri) and delegates to
//! the appropriate backend. Adding support for a new compositor means adding
//! a module here and extending the `detect` function.

pub mod hyprland;
pub mod niri;

use gpui::{App, AppContext, Entity};

#[derive(Debug, Clone, Default)]
pub struct Workspace {
    pub id: i32,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CompositorState {
    pub active_workspace: i32,
    pub workspaces: Vec<Workspace>,
    pub active_window: Option<String>,
}

pub struct CompositorService;

impl CompositorService {
    /// Auto-detect the running compositor and start the appropriate IPC backend.
    /// Returns a GPUI entity holding current workspace/window state.
    pub fn start(cx: &mut App) -> Entity<CompositorState> {
        let entity = cx.new(|_| CompositorState::default());

        match detect_compositor() {
            Compositor::Hyprland => hyprland::start(entity.clone(), cx),
            Compositor::Niri     => niri::start(entity.clone(), cx),
            Compositor::Unknown  => {
                tracing::warn!("Unknown compositor — workspace tracking disabled");
            }
        }

        entity
    }
}

enum Compositor { Hyprland, Niri, Unknown }

fn detect_compositor() -> Compositor {
    detect_compositor_with(|key| std::env::var(key).is_ok())
}

fn detect_compositor_with(has_var: impl Fn(&str) -> bool) -> Compositor {
    if has_var("HYPRLAND_INSTANCE_SIGNATURE") {
        Compositor::Hyprland
    } else if has_var("NIRI_SOCKET") {
        Compositor::Niri
    } else {
        Compositor::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- CompositorState ---

    #[test]
    fn compositor_state_default() {
        let s = CompositorState::default();
        assert_eq!(s.active_workspace, 0);
        assert!(s.workspaces.is_empty());
        assert_eq!(s.active_window, None);
    }

    #[test]
    fn compositor_state_clone() {
        let ws = Workspace { id: 1, name: "main".into(), active: true };
        let a = CompositorState {
            active_workspace: 1,
            workspaces: vec![ws],
            active_window: Some("kitty".into()),
        };
        let b = a.clone();
        assert_eq!(b.active_workspace, 1);
        assert_eq!(b.workspaces.len(), 1);
        assert_eq!(b.workspaces[0].name, "main");
        assert_eq!(b.active_window.as_deref(), Some("kitty"));
    }

    // --- Workspace ---

    #[test]
    fn workspace_default() {
        let w = Workspace::default();
        assert_eq!(w.id, 0);
        assert!(w.name.is_empty());
        assert!(!w.active);
    }

    #[test]
    fn workspace_clone() {
        let a = Workspace { id: 3, name: "work".into(), active: false };
        let b = a.clone();
        assert_eq!(b.id, 3);
        assert_eq!(b.name, "work");
    }

    // --- detect_compositor (env-var driven) ---
    // Tests use detect_compositor_with() to avoid touching process-wide env
    // vars, which are unsafe to mutate in a parallel test runner.

    #[test]
    fn detect_unknown_when_no_vars_set() {
        assert!(matches!(detect_compositor_with(|_| false), Compositor::Unknown));
    }

    #[test]
    fn detect_hyprland_when_sig_set() {
        let result = detect_compositor_with(|k| k == "HYPRLAND_INSTANCE_SIGNATURE");
        assert!(matches!(result, Compositor::Hyprland));
    }

    #[test]
    fn detect_niri_when_socket_set() {
        let result = detect_compositor_with(|k| k == "NIRI_SOCKET");
        assert!(matches!(result, Compositor::Niri));
    }

    #[test]
    fn hyprland_takes_priority_over_niri() {
        let result = detect_compositor_with(|_| true);
        assert!(matches!(result, Compositor::Hyprland));
    }
}
