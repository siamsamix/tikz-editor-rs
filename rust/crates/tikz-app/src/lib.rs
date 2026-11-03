#![forbid(unsafe_code)]
//! `tikz-app`: the egui shell over [`tikz_core`].
//!
//! The crate is split so the UI-independent policy stays testable:
//!
//! * [`document`] — editor text, compile cache, diagnostics, file I/O;
//! * [`camera`] — pan/zoom and the world/screen mapping;
//! * [`canvas`] — scene painting, picking and pointer interaction;
//! * [`app`] — panel layout and the [`eframe::App`] implementation.
//!
//! ```
//! use tikz_app::document::Document;
//! let doc = Document::new("\\begin{tikzpicture}\\draw (0,0) -- (1,1);\\end{tikzpicture}");
//! assert!(doc.diagnostics.is_empty());
//! assert_eq!(doc.scene.unwrap().len(), 1);
//! ```

pub mod app;
pub mod camera;
pub mod canvas;
pub mod document;
