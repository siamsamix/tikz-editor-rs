//! Editable document: source text, compiled scene, diagnostics and file I/O.
//!
//! Kept free of egui so the compile/refresh policy is unit-testable and the
//! UI only has to call [`Document::sync`] after editing.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tikz_core::render::{compile_with_warnings, Diagnostic};
use tikz_core::scene::Scene;

/// One open `.tex` document.
#[derive(Debug, Clone)]
pub struct Document {
    /// Current editor text.
    pub source: String,
    /// File the document was loaded from or last saved to.
    pub path: Option<PathBuf>,
    /// Last successful compilation, `None` when the source has errors.
    pub scene: Option<Scene>,
    /// Error-severity diagnostics from the latest compile.
    pub diagnostics: Vec<Diagnostic>,
    /// Warnings from the latest compile (rendering still succeeded).
    pub warnings: Vec<Diagnostic>,
    /// Source text that produced `scene`/`diagnostics`.
    compiled_from: String,
    /// Whether the text has changed since the last open/save.
    pub modified: bool,
}

impl Default for Document {
    fn default() -> Self {
        Self::new("")
    }
}

impl Document {
    /// Create a document from source text and compile it immediately.
    pub fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        let mut doc = Self {
            source,
            path: None,
            scene: None,
            diagnostics: Vec::new(),
            warnings: Vec::new(),
            compiled_from: String::new(),
            modified: false,
        };
        doc.sync();
        doc
    }

    /// Empty document carrying the supported-syntax template.
    pub fn template() -> Self {
        Self::new(
            "\\begin{tikzpicture}\n  % This slice renders bare \\draw polylines only.\n  \\draw (0,0) -- (4,0) -- (4,2) -- (0,2) -- (0,0);\n  \\draw (0,0) -- (4,2);\n\\end{tikzpicture}\n",
        )
    }

    /// Load a document from disk.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let source = fs::read_to_string(path)?;
        let mut doc = Self {
            source,
            path: Some(path.to_path_buf()),
            scene: None,
            diagnostics: Vec::new(),
            warnings: Vec::new(),
            compiled_from: String::new(),
            modified: false,
        };
        doc.sync();
        Ok(doc)
    }

    /// Replace this document with the file at `path`.
    pub fn load_into(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let next = Self::open(path)?;
        *self = next;
        Ok(())
    }

    /// Write the current text to `path` and adopt it as the document path.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        fs::write(path, &self.source)?;
        self.path = Some(path.to_path_buf());
        self.modified = false;
        Ok(())
    }

    /// Recompile when the text changed since the last compile.
    ///
    /// Returns `true` when a compile actually ran, so the UI can refit the
    /// camera only on meaningful changes.
    pub fn sync(&mut self) -> bool {
        if self.source == self.compiled_from {
            return false;
        }
        match compile_with_warnings(&self.source) {
            Ok((scene, warnings)) => {
                self.scene = Some(scene);
                self.diagnostics.clear();
                self.warnings = warnings;
            }
            Err(diagnostics) => {
                self.scene = None;
                self.diagnostics = diagnostics;
                self.warnings.clear();
            }
        }
        self.compiled_from = self.source.clone();
        true
    }

    /// Number of lines in the source (a trailing newline does not add one).
    pub fn line_count(&self) -> usize {
        self.source.lines().count().max(1)
    }

    /// Display name for the window title / toolbar.
    pub fn display_name(&self) -> String {
        match &self.path {
            Some(path) => path
                .file_name()
                .map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned()),
            None => "untitled.tex".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "\\begin{tikzpicture}\n\\draw (0,0) -- (1,1);\n\\end{tikzpicture}";

    #[test]
    fn fresh_document_compiles_without_diagnostics() {
        let doc = Document::new(MINIMAL);
        assert!(doc.diagnostics.is_empty());
        assert!(doc.warnings.is_empty());
        assert_eq!(doc.scene.as_ref().map_or(0, |s| s.len()), 1);
        assert_eq!(doc.line_count(), 3);
    }

    #[test]
    fn warnings_keep_the_scene_and_errors_drop_it() {
        let doc = Document::new(
            "\\begin{tikzpicture}\\draw (0,0) -- (1,1) node[above] {A};\\end{tikzpicture}",
        );
        assert!(doc.scene.is_some());
        assert_eq!(doc.warnings.len(), 1);
        assert!(doc.diagnostics.is_empty());

        let doc = Document::new("\\begin{tikzpicture}\\draw (A) -- (B);\\end{tikzpicture}");
        assert!(doc.scene.is_none());
        assert!(!doc.diagnostics.is_empty());
        assert!(doc.warnings.is_empty());
    }

    #[test]
    fn sync_reports_changes_only_once() {
        let mut doc = Document::new(MINIMAL);
        assert!(!doc.sync());
        doc.source.push_str("\n% note");
        assert!(doc.sync());
        assert!(!doc.sync());
        assert!(doc.scene.is_some());
    }

    #[test]
    fn unsupported_source_keeps_diagnostics_and_drops_scene() {
        let mut doc = Document::new("\\begin{tikzpicture}\\draw (A) -- (B);\\end{tikzpicture}");
        assert!(doc.scene.is_none());
        assert!(!doc.diagnostics.is_empty());
        doc.source = MINIMAL.into();
        assert!(doc.sync());
        assert!(doc.scene.is_some());
        assert!(doc.diagnostics.is_empty());
    }

    #[test]
    fn save_as_roundtrips_through_disk() {
        let dir = std::env::temp_dir().join(format!("tikz-app-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.tex");
        let mut doc = Document::new(MINIMAL);
        doc.modified = true;
        doc.save_as(&path).unwrap();
        assert!(!doc.modified);
        assert_eq!(doc.path.as_deref(), Some(path.as_path()));
        let loaded = Document::open(&path).unwrap();
        assert_eq!(loaded.source, MINIMAL);
        assert!(!loaded.modified);
        assert_eq!(loaded.display_name(), "doc.tex");
        fs::remove_dir_all(&dir).unwrap();
    }
}