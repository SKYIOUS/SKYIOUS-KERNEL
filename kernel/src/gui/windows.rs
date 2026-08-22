//! Window lifecycle — creation, destruction, and management.

use super::*;

impl Compositor {
    pub(crate) fn create_terminal_window(&mut self) {
        let term_w = 600;
        let term_h = 360;
        let mut term_win = window::Window::new(100, 60, term_w + 2, term_h + 22, "Terminal");
        term_win.terminal = Some(crate::gui::terminal::TerminalWidget::new(term_w, term_h));
        if let Some(ref mut t) = term_win.terminal {
            t.print_str("SkyOS Terminal v0.1\n");
            t.print_str("Type commands below...\n\n$ ");
        }
        self.windows.push(term_win);
    }

    pub(crate) fn create_info_window(&mut self, title: &str, body: &str) {
        let w = 340;
        let h = 200;
        let mut info_win = window::Window::new(120, 80, w, h, title);
        let mut term = crate::gui::terminal::TerminalWidget::new(w - 4, h - 24);
        term.print_str(body);
        info_win.terminal = Some(term);
        self.windows.push(info_win);
    }

    pub(crate) fn create_monitor_window(&mut self) {
        let w = 360;
        let h = 280;
        let mut mon_win = window::Window::new(150, 90, w, h, "System Monitor");
        let mut term = crate::gui::terminal::TerminalWidget::new(w - 4, h - 24);
        term.is_monitor = true;
        term.print_str("Loading system data...\n");
        mon_win.terminal = Some(term);
        self.windows.push(mon_win);
    }

    pub(crate) fn create_file_manager_window(&mut self) {
        let w = 400;
        let h = 300;
        let mut fm_win = window::Window::new(130, 70, w, h, "File Manager");
        fm_win.file_manager = Some(crate::gui::filemanager::FileManagerWidget::new(w - 4, h - 24));
        self.windows.push(fm_win);
    }

    pub(crate) fn shutdown_qemu(&mut self) {
        unsafe { x86_64::instructions::port::Port::<u16>::new(0x604).write(0x2000); }
        x86_64::instructions::interrupts::disable();
        loop { x86_64::instructions::hlt(); }
    }
}
