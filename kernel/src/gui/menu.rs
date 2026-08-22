//! Menu handling — context menus and start menu dispatch.

use super::*;

impl Compositor {
    pub(crate) fn show_desktop_context_menu(&mut self, x: usize, y: usize) {
        self.context_menu = ContextMenu {
            open: true,
            x, y,
            items: alloc::vec![
                ("Terminal", ContextAction::OpenTerminal),
                ("File Manager", ContextAction::OpenFileManager),
                ("System Monitor", ContextAction::OpenMonitor),
                ("---", ContextAction::CloseWindow),
                ("Shutdown", ContextAction::Shutdown),
            ],
            selected: None,
        };
    }

    pub(crate) fn show_window_context_menu(&mut self, x: usize, y: usize, _win_idx: usize) {
        self.context_menu = ContextMenu {
            open: true,
            x, y,
            items: alloc::vec![
                ("Minimize", ContextAction::MinimizeWindow),
                ("Maximize", ContextAction::MaximizeWindow),
                ("Close", ContextAction::CloseWindow),
            ],
            selected: None,
        };
    }

    pub(crate) fn execute_context_action(&mut self, idx: usize) {
        if idx >= self.context_menu.items.len() { return; }
        let action = &self.context_menu.items[idx].1;
        match action {
            ContextAction::OpenTerminal => self.create_terminal_window(),
            ContextAction::OpenFileManager => self.create_file_manager_window(),
            ContextAction::OpenMonitor => self.create_monitor_window(),
            ContextAction::Shutdown => self.shutdown_qemu(),
            ContextAction::CloseWindow => {
                if let Some(idx) = self.focused_window() {
                    self.close_pending = Some(idx);
                }
            }
            ContextAction::MinimizeWindow => {
                if let Some(idx) = self.focused_window() {
                    self.windows[idx].minimized = true;
                }
            }
            ContextAction::MaximizeWindow => {
                if let Some(idx) = self.focused_window() {
                    self.windows[idx].toggle_maximize();
                }
            }
        }
    }
}
