//! Tiny window that asks for a user name, password or key passphrase when git needs one.

use eframe::egui;
use crate::i18n::tr;
use std::sync::{Arc, Mutex};

pub fn run(prompt: &str) -> i32 {
    let answer: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let secret = {
        let p = prompt.to_lowercase();
        p.contains("password") || p.contains("passphrase") || p.contains("token")
    };
    let prompt = prompt.trim().to_string();
    let host = crate::credentials::host_key_question(&prompt);
    let about_host = host.is_some();
    let height = if about_host { 250.0 } else { 150.0 };
    let out = answer.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RefBraid — authentication")
            .with_app_id("refbraid")
            .with_inner_size([if about_host { 560.0 } else { 460.0 }, height])
            .with_resizable(false),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "RefBraid askpass",
        options,
        Box::new(move |cc| {
            crate::theme::setup(&cc.egui_ctx);
            Ok(Box::new(Ask {
                host,
                prompt,
                secret,
                text: String::new(),
                out,
                focused: false,
            }))
        }),
    );
    let v = answer.lock().unwrap().take();
    match v {
        Some(s) => {
            println!("{s}");
            0
        }
        // A closed window is "no" to ssh's question about an unknown server.
        None if about_host => {
            println!("no");
            0
        }
        None => 1,
    }
}

struct Ask {
    /// ssh's question about a server it has not seen before, when that is what is asked.
    host: Option<crate::credentials::HostKey>,
    prompt: String,
    secret: bool,
    text: String,
    out: Arc<Mutex<Option<String>>>,
    focused: bool,
}

impl eframe::App for Ask {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(crate::theme::BG_BAR()).inner_margin(16))
            .show(ui, |ui| {
                if let Some(h) = self.host.clone() {
                    ui.label(egui::RichText::new(tr("Connect to a server this computer has not seen before?")).strong());
                    ui.add_space(6.0);
                    ui.label(format!("{}  {}", tr("Server:"), h.host));
                    ui.label(format!("{} ({}):", tr("Fingerprint of its key"), h.kind));
                    ui.add(egui::Label::new(egui::RichText::new(&h.fingerprint).monospace()).selectable(true));
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(egui::RichText::new(tr("If you can, compare the fingerprint with the one the server's administrator or the hosting publishes. After you trust it, the server is remembered and this question is not asked again.")).color(crate::theme::TEXT_MID()).size(12.0)).wrap());
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(tr("Trust and Connect")).clicked() {
                            *self.out.lock().unwrap() = Some("yes".into());
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button(tr("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            *self.out.lock().unwrap() = Some("no".into());
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    return;
                }
                ui.label(egui::RichText::new(tr("Git needs your credentials")).strong());
                ui.add_space(4.0);
                ui.label(&self.prompt);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.text)
                        .password(self.secret)
                        .desired_width(f32::INFINITY),
                );
                let r = crate::theme::edit_menu(r);
                if !self.focused {
                    r.request_focus();
                    self.focused = true;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(tr("OK")).clicked() || enter {
                        *self.out.lock().unwrap() = Some(self.text.clone());
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button(tr("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
    }
}
