extern crate native_windows_gui as nwg;
extern crate native_windows_derive as nwd;

use nwd::NwgUi;
use nwg::NativeUi;
use std::cell::Cell;
use std::time::Duration;

#[derive(Default, NwgUi)]
pub struct Wizard {
    #[nwg_control(size: (500, 400), title: "Ember", flags: "WINDOW|VISIBLE")]
    #[nwg_events(OnWindowClose: [Wizard::on_close])]
    window: nwg::Window,

    #[nwg_resource(family: "Segoe UI", size: 22, weight: 700)]
    font_title: nwg::Font,

    // ── Page 1: Welcome ─────────────────────────────────────────────────

    #[nwg_control(parent: window, text: "Ember",
        size: (460, 30), position: (20, 20), font: Some(&data.font_title))]
    p1_title: nwg::Label,

    #[nwg_control(parent: window,
        text: "A simple installer for the Ember Discord unlock.\r\n\r\nClick Next to continue.",
        size: (460, 180), position: (20, 60))]
    p1_text: nwg::Label,

    // ── Page 2: How it works ────────────────────────────────────────────

    #[nwg_control(parent: window, text: "How it works",
        size: (460, 30), position: (20, 20), font: Some(&data.font_title))]
    p2_title: nwg::Label,

    #[nwg_control(parent: window,
        text: "Discord Phoenix won't unlock if you open Discord directly. Launch it through Ember instead \u{2014} Ember patches the unlock each time it starts.\r\n\r\nThis installer sets that up for you.",
        size: (460, 180), position: (20, 60))]
    p2_text: nwg::Label,

    // ── Page 3: Terms ───────────────────────────────────────────────────

    #[nwg_control(parent: window, text: "Terms",
        size: (460, 30), position: (20, 20), font: Some(&data.font_title))]
    p3_title: nwg::Label,

    #[nwg_control(parent: window,
        text: "Ember is not affiliated with Discord. Use it at your own risk \u{2014} anything that happens to your account is your responsibility.",
        size: (460, 120), position: (20, 60))]
    p3_text: nwg::Label,

    #[nwg_control(parent: window, text: "I agree to these terms",
        size: (300, 25), position: (20, 195))]
    #[nwg_events(OnButtonClick: [Wizard::on_agree_changed])]
    p3_agree: nwg::CheckBox,

    // ── Page 4: Options ─────────────────────────────────────────────────

    #[nwg_control(parent: window, text: "Options",
        size: (460, 30), position: (20, 20), font: Some(&data.font_title))]
    p4_title: nwg::Label,

    #[nwg_control(parent: window, text: "Pin to taskbar (recommended)",
        size: (400, 25), position: (20, 70))]
    p4_taskbar: nwg::CheckBox,

    #[nwg_control(parent: window, text: "Add to desktop (recommended)",
        size: (400, 25), position: (20, 105))]
    p4_desktop: nwg::CheckBox,

    // ── Page 5: Installing ──────────────────────────────────────────────

    #[nwg_control(parent: window, text: "Installing\u{2026}",
        size: (460, 30), position: (20, 20), font: Some(&data.font_title))]
    p5_title: nwg::Label,

    #[nwg_control(parent: window, text: "Copying files and creating shortcuts\u{2026}",
        size: (460, 180), position: (20, 60))]
    p5_text: nwg::Label,

    // ── Page 6: Done ────────────────────────────────────────────────────

    #[nwg_control(parent: window, text: "Done",
        size: (460, 30), position: (20, 20), font: Some(&data.font_title))]
    p6_title: nwg::Label,

    #[nwg_control(parent: window,
        text: "Ember is installed. Open Discord from the Ember icon.",
        size: (460, 100), position: (20, 60))]
    p6_text: nwg::Label,

    #[nwg_control(parent: window, text: "Launch Ember now",
        size: (300, 25), position: (20, 165))]
    p6_launch: nwg::CheckBox,

    #[nwg_control(parent: window,
        text: "Tip: drag the Ember shortcut from your Start Menu to the taskbar to pin it.",
        size: (460, 40), position: (20, 200))]
    p6_pin_note: nwg::Label,

    // ── Navigation ──────────────────────────────────────────────────────

    #[nwg_control(parent: window, text: "Back", size: (80, 30), position: (230, 320))]
    #[nwg_events(OnButtonClick: [Wizard::on_back])]
    back_btn: nwg::Button,

    #[nwg_control(parent: window, text: "Next", size: (80, 30), position: (315, 320))]
    #[nwg_events(OnButtonClick: [Wizard::on_next])]
    next_btn: nwg::Button,

    #[nwg_control(parent: window, text: "Cancel", size: (80, 30), position: (400, 320))]
    #[nwg_events(OnButtonClick: [Wizard::on_close])]
    cancel_btn: nwg::Button,

    // ── Timer (fires once to run the install after the UI repaints) ─────

    #[nwg_control(parent: window, interval: Duration::from_millis(100))]
    #[nwg_events(OnTimerTick: [Wizard::on_install_tick])]
    install_timer: nwg::AnimationTimer,

    // ── State ───────────────────────────────────────────────────────────

    page: Cell<usize>,
    pin_failed: Cell<bool>,
}

impl Wizard {
    fn on_close(&self) {
        nwg::stop_thread_dispatch();
    }

