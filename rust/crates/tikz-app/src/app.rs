//! egui shell: source editor, canvas, inspector and diagnostics.
//!
//! The shell is deliberately thin: it owns UI state (camera, selection,
//! notices) and delegates parsing to [`tikz_core::render::compile`] through
//! [`Document`]. No editing action rewrites TikZ yet; this slice is the
//! view/render loop the edit-command layer will plug into.

use std::path::PathBuf;

use egui::{Color32, RichText};
use tikz_core::scene::{PathStyle, SceneElement};
use tikz_core::units::Vec2;

use crate::camera::Camera;
use crate::canvas::{self, CanvasOutcome};
use crate::document::Document;

/// Accent used for headings and the modified marker.
const ACCENT: Color32 = Color32::from_rgb(24, 108, 214);
/// Error text color for diagnostics and failed file operations.
const ERROR: Color32 = Color32::from_rgb(178, 40, 40);
/// Success/info text color.
const OK: Color32 = Color32::from_rgb(28, 120, 62);
/// Warning text color for degraded rendering.
const WARN: Color32 = Color32::from_rgb(176, 112, 12);

/// Human-readable kind of a scene element.
fn element_kind(element: &tikz_core::scene::SceneElement) -> &'static str {
    match element {
        tikz_core::scene::SceneElement::Path(_) => "path",
        tikz_core::scene::SceneElement::Ellipse(_) => "ellipse",
        tikz_core::scene::SceneElement::Text(_) => "text",
    }
}

/// Fill attribute of a scene element (text elements carry their own style).
fn element_fill(element: &tikz_core::scene::SceneElement) -> String {
    match element {
        tikz_core::scene::SceneElement::Path(path) => path.style.fill.as_attr(),
        tikz_core::scene::SceneElement::Ellipse(ellipse) => ellipse.style.fill.as_attr(),
        tikz_core::scene::SceneElement::Text(text) => text.style.fill.as_attr(),
    }
}

/// Transient message shown in the diagnostics panel.
#[derive(Debug, Clone)]
struct Notice {
    text: String,
    error: bool,
}

impl Notice {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
        }
    }
}

/// Top-level application state.
pub struct TikzApp {
    doc: Document,
    camera: Camera,
    selection: Option<usize>,
    hovered: Option<usize>,
    cursor_world: Option<Vec2>,
    /// Set when the next frame should frame the scene.
    needs_fit: bool,
    path_input: String,
    notice: Option<Notice>,
    window_title: String,
}

impl TikzApp {
    /// Build the app, opening `initial` when provided.
    pub fn new(initial: Option<PathBuf>) -> Self {
        let (doc, notice) = match initial {
            Some(path) => match Document::open(&path) {
                Ok(doc) => {
                    let notice = Notice::ok(format!("opened {}", path.display()));
                    (doc, Some(notice))
                }
                Err(error) => (
                    Document::template(),
                    Some(Notice::error(format!("{}: {error}", path.display()))),
                ),
            },
            None => (Document::template(), None),
        };
        let path_input = doc
            .path
            .as_ref()
            .map_or_else(String::new, |p| p.display().to_string());
        Self {
            doc,
            camera: Camera::default(),
            selection: None,
            hovered: None,
            cursor_world: None,
            needs_fit: true,
            path_input,
            notice,
            window_title: String::new(),
        }
    }

    /// Replace the current document with a fresh template.
    fn new_document(&mut self) {
        self.doc = Document::template();
        self.path_input.clear();
        self.selection = None;
        self.needs_fit = true;
        self.notice = Some(Notice::ok("new document"));
    }

    /// Open a path chosen outside the toolbar (CLI arg, drag and drop).
    fn open_path(&mut self, path: PathBuf) {
        match Document::open(&path) {
            Ok(doc) => {
                self.path_input = doc
                    .path
                    .as_ref()
                    .map_or_else(String::new, |p| p.display().to_string());
                self.doc = doc;
                self.selection = None;
                self.needs_fit = true;
                self.notice = Some(Notice::ok(format!("opened {}", path.display())));
            }
            Err(error) => {
                self.notice = Some(Notice::error(format!("{}: {error}", path.display())));
            }
        }
    }

    /// Open whatever path the toolbar field contains.
    fn open_from_input(&mut self) {
        let text = self.path_input.trim();
        if text.is_empty() {
            self.notice = Some(Notice::error("enter a file path to open"));
            return;
        }
        self.open_path(PathBuf::from(text));
    }

    /// Save to the current path, or to the toolbar field when untitled.
    fn save(&mut self) {
        match self.doc.path.clone() {
            Some(path) => self.save_to(path),
            None => self.save_as_input(),
        }
    }

    /// Save to the toolbar field path.
    fn save_as_input(&mut self) {
        let text = self.path_input.trim();
        if text.is_empty() {
            self.notice = Some(Notice::error("enter a file path to save"));
            return;
        }
        self.save_to(PathBuf::from(text));
    }

