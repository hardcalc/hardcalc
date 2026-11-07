//! Calculation reports: the sheet typeset with Typst.
//!
//! The Typst source is always written from the syntax tree
//! ([`crate::sheet::notation`]). Turning it into a PDF needs the Typst
//! compiler: built in with the cargo feature `typst`, otherwise the `typst`
//! program if it is installed.

use crate::engine::Workbook;
use crate::model::{Block, Document};
use std::path::Path;

/// Title of the report: the document's first Markdown heading, else the file name.
pub fn title(doc: &Document, path: &Path) -> String {
    doc.blocks
        .iter()
        .find_map(|b| match b {
            Block::Text(lines) => lines.iter().find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string())),
            _ => None,
        })
        .unwrap_or_else(|| {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            crate::storage::document_stem(&name).to_string()
        })
}

/// The Typst source of the report.
pub fn typst_source(doc: &Document, wb: &mut Workbook, title: &str) -> String {
    let out = crate::sheet::evaluate_document(doc, wb);
    crate::sheet::notation::typst_report(title, &out)
}

/// Compiles Typst source to PDF with the built-in compiler.
#[cfg(feature = "typst")]
pub fn pdf(source: &str) -> Result<Vec<u8>, String> {
    use typst::diag::{FileError, FileResult};
    use typst::foundations::{Bytes, Datetime};
    use typst::layout::PagedDocument;
    use typst::syntax::{FileId, Source, VirtualPath};
    use typst::text::{Font, FontBook};
    use typst::utils::LazyHash;
    use typst::{Library, LibraryExt, World};

    struct ReportWorld {
        library: LazyHash<Library>,
        book: LazyHash<FontBook>,
        fonts: Vec<Font>,
        main: Source,
    }

    impl World for ReportWorld {
        fn library(&self) -> &LazyHash<Library> {
            &self.library
        }
        fn book(&self) -> &LazyHash<FontBook> {
            &self.book
        }
        fn main(&self) -> FileId {
            self.main.id()
        }
        fn source(&self, id: FileId) -> FileResult<Source> {
            if id == self.main.id() { Ok(self.main.clone()) } else { Err(FileError::NotFound(id.vpath().as_rootless_path().into())) }
        }
        fn file(&self, id: FileId) -> FileResult<Bytes> {
            Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
        }
        fn font(&self, index: usize) -> Option<Font> {
            self.fonts.get(index).cloned()
        }
        fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
            None
        }
    }

    let fonts: Vec<Font> = typst_assets::fonts().flat_map(|data| Font::iter(Bytes::new(data))).collect();
    let world = ReportWorld {
        library: LazyHash::new(Library::default()),
        book: LazyHash::new(FontBook::from_fonts(&fonts)),
        main: Source::new(FileId::new(None, VirtualPath::new("report.typ")), source.to_string()),
        fonts,
    };
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| errors.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))?;
    typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|errors| errors.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))
}

/// Compiles Typst source to PDF: the built-in compiler, else the `typst` program.
pub fn compile(source: &str, out: &Path) -> Result<(), String> {
    #[cfg(feature = "typst")]
    {
        let bytes = pdf(source)?;
        std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))
    }
    #[cfg(not(feature = "typst"))]
    {
        let typ = out.with_extension("typ");
        std::fs::write(&typ, source).map_err(|e| format!("{}: {e}", typ.display()))?;
        let status = std::process::Command::new("typst").arg("compile").arg(&typ).arg(out).status();
        match status {
            Ok(s) if s.success() => {
                let _ = std::fs::remove_file(&typ);
                Ok(())
            }
            Ok(_) => Err(format!("typst could not compile {}", typ.display())),
            Err(_) => Err(format!(
                "no Typst compiler: wrote {} – install typst (typst.app) or build hardcalc with --features typst",
                typ.display()
            )),
        }
    }
}