    fn on_next(&self) {
        match self.page.get() {
            1 => self.show_page(2),
            2 => self.show_page(3),
            3 => self.show_page(4),
            4 => {
                self.show_page(5);
                self.install_timer.start();
            }
            6 => {
                if self.p6_launch.check_state() == nwg::CheckBoxState::Checked {
                    let _ = super::install::launch_installed();
                }
                nwg::stop_thread_dispatch();
            }
            _ => {}
        }
    }

    fn on_back(&self) {
        match self.page.get() {
            2 => self.show_page(1),
            3 => self.show_page(2),
            4 => self.show_page(3),
            _ => {}
        }
    }

    fn on_agree_changed(&self) {
        if self.page.get() == 3 {
            let checked = self.p3_agree.check_state() == nwg::CheckBoxState::Checked;
            self.next_btn.set_enabled(checked);
        }
    }

    fn on_install_tick(&self) {
        self.install_timer.stop();
        let opts = super::install::InstallOptions {
            pin_taskbar: self.p4_taskbar.check_state() == nwg::CheckBoxState::Checked,
            add_desktop: self.p4_desktop.check_state() == nwg::CheckBoxState::Checked,
        };
        match super::install::do_install(&opts) {
            Ok(pin_failed) => {
                self.pin_failed.set(pin_failed);
                self.show_page(6);
            }
            Err(e) => {
                super::alert(&format!("Install failed: {e}"));
                self.show_page(4);
            }
        }
    }

    // ── Page management ─────────────────────────────────────────────────

    fn show_page(&self, page: usize) {
        self.page.set(page);
        self.hide_all_pages();

        match page {
            1 => {
                self.p1_title.set_visible(true);
                self.p1_text.set_visible(true);
                self.back_btn.set_visible(false);
                self.next_btn.set_visible(true);
                self.next_btn.set_text("Next");
                self.next_btn.set_enabled(true);
                self.cancel_btn.set_visible(true);
            }
            2 => {
                self.p2_title.set_visible(true);
                self.p2_text.set_visible(true);
                self.back_btn.set_visible(true);
                self.next_btn.set_visible(true);
                self.next_btn.set_text("Next");
                self.next_btn.set_enabled(true);
                self.cancel_btn.set_visible(true);
            }
            3 => {
                self.p3_title.set_visible(true);
                self.p3_text.set_visible(true);
                self.p3_agree.set_visible(true);
                self.back_btn.set_visible(true);
                self.next_btn.set_visible(true);
                self.next_btn.set_text("Next");
                let checked =
                    self.p3_agree.check_state() == nwg::CheckBoxState::Checked;
                self.next_btn.set_enabled(checked);
                self.cancel_btn.set_visible(true);
            }
            4 => {
                self.p4_title.set_visible(true);
                self.p4_taskbar.set_visible(true);
                self.p4_desktop.set_visible(true);
                self.back_btn.set_visible(true);
                self.next_btn.set_visible(true);
                self.next_btn.set_text("Install");
                self.next_btn.set_enabled(true);
                self.cancel_btn.set_visible(true);
            }
            5 => {
                self.p5_title.set_visible(true);
                self.p5_text.set_visible(true);
                self.back_btn.set_visible(false);
                self.next_btn.set_visible(false);
                self.cancel_btn.set_visible(false);
            }
            6 => {
                self.p6_title.set_visible(true);
                self.p6_text.set_visible(true);
                self.p6_launch.set_visible(true);
                if self.pin_failed.get() {
                    self.p6_pin_note.set_visible(true);
                }
                self.back_btn.set_visible(false);
                self.next_btn.set_visible(true);
                self.next_btn.set_text("Finish");
                self.next_btn.set_enabled(true);
                self.cancel_btn.set_visible(false);
            }
            _ => {}
        }
    }

    fn hide_all_pages(&self) {
        self.p1_title.set_visible(false);
        self.p1_text.set_visible(false);
        self.p2_title.set_visible(false);
        self.p2_text.set_visible(false);
        self.p3_title.set_visible(false);
        self.p3_text.set_visible(false);
        self.p3_agree.set_visible(false);
        self.p4_title.set_visible(false);
        self.p4_taskbar.set_visible(false);
        self.p4_desktop.set_visible(false);
        self.p5_title.set_visible(false);
        self.p5_text.set_visible(false);
        self.p6_title.set_visible(false);
        self.p6_text.set_visible(false);
        self.p6_launch.set_visible(false);
        self.p6_pin_note.set_visible(false);
    }
}

pub fn show() -> super::Result<()> {
    nwg::init().map_err(|e| format!("GUI init: {e}"))?;

    let mut default_font = nwg::Font::default();
    nwg::Font::builder()
        .family("Segoe UI")
        .size(17)
        .build(&mut default_font)
        .map_err(|e| format!("font: {e}"))?;
    nwg::Font::set_global_default(Some(default_font));

    let app = Wizard::build_ui(Default::default()).map_err(|e| format!("UI build: {e}"))?;

    // Set initial checkbox states (done after build so we avoid derive-macro
    // enum-resolution issues with CheckBoxState in attributes).
    app.p4_taskbar
        .set_check_state(nwg::CheckBoxState::Checked);
    app.p4_desktop
        .set_check_state(nwg::CheckBoxState::Checked);
    app.p6_launch
        .set_check_state(nwg::CheckBoxState::Checked);

    // AnimationTimer may start active; ensure it's stopped until page 5.
    app.install_timer.stop();

    app.show_page(1);

    nwg::dispatch_thread_events();
    Ok(())
}