    fn save_to(&mut self, path: PathBuf) {
        match self.doc.save_as(&path) {
            Ok(()) => {
                self.path_input = path.display().to_string();
                self.notice = Some(Notice::ok(format!("saved {}", path.display())));
            }
            Err(error) => {
                self.notice = Some(Notice::error(format!("{}: {error}", path.display())));
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let save = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        let new = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N));
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if save {
            self.save();
        }
        if new {
            self.new_document();
        }
        if escape {
            self.selection = None;
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped.into_iter().find_map(|file| file.path) {
            self.open_path(path);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                self.new_document();
            }
            ui.separator();
            let width = (ui.available_width() - 230.0).max(180.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.path_input)
                    .desired_width(width)
                    .hint_text("/path/to/document.tex"),
            );
            if ui.button("Open").clicked() {
                self.open_from_input();
            }
            if ui.button("Save").clicked() {
                self.save();
            }
            ui.separator();
            if ui.button("Fit view").clicked() {
                self.needs_fit = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("zoom ×{:.2}", self.camera.zoom));
                ui.separator();
                if self.doc.modified {
                    ui.label(RichText::new("● modified").color(ACCENT));
                }
                ui.label(RichText::new(self.doc.display_name()).strong());
            });
        });
    }

    fn source_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Source");
            ui.label(RichText::new(format!("{} lines", self.doc.line_count())).weak());
        });
        ui.separator();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = ui.available_width().max(320.0);
                let response = ui.add(
                    egui::TextEdit::multiline(&mut self.doc.source)
                        .code_editor()
                        .desired_width(width)
                        .desired_rows(36),
                );
                if response.changed() {
                    self.doc.modified = true;
                }
            });
    }

    fn inspector_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Inspector");
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("document_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("File");
                        ui.label(self.doc.display_name());
                        ui.end_row();

                        ui.label("Lines");
                        ui.label(self.doc.line_count().to_string());
                        ui.end_row();

                        ui.label("Elements");
                        let elements = self.doc.scene.as_ref().map_or(0, |scene| scene.len());
                        ui.label(elements.to_string());
                        ui.end_row();

                        ui.label("Errors");
                        let color = if self.doc.diagnostics.is_empty() {
                            OK
                        } else {
                            ERROR
                        };
                        ui.label(
                            RichText::new(self.doc.diagnostics.len().to_string()).color(color),
                        );
                        ui.end_row();

                        ui.label("Warnings");
                        let color = if self.doc.warnings.is_empty() {
                            OK
                        } else {
                            WARN
                        };
                        ui.label(RichText::new(self.doc.warnings.len().to_string()).color(color));
                        ui.end_row();

                        ui.label("Bounds");
                        let bounds = self.doc.scene.as_ref().and_then(|scene| scene.bounds());
                        ui.label(bounds.map_or_else(
                            || "—".to_string(),
                            |(lo, hi)| format!("{:.2} × {:.2} pt", hi.x - lo.x, hi.y - lo.y),
                        ));
                        ui.end_row();

                        ui.label("Cursor");
                        ui.label(self.cursor_world.map_or_else(
                            || "—".to_string(),
                            |p| format!("{:.2}, {:.2} pt", p.x, p.y),
                        ));
                        ui.end_row();
                    });

                ui.add_space(8.0);
                if ui.button("Copy SVG").clicked() {
                    match tikz_core::render::render_tex_with_warnings(&self.doc.source) {
                        Ok(rendered) => {
                            ctx.copy_text(rendered.svg);
                            let suffix = if rendered.warnings.is_empty() {
                                String::new()
                            } else {
                                format!(" ({} warning(s))", rendered.warnings.len())
                            };
                            self.notice =
                                Some(Notice::ok(format!("SVG copied to clipboard{suffix}")));
                        }
                        Err(errors) => {
                            self.notice = Some(Notice::error(format!(
                                "cannot copy SVG: {} error(s)",
                                errors.len()
                            )));
                        }
                    }
                }

                ui.separator();
                ui.label(RichText::new("Selection").strong());
                match self.selection.and_then(|index| {
                    self.doc
                        .scene
                        .as_ref()
                        .and_then(|scene| scene.elements.get(index))
                        .map(|element| (index, element))
                }) {
                    Some((index, element)) => {
                        egui::Grid::new("selection_grid")
                            .num_columns(2)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Index");
                                ui.label(index.to_string());
                                ui.end_row();

                                ui.label("Kind");
                                ui.label(element_kind(element));
                                ui.end_row();

                                ui.label("Source id");
                                ui.label(RichText::new(element.source_id()).monospace());
                                ui.end_row();

                                ui.label("Stroke");
                                let style = match element {
                                    SceneElement::Path(path) => &path.style,
                                    SceneElement::Ellipse(ellipse) => &ellipse.style,
                                    SceneElement::Text(_) => &PathStyle::default(),
                                };
                                ui.label(
                                    RichText::new(format!(
                                        "{} / {:.2} pt",
                                        style.stroke.as_attr(),
                                        style.stroke_width
                                    ))
                                    .monospace(),
                                );
                                ui.end_row();

                                ui.label("Fill");
                                ui.label(RichText::new(element_fill(element)).monospace());
                                ui.end_row();

                                ui.label("Dash");
                                ui.label(match &style.dash {
                                    None => "solid".to_string(),
                                    Some(values) => values
                                        .iter()
                                        .map(|value| format!("{value:.2}"))
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                });
                                ui.end_row();

                                ui.label("Hovered");
                                ui.label(if self.hovered == Some(index) {
                                    "yes"
                                } else {
                                    "no"
                                });
                                ui.end_row();
                            });
                        if ui.button("Deselect").clicked() {
                            self.selection = None;
                        }
                    }
                    None => {
                        ui.label(RichText::new("Click a path on the canvas to inspect it.").weak());
                    }
                }

                let hovered = self.hovered.and_then(|index| {
                    self.doc
                        .scene
                        .as_ref()
                        .and_then(|scene| scene.elements.get(index))
                });
                if let Some(element) = hovered {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Under pointer").weak());
                    ui.label(RichText::new(element.source_id()).monospace());
                }
            });
    }

    fn diagnostics_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Diagnostics");
            let error_color = if self.doc.diagnostics.is_empty() {
                OK
            } else {
                ERROR
            };
            ui.label(
                RichText::new(format!("{} error(s)", self.doc.diagnostics.len()))
                    .color(error_color),
            );
            let warn_color = if self.doc.warnings.is_empty() {
                OK
            } else {
                WARN
            };
            ui.label(
                RichText::new(format!("{} warning(s)", self.doc.warnings.len())).color(warn_color),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(notice) = &self.notice {
                    let color = if notice.error { ERROR } else { OK };
                    ui.label(RichText::new(&notice.text).color(color));
                }
            });
        });
        ui.separator();
        if self.doc.diagnostics.is_empty() && self.doc.warnings.is_empty() {
            ui.label(
                RichText::new("No diagnostics. Unsupported TikZ is reported here, never dropped.")
                    .weak(),
            );
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for diagnostic in &self.doc.diagnostics {
                    let (line, col) = diagnostic.line_col(&self.doc.source);
                    ui.label(
                        RichText::new(format!("{line}:{col}  error: {}", diagnostic.message))
                            .monospace()
                            .color(ERROR),
                    );
                }
                for warning in &self.doc.warnings {
                    let (line, col) = warning.line_col(&self.doc.source);
                    ui.label(
                        RichText::new(format!("{line}:{col}  warning: {}", warning.message))
                            .monospace()
                            .color(WARN),
                    );
                }
            });
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        if self.needs_fit {
            match self.doc.scene.as_ref().and_then(|scene| scene.bounds()) {
                Some(bounds) => self.camera.fit(bounds, rect),
                None => self.camera = Camera::default(),
            }
            self.needs_fit = false;
        }
        let outcome: CanvasOutcome = canvas::show(
            ui,
            &mut self.camera,
            self.doc.scene.as_ref(),
            self.selection,
        );
        if let Some(index) = outcome.clicked {
            self.selection = Some(index);
        } else if outcome.clicked_empty {
            self.selection = None;
        }
        self.hovered = outcome.hovered;
        self.cursor_world = outcome.cursor_world;

        if let Some(scene) = self.doc.scene.as_ref() {
            if self.selection.is_some_and(|index| index >= scene.len()) {
                self.selection = None;
            }
        } else {
            self.selection = None;
        }
    }
}

impl eframe::App for TikzApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ctx);
        self.handle_dropped_files(ctx);
        if self.doc.sync() {
            self.selection = None;
        }

        let title = format!(
            "{}{} — TikZ Editor",
            if self.doc.modified { "● " } else { "" },
            self.doc.display_name()
        );
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(2.0);
            self.toolbar(ui);
            ui.add_space(2.0);
        });
        egui::TopBottomPanel::bottom("diagnostics")
            .resizable(true)
            .default_height(120.0)
            .show(ctx, |ui| self.diagnostics_panel(ui));
        egui::SidePanel::left("source")
            .resizable(true)
            .default_width(460.0)
            .min_width(240.0)
            .show(ctx, |ui| self.source_panel(ui));
        egui::SidePanel::right("inspector")
            .resizable(true)
            .default_width(260.0)
            .min_width(200.0)
            .show(ctx, |ui| self.inspector_panel(ui, ctx));
        egui::CentralPanel::default().show(ctx, |ui| self.central_panel(ui));
    }
}
