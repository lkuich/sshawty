#![cfg_attr(windows, windows_subsystem = "windows")]
mod design;
#[cfg(not(target_os = "macos"))]
mod drop_position;
#[cfg(target_os = "macos")]
mod macos;
mod state;
mod transfer;
use eframe::egui;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

struct App {
    state: state::State,
    dir: PathBuf,
    editor: Option<state::Host>,
    message: String,
    copied_until: Option<std::time::Instant>,
    warning: String,
    statuses: HashMap<String, String>,
    tx: mpsc::Sender<transfer::Job>,
    rx: mpsc::Receiver<transfer::Event>,
    pending: Vec<Arc<AtomicBool>>,
    textures: HashMap<String, egui::TextureHandle>,
    clipboard: Option<arboard::Clipboard>,
    save_blocked: bool,
    confirm_close: bool,
    #[cfg(target_os = "macos")]
    native: macos::DropBridge,
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        design::configure(&cc.egui_ctx);
        let dir = state::data_dir();
        let (state, message, save_blocked) = match state::load(&dir) {
            Ok(s) => (s, String::new(), false),
            Err(e) => (state::State::default(), e, true),
        };
        let (tx, rx) = transfer::worker(cc.egui_ctx.clone());
        Self {
            state,
            dir,
            editor: None,
            message,
            copied_until: None,
            warning: String::new(),
            statuses: HashMap::new(),
            tx,
            rx,
            pending: vec![],
            textures: HashMap::new(),
            clipboard: arboard::Clipboard::new().ok(),
            save_blocked,
            confirm_close: false,
            #[cfg(target_os = "macos")]
            native: macos::DropBridge::new(cc),
        }
    }
    fn save(&mut self) {
        if !self.save_blocked
            && let Err(e) = state::save(&self.dir, &self.state)
        {
            self.warning = format!("Could not save history/settings: {e}");
        }
    }
    fn copy(&mut self, text: &str) {
        match self
            .clipboard
            .as_mut()
            .ok_or("Clipboard unavailable".into())
            .and_then(|c| c.set_text(text).map_err(|e| e.to_string()))
        {
            Ok(()) => {
                self.message = format!("Copied: {text}");
                self.copied_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            }
            Err(e) => {
                self.copied_until = None;
                self.message = format!("Upload path is available, but copy failed: {e}");
            }
        }
    }
    fn enqueue(&mut self, id: &str, paths: Vec<PathBuf>, temporary: Option<tempfile::TempDir>) {
        if paths.is_empty() {
            return;
        }
        if let Some(host) = self.state.hosts.iter().find(|h| h.id == id).cloned() {
            self.statuses.insert(id.to_owned(), "Queued".into());
            let cancel = Arc::new(AtomicBool::new(false));
            if self
                .tx
                .send(transfer::Job {
                    host,
                    sources: paths,
                    temporary,
                    cancel: cancel.clone(),
                })
                .is_ok()
            {
                self.pending.push(cancel);
                self.message = "Queued upload".into();
            }
        }
    }
    fn paste(&mut self) {
        let Some(id) = self.state.selected.clone() else {
            self.message = "Select a host first.".into();
            return;
        };
        let result = (|| -> Result<tempfile::TempDir, String> {
            let image = self
                .clipboard
                .as_mut()
                .ok_or("Clipboard unavailable")?
                .get_image()
                .map_err(|e| format!("No image on clipboard: {e}"))?;
            if image.bytes.len() > 256 * 1024 * 1024 {
                return Err("Clipboard image exceeds 256 MiB.".into());
            }
            let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
            image::save_buffer_with_format(
                dir.path().join("clipboard.png"),
                &image.bytes,
                image.width.try_into().map_err(|_| "Image too wide")?,
                image.height.try_into().map_err(|_| "Image too tall")?,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )
            .map_err(|e| e.to_string())?;
            Ok(dir)
        })();
        match result {
            Ok(dir) => self.enqueue(&id, vec![dir.path().join("clipboard.png")], Some(dir)),
            Err(e) => self.message = e,
        }
    }
    fn events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                transfer::Event::Status(id, s) => {
                    self.statuses.insert(id, "Uploading…".into());
                    self.message = s;
                }
                transfer::Event::Uploaded(record, thumb) => {
                    let path = state::thumbnail(&self.dir, &record.id);
                    if let Err(e) = std::fs::create_dir_all(path.parent().unwrap())
                        .and_then(|_| std::fs::write(path, thumb))
                    {
                        self.warning = format!("Uploaded, but thumbnail save failed: {e}");
                    }
                    self.state.history.insert(0, record);
                    self.save();
                }
                transfer::Event::Finished(id, paths, errors) => {
                    self.statuses.insert(
                        id,
                        if errors.is_empty() {
                            "Uploaded"
                        } else {
                            "Failed / cancelled; see status below"
                        }
                        .into(),
                    );
                    if !self.pending.is_empty() {
                        self.pending.remove(0);
                    }
                    if !paths.is_empty() {
                        self.copy(&paths.join("\n"));
                    }
                    if !errors.is_empty() {
                        self.message = format!(
                            "{}\n{}",
                            if paths.is_empty() {
                                "Upload failed"
                            } else {
                                "Some uploads succeeded; see history"
                            },
                            errors.join("\n")
                        );
                    }
                }
            }
        }
    }
    fn remove_history(&mut self, ids: &[String]) {
        self.state.history.retain(|r| !ids.contains(&r.id));
        self.save();
        for id in ids {
            let _ = std::fs::remove_file(state::thumbnail(&self.dir, id));
            self.textures.remove(id);
        }
    }
}
impl App {
    fn toggle_focus(&mut self, id: &str) {
        self.state.toggle_focus(id);
        self.save();
    }

    fn host_card(
        &mut self,
        ui: &mut egui::Ui,
        host: &state::Host,
        dragging: bool,
        drag_host: Option<&str>,
        drag_position: Option<egui::Pos2>,
    ) -> egui::Rect {
        use design::*;
        use egui::*;
        let height = if self.state.focused { 232.0 } else { 216.0 };
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        response.widget_info(|| {
            WidgetInfo::labeled(
                Role::Button,
                true,
                format!("Select {}. Double-click to toggle focus", host.name),
            )
        });
        let selected = self.state.selected.as_ref() == Some(&host.id);
        let drop_over = self.editor.is_none()
            && (drag_host == Some(host.id.as_str())
                || drag_position.is_some_and(|p| rect.intersect(ui.clip_rect()).contains(p)));
        let hover = response.contains_pointer() && !dragging && self.editor.is_none();
        let border = if drop_over || hover {
            ACCENT
        } else if selected {
            Color32::from_rgb(83, 123, 106)
        } else {
            BORDER
        };
        ui.painter().rect_filled(
            rect,
            14.0,
            if selected {
                Color32::from_rgb(29, 43, 39)
            } else {
                PANEL
            },
        );
        ui.painter().rect_stroke(
            rect,
            14.0,
            Stroke::new(if drop_over { 2.0 } else { 1.0 }, border),
            StrokeKind::Inside,
        );
        let tile = Rect::from_min_size(rect.min + vec2(18., 18.), vec2(32., 32.));
        ui.painter()
            .rect_filled(tile, 8.0, Color32::from_rgb(40, 57, 50));
        icon(ui.painter(), tile.shrink(7.0), Icon::Server, ACCENT);
        left_label(
            ui,
            Rect::from_min_size(
                rect.min + vec2(62., 16.),
                vec2((rect.width() - 156.).max(70.), 25.),
            ),
            Label::new(RichText::new(&host.name).size(18.).strong())
                .selectable(false)
                .truncate(),
        );
        left_label(
            ui,
            Rect::from_min_size(rect.min + vec2(62., 42.), vec2(rect.width() - 82., 18.)),
            Label::new(
                RichText::new(&host.target)
                    .monospace()
                    .size(11.)
                    .color(MUTED),
            )
            .selectable(false)
            .truncate(),
        )
        .on_hover_text(&host.target);
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    rect.right_top() + vec2(-86., 14.),
                    vec2(72., 34.),
                ))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let (glyph, label) = if self.state.focused {
                    (Icon::Collapse, "Show all hosts")
                } else {
                    (Icon::Expand, "Focus host")
                };
                if icon_button(ui, glyph, label).clicked() {
                    self.toggle_focus(&host.id);
                }
                if icon_button(ui, Icon::Edit, "Edit host").clicked() {
                    self.editor = Some(host.clone());
                }
            },
        );
        left_label(
            ui,
            Rect::from_min_size(rect.min + vec2(18., 72.), vec2(rect.width() - 36., 20.)),
            Label::new(
                RichText::new(&host.folder)
                    .monospace()
                    .size(12.)
                    .color(MUTED),
            )
            .selectable(false)
            .truncate(),
        )
        .on_hover_text(&host.folder);
        let drop_rect = Rect::from_min_max(
            rect.min + vec2(18., 104.),
            rect.right_bottom() - vec2(18., 44.),
        );
        ui.painter().rect_filled(
            drop_rect,
            9.,
            if drop_over {
                Color32::from_rgb(43, 76, 60)
            } else {
                BG
            },
        );
        ui.painter().rect_stroke(
            drop_rect,
            9.,
            Stroke::new(1., if hover || drop_over { ACCENT } else { BORDER }),
            StrokeKind::Inside,
        );
        let center = drop_rect.center();
        icon(
            ui.painter(),
            Rect::from_center_size(center - vec2(75., 0.), vec2(22., 22.)),
            Icon::Upload,
            if hover || drop_over { ACCENT } else { MUTED },
        );
        ui.painter().text(
            center + vec2(12., 0.),
            Align2::CENTER_CENTER,
            if drop_over {
                "Release to upload"
            } else {
                "Drop a screenshot"
            },
            FontId::proportional(14.),
            if hover || drop_over { ACCENT } else { MUTED },
        );
        let footer = Rect::from_min_size(
            rect.left_bottom() + vec2(18., -34.),
            vec2(rect.width() - 64., 24.),
        );
        let status = self
            .statuses
            .get(&host.id)
            .map(String::as_str)
            .unwrap_or(if selected {
                "Selected for paste"
            } else {
                "Double-click to focus"
            });
        left_label(
            ui,
            footer,
            Label::new(RichText::new(status).size(11.).color(if selected {
                ACCENT
            } else {
                MUTED
            }))
            .selectable(false)
            .truncate(),
        )
        .on_hover_text(status);
        if let Some(path) = self
            .state
            .history
            .iter()
            .find(|r| r.host_id == host.id)
            .map(|r| r.path.clone())
        {
            ui.scope_builder(
                UiBuilder::new().max_rect(Rect::from_min_size(
                    rect.right_bottom() - vec2(46., 37.),
                    vec2(30., 30.),
                )),
                |ui| {
                    if icon_button(ui, Icon::Copy, "Copy latest upload path")
                        .on_hover_text(&path)
                        .clicked()
                    {
                        self.copy(&path);
                    }
                },
            );
        }
        if response.double_clicked() {
            self.toggle_focus(&host.id);
        } else if response.clicked() {
            self.state.selected = Some(host.id.clone());
            self.save();
        }
        rect.intersect(ui.clip_rect())
    }

    fn history_row(&mut self, ui: &mut egui::Ui, record: &state::Upload) -> bool {
        use design::*;
        use egui::*;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 90.), Sense::hover());
        if !self.textures.contains_key(&record.id)
            && let Ok(img) = image::open(state::thumbnail(&self.dir, &record.id))
        {
            let img = img.to_rgba8();
            self.textures.insert(
                record.id.clone(),
                ui.ctx().load_texture(
                    &record.id,
                    ColorImage::from_rgba_unmultiplied(
                        [img.width() as usize, img.height() as usize],
                        &img,
                    ),
                    Default::default(),
                ),
            );
        }
        let thumb = Rect::from_min_size(rect.min + vec2(0., 9.), vec2(86., 64.));
        ui.painter().rect_filled(thumb, 8., PANEL);
        if let Some(tex) = self.textures.get(&record.id) {
            ui.put(
                thumb,
                Image::new(tex)
                    .fit_to_exact_size(thumb.size())
                    .corner_radius(8.),
            );
        } else {
            icon(ui.painter(), thumb.shrink(23.), Icon::Upload, MUTED);
        }
        let x = rect.min.x + 102.;
        let width = (rect.width() - 186.).max(60.);
        left_label(
            ui,
            Rect::from_min_size(pos2(x, rect.top() + 8.), vec2(width, 22.)),
            Label::new(RichText::new(&record.filename).strong().size(14.)).truncate(),
        )
        .on_hover_text(&record.filename);
        left_label(
            ui,
            Rect::from_min_size(pos2(x, rect.top() + 32.), vec2(width, 18.)),
            Label::new(
                RichText::new(format!("{}   ·   {}", record.host_name, record.time))
                    .size(11.)
                    .color(MUTED),
            )
            .truncate(),
        );
        let path_response = left_label(
            ui,
            Rect::from_min_size(pos2(x, rect.top() + 54.), vec2(width, 20.)),
            Label::new(
                RichText::new(&record.path)
                    .monospace()
                    .size(11.)
                    .color(ACCENT),
            )
            .truncate()
            .sense(Sense::click()),
        )
        .on_hover_text(&record.path)
        .on_hover_cursor(CursorIcon::PointingHand);
        if path_response.clicked() {
            self.copy(&record.path);
        }
        let mut remove = false;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    rect.right_center() - vec2(72., 16.),
                    vec2(72., 32.),
                ))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 2.;
                if icon_button(ui, Icon::Copy, "Copy path").clicked() {
                    self.copy(&record.path);
                }
                remove = icon_button(ui, Icon::Trash, "Remove from history").clicked();
            },
        );
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1., BORDER.gamma_multiply(0.6)),
        );
        ui.advance_cursor_after_rect(rect);
        remove
    }

    fn host_editor(&mut self, ctx: &egui::Context) {
        use design::*;
        use egui::*;
        let Some(mut host) = self.editor.take() else {
            return;
        };
        let mut keep = true;
        Window::new("Host settings")
            .anchor(Align2::CENTER_CENTER, [0., 0.])
            .default_width(360.)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(6.);
                ui.label(RichText::new("A destination for your screenshots").color(MUTED));
                ui.add_space(8.);
                for (label, field) in [
                    ("Name", &mut host.name),
                    ("SSH target", &mut host.target),
                    ("Remote folder", &mut host.folder),
                ] {
                    ui.label(RichText::new(label).size(12.).color(MUTED));
                    ui.add(
                        TextEdit::singleline(field)
                            .desired_width(f32::INFINITY)
                            .margin(vec2(10., 9.)),
                    );
                }
                ui.small(
                    RichText::new("Uses your existing SSH keys and configuration.").color(MUTED),
                );
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    if self.state.hosts.iter().any(|h| h.id == host.id)
                        && icon_button(ui, Icon::Trash, "Remove host").clicked()
                    {
                        self.state.hosts.retain(|h| h.id != host.id);
                        self.state.focused = false;
                        self.state.selected = self.state.hosts.first().map(|h| h.id.clone());
                        self.save();
                        keep = false;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                Button::new(RichText::new("Save host").color(BG).strong())
                                    .fill(ACCENT),
                            )
                            .clicked()
                        {
                            match host.validate() {
                                Ok(()) => {
                                    if let Some(h) =
                                        self.state.hosts.iter_mut().find(|h| h.id == host.id)
                                    {
                                        *h = host.clone();
                                    } else {
                                        self.state.hosts.push(host.clone());
                                    }
                                    self.state.selected = Some(host.id.clone());
                                    self.save();
                                    keep = false;
                                }
                                Err(e) => self.warning = e,
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            });
        if keep {
            self.editor = Some(host);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        use design::*;
        use egui::*;
        let ctx = root.ctx().clone();
        self.events();
        if ctx.input(|i| i.viewport().close_requested()) && !self.pending.is_empty() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.confirm_close {
            Window::new("Uploads in progress")
                .collapsible(false)
                .show(&ctx, |ui| {
                    ui.label("Cancel queued and active transfers before closing?");
                    if ui.button("Cancel uploads").clicked() {
                        for flag in &self.pending {
                            flag.store(true, Ordering::Relaxed);
                        }
                        self.confirm_close = false;
                    }
                    if ui.button("Keep uploading").clicked() {
                        self.confirm_close = false;
                    }
                });
        }
        if self.editor.is_none() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) && self.state.focused
            {
                self.state.focused = false;
                self.save();
            }
            let paste = ctx.input_mut(|i| {
                i.consume_key(Modifiers::COMMAND, Key::V)
                    || i.events.iter().any(|e| matches!(e, Event::PasteImage(_)))
            });
            if paste {
                self.paste();
            }
        }
        CentralPanel::default().frame(Frame::central_panel(root.style()).inner_margin(24.)).show(root,|ui| {
            ui.horizontal(|ui| {
                let (logo,_) = ui.allocate_exact_size(vec2(32.,32.),Sense::hover());
                ui.painter().rect_filled(logo,9.,ACCENT); icon(ui.painter(),logo.shrink(7.),Icon::Upload,BG);
                ui.label(RichText::new("sshawty").size(25.).strong());
                ui.with_layout(Layout::right_to_left(Align::Center),|ui| {
                    if icon_button(ui,Icon::Plus,"Add host").clicked() {self.editor=Some(state::Host::default());}
                    if icon_button(ui,Icon::Paste,"Paste image to selected host").clicked() {self.paste();}
                });
            });
            ui.add_space(16.);
            ui.separator();
            ui.add_space(12.);
            if self.state.focused {
                ui.horizontal(|ui| {
                    let back = ui.add(Button::new(RichText::new("     All hosts").color(ACCENT).strong()).min_size(vec2(122.,34.))).on_hover_text("Back to all hosts · Esc");
                    icon(ui.painter(),Rect::from_center_size(back.rect.left_center()+vec2(18.,0.),vec2(16.,16.)),Icon::Back,ACCENT);
                    if back.clicked() {self.state.focused=false;self.save();}
                    ui.label(RichText::new("FOCUSED VIEW").size(10.).color(MUTED));
                });
            } else {
                ui.horizontal(|ui| {ui.label(RichText::new("Your hosts").size(19.).strong());ui.label(RichText::new(format!("{:02}",self.state.hosts.len())).monospace().size(12.).color(MUTED));});
            }
            ui.label(RichText::new(if self.state.focused {"Drop here, or double-click the card to return to all hosts."} else {"Drop a screenshot. Paste the path."}).size(12.).color(MUTED));
            ui.add_space(6.);
            if self.save_blocked {ui.colored_label(Color32::LIGHT_RED,"Saved state could not be loaded. Repair state.json and restart; the original is preserved.");}
            if !self.warning.is_empty() {ui.colored_label(Color32::LIGHT_RED,&self.warning);}
            #[cfg(target_os="macos")]
            let (dragging,drag_host)=self.native.drag_hover();
            #[cfg(not(target_os="macos"))]
            let (dragging,drag_host)=(ctx.input(|i|!i.raw.hovered_files.is_empty()),None::<String>);
            #[cfg(target_os="macos")]
            let drag_position=None;
            #[cfg(not(target_os="macos"))]
            let drag_position=if dragging {ctx.request_repaint_after(std::time::Duration::from_millis(33));drop_position::current(frame,ctx.pixels_per_point())} else {None};
            let mut zones=vec![];
            let hosts:Vec<_>=self.state.hosts.iter().filter(|h|!self.state.focused||Some(&h.id)==self.state.selected.as_ref()).cloned().collect();
            ScrollArea::vertical().id_salt("hosts").max_height((ui.available_height()*0.56).max(248.)).show(ui,|ui| {
                if hosts.is_empty() { Frame::new().fill(PANEL).corner_radius(14.).inner_margin(28.).show(ui,|ui| {ui.heading("Your remote workspace, one drop away.");ui.label(RichText::new("Add your first SSH host to get started.").color(MUTED));if ui.button("+ Add host").clicked(){self.editor=Some(state::Host::default());}}); }
                let columns=if self.state.focused {1} else {((ui.available_width()/330.) as usize).clamp(1,3).min(hosts.len().max(1))};
                for row in hosts.chunks(columns) {
                    ui.columns(columns,|uis| {for (host,ui) in row.iter().zip(uis) {let rect=self.host_card(ui,host,dragging,drag_host.as_deref(),drag_position);zones.push((host.id.clone(),rect));}});
                    ui.add_space(4.);
                }
            });
            if self.editor.is_some() {zones.clear();}
            #[cfg(target_os="macos")] {
                let _ = frame;
                self.native.set_zones(zones.clone(),ctx.pixels_per_point());
                for drop in self.native.receive() {match drop {Ok(drop)=>self.enqueue(&drop.host,drop.paths,Some(drop.temporary)),Err(e)=>self.message=e}}
            }
            #[cfg(not(target_os="macos"))] {
                let files=ctx.input(|i|i.raw.dropped_files.clone());
                if !files.is_empty() {
                    let pos=drop_position::current(frame,ctx.pixels_per_point());
                    if let Some((id,_))=zones.iter().find(|(_,rect)|pos.is_some_and(|p|rect.contains(p))) {self.enqueue(id,files.iter().map(|f|f.path().to_owned()).collect(),None);} else {self.message="Drop directly onto a host card.".into();}
                }
            }
            ui.add_space(12.);
            let history:Vec<_>=self.state.history.iter().filter(|r|!self.state.focused||Some(&r.host_id)==self.state.selected.as_ref()).cloned().collect();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Recent uploads").size(18.).strong());ui.label(RichText::new(history.len().to_string()).size(12.).color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center),|ui| {if !history.is_empty() && icon_button(ui,Icon::Trash,"Clear visible history (remote files stay intact)").clicked(){self.remove_history(&history.iter().map(|r|r.id.clone()).collect::<Vec<_>>());}});
            });
            let mut remove=vec![];
            ScrollArea::vertical().id_salt("history").auto_shrink([false,false]).max_height((ui.available_height()-46.).max(70.)).show_rows(ui,100.,history.len().max(1),|ui,range| {
                if history.is_empty() {ui.add_space(22.);ui.horizontal(|ui| {let (r,_)=ui.allocate_exact_size(vec2(28.,28.),Sense::hover());icon(ui.painter(),r,Icon::Clock,MUTED);ui.label(RichText::new("Uploaded screenshots will appear here.").color(MUTED));});}
                else {for record in &history[range] { if self.history_row(ui,record) {remove.push(record.id.clone());} }}
            });
            if !remove.is_empty(){self.remove_history(&remove);}
            if self.textures.len()>100 {self.textures.clear();}
            ui.add_space(8.);
            ui.separator();
            ui.horizontal(|ui| {
                if !self.pending.is_empty() {ui.spinner();ui.label(RichText::new(format!("{} transfer batches",self.pending.len())).size(12.).color(MUTED));if icon_button(ui,Icon::Close,"Cancel all uploads").clicked(){for flag in &self.pending {flag.store(true,Ordering::Relaxed);}}}
                else {let (r,_)=ui.allocate_exact_size(vec2(8.,8.),Sense::hover());ui.painter().circle_filled(r.center(),3.,ACCENT);ui.label(RichText::new("Ready").size(11.).color(MUTED));}
                if !self.message.is_empty() && !self.message.starts_with("Copied:") {ui.add(Label::new(RichText::new(&self.message).size(11.).color(MUTED)).truncate()).on_hover_text(&self.message);}
            });
        });
        self.host_editor(&ctx);
        if let Some(remaining) = self
            .copied_until
            .and_then(|d| d.checked_duration_since(std::time::Instant::now()))
        {
            Area::new(Id::unique("clipboard_toast"))
                .anchor(Align2::CENTER_BOTTOM, [0., -24.])
                .order(Order::Foreground)
                .interactable(false)
                .show(&ctx, |ui| {
                    Frame::popup(ui.style())
                        .inner_margin(14.)
                        .corner_radius(10.)
                        .stroke(Stroke::new(1., ACCENT))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(18., 18.), Sense::hover());
                                icon(ui.painter(), r, Icon::Check, ACCENT);
                                ui.label(RichText::new("Copied to clipboard").strong());
                            });
                        });
                });
            ctx.request_repaint_after(remaining);
        } else {
            self.copied_until = None;
        }
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "SSHawty",
        eframe::NativeOptions {
            #[cfg(target_os = "linux")]
            event_loop_builder: Some(Box::new(|builder| {
                use winit::platform::x11::EventLoopBuilderExtX11;
                builder.with_x11();
            })),
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([960.0, 780.0])
                .with_min_inner_size([520.0, 640.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
