//! A markdown document with every file it embeds, as one unit, and its forms on disk.
//!
//! A [`BundledDocument`] holds a document and the file of each image embed in it,
//! `![alt](path)` or `![[name]]`, by its path from the document's folder. An embedded note's own
//! embeds come with it. Links do not travel, nor do embeds inside code blocks; embeds inside
//! block quotes do. A `![[name]]` with no extension names `name.md`. [`images_mut`] changes
//! every image target, a block quote's included, as when a note's pictures move into `media/`.
//!
//! [`BundledDocument::from_file`] and [`BundledDocument::to_file`] read and write a bundle
//! through an os-shim [`System`]: the real disk, or the in-memory `MemorySystem`.
//! [`BundledDocument::from_zip`] and [`BundledDocument::to_zip`] read and write a zip of
//! `index.md` and the files. [`BundledDocument::embedded`] gives the tree with each file inside
//! its embed as a data URI, which [`BundledDocument::new`] turns back into files. The
//! [`forms`](crate::forms) module puts a bundle into each of arky's forms. [`embeds`],
//! [`embedded_paths`] and [`path_of`] say what a document embeds and where each file is, and
//! [`media_type`] gives the media type a data URI names a file with.
//!
//! ```rust
//! use std::path::Path;
//!
//! use arky::bundle::{BundledDocument, Missing};
//! use arky::writer::WriteOptions;
//! use os_shim::System as _;
//! use os_shim::mock::MemorySystem;
//!
//! let report = b"# Report\n\n![Logo](media/logo.gif)\n\n[spec](spec.pdf)\n";
//! let notes = MemorySystem::new()
//!     .with_file("/notes/report.md", report)?
//!     .with_file("/notes/media/logo.gif", b"GIF89a")?
//!     .with_file("/notes/spec.pdf", b"%PDF")?;
//! let path = Path::new("/notes/report.md");
//! let bundle = BundledDocument::from_file(&notes, path, Missing::Refuse)?;
//! assert_eq!(bundle.files(), ["media/logo.gif"]);
//!
//! let mut zip = Vec::new();
//! bundle.to_zip(&mut zip, &WriteOptions::default())?;
//! let back = BundledDocument::from_zip(&mut zip.as_slice(), Missing::Refuse)?;
//! assert_eq!(back, bundle);
//!
//! let copy = MemorySystem::new();
//! back.to_file(&copy, Path::new("/copy/report.md"), &WriteOptions::default())?;
//! assert_eq!(copy.read_to_string(Path::new("/copy/media/logo.gif"))?, "GIF89a");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use alloc::collections::BTreeMap;
use core::fmt::{self, Write as _};
use core::slice;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use os_shim::archive::{ZipOptions, to_zip_writer};
use os_shim::mock::MemorySystem;
use os_shim::{System, WriteMode};
use sha2::{Digest as _, Sha256};

use crate::parser::ast::{
    Block, Document, Image, Inline, Line, LinkForm, LinkInline, MarkdownBlock, Paragraph,
    RawDocument, Section,
};
#[cfg(feature = "binary-format")]
use crate::parser::mirror::BundledDocumentMirror;
use crate::parser::parse;
use crate::writer::{self, WriteError, WriteOptions};

/// A markdown document and every file it embeds, an embedded note's own embeds included,
/// each at its path from the document's folder.
///
/// Every way of making one goes through [`BundledDocument::new`], so a bundle holds no file its
/// document does not embed.
#[derive(Debug, Clone, PartialEq)]
pub struct BundledDocument {
    document: RawDocument,
    files: BTreeMap<String, Vec<u8>>,
}

/// Why a bundle could not be made, read or written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BundledDocumentError {
    /// An embed's target, `target`, leaves the document's folder, as `../logo.png` does.
    #[error("index.md names {target}, which leaves the bundle's folder")]
    Escapes { target: String },
    /// A file or a zip did not read or write.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The bundle holds no file for the embed of `target`, or `target` names no file a bundle
    /// can hold.
    #[error("index.md names {target}, and the bundle does not hold it")]
    Missing { target: String },
    /// A zip holds no `index.md`, at its root or in its one top folder.
    #[error("the bundle holds no index.md")]
    NoIndex,
    /// The base64 of the data URI `target` does not decode.
    #[error("index.md names {target}, whose base64 does not decode")]
    NotBase64 {
        target: String,
        #[source]
        source: base64::DecodeError,
    },
    /// The data URI `target` has no comma before its data.
    #[error("index.md names {target}, a data URI with no comma before its data")]
    NotDataUri { target: String },
    /// A file given to [`BundledDocument::new`] is named `name`, which is not a path inside the
    /// bundle: it is absolute, holds an empty, `.` or `..` part or a `\`, or is `index.md`.
    #[error("{name} is not a path inside the bundle")]
    NotRelative { name: String },
    /// The markdown file at `path` is not UTF-8 from byte `offset`.
    #[error("{path} is not UTF-8 at byte {offset}")]
    NotUtf8 { path: String, offset: usize },
    /// A tree whose embeds changed did not write.
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// What a bundle does with an embed whose file is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Missing {
    /// Carry no file for it, and keep its target as written.
    Keep,
    /// Fail with `BundledDocumentError::Missing`, or `BundledDocumentError::Escapes` for a target
    /// that leaves the document's folder.
    Refuse,
}

/// An embed as written: `![alt](target)` or `![[target|alt]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Embed {
    /// The text in brackets, or the wiki alias; empty when none.
    pub alt: String,
    /// How the embed is written: `![alt](target)` or `![[target|alt]]`.
    pub form: LinkForm,
    /// The target as written, escapes and references resolved.
    pub target: String,
}

/// What a data URI carries: its bytes, its media type, and its `name` when that is UTF-8 once
/// percent-decoded.
struct Data {
    bytes: Vec<u8>,
    media: String,
    name: Option<String>,
}

/// What `found_in` finds: an embed, or the text of a block quote, which the tree keeps unread.
enum Found {
    Embed(Embed),
    Quote(String),
}

/// A file of the bundle that an embed names: its bytes, its path in the bundle, and its path
/// from the folder of the text that holds the embed.
struct Held<'bundle> {
    bytes: &'bundle [u8],
    path: String,
    relative: String,
}

/// A node `found_in` has still to walk.
enum Node<'tree> {
    Block(&'tree Block),
    Body(&'tree MarkdownBlock),
    Inline(&'tree Inline),
    LinkInline(&'tree LinkInline),
    Section(&'tree Section),
}

/// A node `places_mut` has still to walk.
enum NodeMut<'tree> {
    Block(&'tree mut Block),
    Body(&'tree mut MarkdownBlock),
    Inline(&'tree mut Inline),
    LinkInline(&'tree mut LinkInline),
    Section(&'tree mut Section),
}

/// A text `embedded` writes: the note at `path`, or the document when `path` is empty, the
/// notes it embeds still to be written, and the bytes of each one written.
struct Note {
    document: RawDocument,
    path: String,
    pending: Vec<(String, RawDocument)>,
    written: BTreeMap<String, Vec<u8>>,
}

/// What `places_mut` finds: an image, or the text of a block quote.
enum Place<'tree> {
    Image(&'tree mut Image),
    Quote(&'tree mut String),
}

/// A block quote's text as parsed, and the tree `images_mut` changes.
struct Quote {
    before: RawDocument,
    document: RawDocument,
}

/// What an embed's target names, before it is resolved from the folder of the text that holds it.
enum Relative {
    /// A web address, a data URI, an absolute path or no path at all.
    NoFile,
    /// A path holding a `\`, which a zip reads as a separator and a store refuses: a file no
    /// bundle can hold.
    NotPortable,
    /// A path that is not UTF-8 once percent-decoded: a file no bundle can hold.
    NotText,
    Path(String),
}

/// What an embed's target names in the bundle, resolved from the folder of the text that holds it.
#[derive(Clone, PartialEq, Eq)]
enum Resolved {
    /// A path that leaves the bundle's folder: a file no bundle can hold.
    Escapes,
    /// A path holding a `\`, which a zip reads as a separator and a store refuses: a file no
    /// bundle can hold.
    NotPortable,
    /// A path that is not UTF-8 once percent-decoded: a file no bundle can hold.
    NotText,
    Path(String),
}

impl BundledDocument {
    /// The document's file in a zip.
    pub const INDEX: &'static str = "index.md";

    /// The document. Each embed's target names its file's path, never a data URI.
    #[must_use]
    pub const fn document(&self) -> &RawDocument {
        &self.document
    }

    /// The tree with each file written into its embed as a data URI that carries the file's
    /// path, from the folder of the text that embeds it, as its `name`. An embedded note is
    /// written with its own embeds as data URIs first.
    ///
    /// # Errors
    ///
    /// The writer's error for a block quote or an embedded note whose changed tree does not
    /// write.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::BTreeMap;
    ///
    /// use arky::bundle::{BundledDocument, Missing};
    /// use arky::parser::parse;
    /// use arky::writer::{WriteOptions, write};
    ///
    /// let document = parse("# Recipe\n\n![Loaf](images/loaf.gif)\n").document;
    /// let files = BTreeMap::from([("images/loaf.gif".to_owned(), b"GIF89a".to_vec())]);
    /// let bundle = BundledDocument::new(document, files, Missing::Refuse)?;
    /// assert_eq!(
    ///     write(&bundle.embedded()?, &WriteOptions::default())?,
    ///     "# Recipe\n\n![Loaf](data:image/gif;name=images/loaf.gif;base64,R0lGODlh)\n",
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn embedded(&self) -> Result<RawDocument, BundledDocumentError> {
        let mut root = self.note(String::new(), self.document.clone(), &[])?;
        let mut stack: Vec<Note> = Vec::new();
        loop {
            let top = stack.last_mut().unwrap_or(&mut root);
            if let Some((path, document)) = top.pending.pop() {
                let open: Vec<&str> = stack.iter().map(|note| note.path.as_str()).collect();
                let note = self.note(path, document, &open)?;
                stack.push(note);
            } else if let Some(mut done) = stack.pop() {
                self.write_files_into(&mut done)?;
                let text = writer::write(&done.document, &WriteOptions::default())?;
                stack
                    .last_mut()
                    .unwrap_or(&mut root)
                    .written
                    .insert(done.path, text.into_bytes());
            } else {
                break;
            }
        }
        self.write_files_into(&mut root)?;
        Ok(root.document)
    }

    /// The bytes of the file at `path`, as `files` lists it.
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// The file of the bundle that `embed`, in the text at `folder`, names.
    fn file_of(
        &self,
        folder: &str,
        embed: &Embed,
    ) -> Result<Option<Held<'_>>, BundledDocumentError> {
        let relative = match relative_of(embed) {
            Relative::NoFile | Relative::NotPortable | Relative::NotText => return Ok(None),
            Relative::Path(relative) => relative,
        };
        let path = match joined(folder, &relative, &embed.target) {
            Ok(Some(path)) => path,
            Ok(None) | Err(BundledDocumentError::Escapes { target: _ }) => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(self.files.get(&path).map(|bytes| Held {
            bytes,
            path,
            relative,
        }))
    }

    /// The paths of the files, sorted.
    #[must_use]
    pub fn files(&self) -> Vec<&str> {
        self.files.keys().map(String::as_str).collect()
    }

    /// The bytes of each file, by its path.
    #[cfg(feature = "binary-format")]
    pub(crate) const fn files_by_path(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.files
    }

    /// The markdown file at `path` in `system`, and every file it embeds, read from the file's
    /// folder.
    ///
    /// With os-shim's `MemorySystem`, give an absolute path: it finds a file only by the path it
    /// was stored at.
    ///
    /// # Errors
    ///
    /// A file that does not read or is not UTF-8, and the errors of [`BundledDocument::new`].
    pub fn from_file<S>(
        system: &S,
        path: &Path,
        missing: Missing,
    ) -> Result<Self, BundledDocumentError>
    where
        S: System + ?Sized,
    {
        let document = parse(&text(&path.to_string_lossy(), read(system, path)?)?).document;
        let folder = path.parent().unwrap_or_else(|| Path::new(""));
        let present = |name: &str| -> Result<Option<Vec<u8>>, BundledDocumentError> {
            let file = folder.join(name);
            if system.is_file(&file)? {
                Ok(Some(read(system, &file)?))
            } else {
                Ok(None)
            }
        };
        let mut files = BTreeMap::new();
        for name in named(&document, present)?
            .into_iter()
            .filter_map(|(_, file)| file.into_path())
        {
            if let Some(bytes) = present(&name)? {
                files.insert(name, bytes);
            }
        }
        Self::new(document, files, missing)
    }

    /// A bundle from its zip, read whole from `source`; a pipe will do.
    ///
    /// # Errors
    ///
    /// `NoIndex` when neither the zip's root nor its one top folder holds `index.md`, a zip that
    /// does not read, and the errors of [`BundledDocument::from_file`].
    pub fn from_zip(source: &mut dyn Read, missing: Missing) -> Result<Self, BundledDocumentError> {
        let mut buffer = Vec::new();
        source.read_to_end(&mut buffer)?;
        let system = MemorySystem::from_zip_reader(Cursor::new(buffer))?;
        let root = Path::new("/");
        let folder = if system.is_file(&root.join(Self::INDEX))? {
            root.to_path_buf()
        } else {
            match system.read_dir(root)?.as_slice() {
                [only] if system.is_file(&only.join(Self::INDEX))? => only.clone(),
                _ => return Err(BundledDocumentError::NoIndex),
            }
        };
        Self::from_file(&system, &folder.join(Self::INDEX), missing)
    }

    /// The tree and the bytes of each file, by its path, taken from the bundle.
    pub(crate) fn into_parts(self) -> (RawDocument, BTreeMap<String, Vec<u8>>) {
        (self.document, self.files)
    }

    /// `document` and the files of `files` it embeds, an embedded note's own embeds included.
    /// Every embed written as a data URI becomes a file first.
    ///
    /// A file no embed names is left out. A data URI's file goes at the path its `name` gives,
    /// or else at `media/` and 16 hex digits of its SHA-256, and the embed's target becomes that
    /// path. `missing` decides an embed whose file is not in `files`.
    ///
    /// # Errors
    ///
    /// `NotRelative` for a name of `files` that is not a path inside the bundle, `NotDataUri` for
    /// a data URI with no comma before its data, `NotBase64` for one whose base64 does not decode,
    /// `Escapes` for a target that leaves the document's folder and `Missing` for an embedded file
    /// `files` does not hold when `missing` is `Missing::Refuse`, `NotUtf8` for an embedded note,
    /// and the writer's error for a block quote or a note whose data URIs became files and whose
    /// tree does not write.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::BTreeMap;
    ///
    /// use arky::bundle::{BundledDocument, Missing};
    /// use arky::parser::parse;
    ///
    /// let document = parse("![Logo](data:image/png;base64,iVBORw0KGgo=)\n").document;
    /// let bundle = BundledDocument::new(document, BTreeMap::new(), Missing::Refuse)?;
    /// assert_eq!(bundle.files(), ["media/4c4b6a3be1314ab8.png"]);
    /// let png = bundle.file("media/4c4b6a3be1314ab8.png");
    /// assert_eq!(png, Some(b"\x89PNG\r\n\x1a\n".as_slice()));
    ///
    /// let text = parse("![Logo](media/logo.png)\n").document;
    /// assert!(BundledDocument::new(text.clone(), BTreeMap::new(), Missing::Refuse).is_err());
    /// let kept = BundledDocument::new(text.clone(), BTreeMap::new(), Missing::Keep)?;
    /// assert_eq!(kept.document(), &text);
    /// # Ok::<(), arky::bundle::BundledDocumentError>(())
    /// ```
    pub fn new(
        mut document: RawDocument,
        mut files: BTreeMap<String, Vec<u8>>,
        missing: Missing,
    ) -> Result<Self, BundledDocumentError> {
        if let Some(name) = files.keys().find(|name| !is_inside(name)) {
            return Err(BundledDocumentError::NotRelative { name: name.clone() });
        }
        expand(&mut document, "", &mut files)?;
        let named = named(&document, |path| Ok(files.get(path).cloned()))?;
        if missing == Missing::Refuse
            && let Some((embed, file)) = named
                .iter()
                .find(|(_, file)| !matches!(file, Resolved::Path(path) if files.contains_key(path)))
        {
            return Err(match file {
                Resolved::Escapes => BundledDocumentError::Escapes {
                    target: embed.target.clone(),
                },
                Resolved::NotPortable | Resolved::NotText | Resolved::Path(_) => {
                    BundledDocumentError::Missing {
                        target: embed.target.clone(),
                    }
                }
            });
        }
        files.retain(|path, _| {
            named
                .iter()
                .any(|(_, kept)| matches!(kept, Resolved::Path(held) if held == path))
        });
        Ok(Self { document, files })
    }

    /// `document`, the text at `path`, with each note of the bundle it embeds as still to be
    /// written, once, unless that note is at `path` or in `open`, the notes being written around
    /// it.
    fn note(
        &self,
        path: String,
        document: RawDocument,
        open: &[&str],
    ) -> Result<Note, BundledDocumentError> {
        let mut pending: Vec<(String, RawDocument)> = Vec::new();
        for embed in embeds(&document) {
            if let Some(held) = self.file_of(parent_of(&path), &embed)?
                && is_note(&held.path)
                && held.path != path
                && !open.contains(&held.path.as_str())
                && !pending.iter().any(|(known, _)| *known == held.path)
            {
                let note = parse(&text(&held.path, held.bytes.to_vec())?).document;
                pending.push((held.path, note));
            }
        }
        Ok(Note {
            document,
            path,
            pending,
            written: BTreeMap::new(),
        })
    }

    /// The tree written at `path` in `system` with `options`, and each file at its path from
    /// that folder.
    ///
    /// # Errors
    ///
    /// The writer's error, and a folder or a file that cannot be written.
    pub fn to_file<S>(
        &self,
        system: &S,
        path: &Path,
        options: &WriteOptions,
    ) -> Result<(), BundledDocumentError>
    where
        S: System + ?Sized,
    {
        let folder = path.parent().unwrap_or_else(|| Path::new(""));
        system.create_dir_all(folder)?;
        system.write(
            path,
            writer::write(&self.document, options)?.as_bytes(),
            WriteMode::Replace,
        )?;
        for (name, bytes) in &self.files {
            let file: PathBuf = folder.join(name);
            if let Some(parent) = file.parent() {
                system.create_dir_all(parent)?;
            }
            system.write(&file, bytes, WriteMode::Replace)?;
        }
        Ok(())
    }

    /// The bundle as a zip, written to `sink`: `index.md` and each file at its path.
    ///
    /// # Errors
    ///
    /// The writer's error, and a sink that cannot be written.
    pub fn to_zip(
        &self,
        sink: &mut dyn Write,
        options: &WriteOptions,
    ) -> Result<(), BundledDocumentError> {
        let system = MemorySystem::new();
        self.to_file(&system, &Path::new("/").join(Self::INDEX), options)?;
        to_zip_writer(&system, Path::new("/"), sink, ZipOptions::default())?;
        Ok(())
    }

    /// Each embed of `note` that names a file of the bundle written as that file's data URI, an
    /// embedded note's bytes taken from `written`. A note not in `written`, one being written
    /// around `note`, keeps its target.
    fn write_files_into(&self, note: &mut Note) -> Result<(), BundledDocumentError> {
        let folder = parent_of(&note.path);
        let written = &note.written;
        let mut failure = None;
        images_mut(&mut note.document, &mut |image| {
            let embed = Embed {
                alt: image.alt.clone(),
                form: image.form,
                target: image.target.clone(),
            };
            let held = match self.file_of(folder, &embed) {
                Ok(Some(held)) => held,
                Ok(None) => return,
                Err(error) => {
                    failure = Some(error);
                    return;
                }
            };
            let bytes = if is_note(&held.path) {
                let Some(written_note) = written.get(&held.path) else {
                    return;
                };
                written_note.as_slice()
            } else {
                held.bytes
            };
            image.target = format!(
                "{}{}",
                data_uri(&held.relative, bytes),
                fragment_of(&embed.target)
            );
        })?;
        failure.map_or(Ok(()), Err)
    }
}

/// Unchecked: `forms::decode` makes the bundle again with `new`.
#[cfg(feature = "binary-format")]
impl From<BundledDocumentMirror> for BundledDocument {
    fn from(mirror: BundledDocumentMirror) -> Self {
        Self {
            document: mirror.document,
            files: mirror.files,
        }
    }
}

impl Resolved {
    /// The path in the bundle, or `None` for a file no bundle can hold.
    fn into_path(self) -> Option<String> {
        match self {
            Self::Path(path) => Some(path),
            Self::Escapes | Self::NotPortable | Self::NotText => None,
        }
    }
}

/// The path of the file `target`, a `![alt](target)` target, names from the document's folder,
/// or `None` when it names none.
///
/// # Errors
///
/// `Escapes` for a path that leaves the folder, and `Missing` for one that is not UTF-8 once
/// percent-decoded.
///
/// # Examples
///
/// ```rust
/// use arky::bundle::path_of;
///
/// assert_eq!(path_of("./media/my%20logo.png?v=2")?.as_deref(), Some("media/my logo.png"));
/// assert_eq!(path_of("https://example.com/logo.png")?, None);
/// assert!(path_of("../logo.png").is_err());
/// # Ok::<(), arky::bundle::BundledDocumentError>(())
/// ```
pub fn path_of(target: &str) -> Result<Option<String>, BundledDocumentError> {
    let embed = Embed {
        alt: String::new(),
        form: LinkForm::Markdown,
        target: target.to_owned(),
    };
    match relative_of(&embed) {
        Relative::NoFile => Ok(None),
        Relative::NotPortable | Relative::NotText => Err(BundledDocumentError::Missing {
            target: target.to_owned(),
        }),
        Relative::Path(relative) => joined("", &relative, target),
    }
}

/// Each embed of `document`, `![alt](target)` and `![[target]]`, a block quote's included, in
/// document order.
///
/// Embeds inside code blocks are left out.
///
/// # Examples
///
/// ```rust
/// use arky::bundle::embeds;
/// use arky::parser::ast::LinkForm;
/// use arky::parser::parse;
///
/// let source = "![Picture](logo.png)\n\n> quoted ![[chart|Chart]]\n\n```\n![c](code.png)\n```\n";
/// let found = embeds(&parse(source).document);
/// let targets: Vec<&str> = found.iter().map(|embed| embed.target.as_str()).collect();
/// assert_eq!(targets, ["logo.png", "chart"]);
/// assert_eq!(found[1].form, LinkForm::Wiki);
/// assert_eq!(found[1].alt, "Chart");
/// ```
#[must_use]
pub fn embeds(document: &RawDocument) -> Vec<Embed> {
    let mut embeds = Vec::new();
    let mut pending: Vec<Found> = found_in(document).into_iter().rev().collect();
    while let Some(found) = pending.pop() {
        match found {
            Found::Embed(embed) => embeds.push(embed),
            Found::Quote(quoted) => {
                pending.extend(found_in(&parse(&quoted).document).into_iter().rev());
            }
        }
    }
    embeds
}

/// Gives each image of `document`, a block quote's included, to `visit`, in document order. A
/// block quote whose images `visit` changes is written again from its changed tree.
///
/// # Errors
///
/// The writer's error for a block quote whose changed tree does not write.
///
/// # Examples
///
/// ```rust
/// use arky::bundle::images_mut;
/// use arky::parser::parse;
/// use arky::writer::{WriteOptions, write};
///
/// let mut document = parse("![Logo](logo.png)\n\n> ![Chart](chart.png)\n").document;
/// images_mut(&mut document, &mut |image| image.target.insert_str(0, "media/"))?;
/// assert_eq!(
///     write(&document, &WriteOptions::default())?,
///     "![Logo](media/logo.png)\n\n> ![Chart](media/chart.png)\n",
/// );
/// # Ok::<(), arky::writer::WriteError>(())
/// ```
pub fn images_mut(
    document: &mut RawDocument,
    visit: &mut dyn FnMut(&mut Image),
) -> Result<(), WriteError> {
    let mut levels: Vec<Vec<Quote>> = Vec::new();
    let mut quoted = quotes_in(document);
    while !quoted.is_empty() {
        let level: Vec<Quote> = quoted
            .iter()
            .map(|text| {
                let parsed = parse(text).document;
                Quote {
                    before: parsed.clone(),
                    document: parsed,
                }
            })
            .collect();
        quoted = level
            .iter()
            .flat_map(|quote| quotes_in(&quote.document))
            .collect();
        levels.push(level);
    }
    visit_in_order(document, &mut levels, visit);
    write_back(document, &mut levels)
}

/// Each file that `document`, the text at `folder`, embeds, as the embed as written and the
/// file's path in the bundle, each path once, in document order.
///
/// An embedded note's own embeds are not followed, and a target that leaves the bundle's folder,
/// is not UTF-8 once percent-decoded or is a `![[target]]` holding a `\` is left out.
///
/// # Errors
///
/// None: each target names a path in the bundle or is left out.
///
/// # Examples
///
/// ```rust
/// use arky::bundle::embedded_paths;
/// use arky::parser::parse;
///
/// let note = parse("![chart](../media/chart.png)\n\n![[sub/note#Heading]]\n").document;
/// let paths: Vec<String> =
///     embedded_paths(&note, "parts")?.into_iter().map(|(_, path)| path).collect();
/// assert_eq!(paths, ["media/chart.png", "parts/sub/note.md"]);
/// # Ok::<(), arky::bundle::BundledDocumentError>(())
/// ```
pub fn embedded_paths(
    document: &RawDocument,
    folder: &str,
) -> Result<Vec<(Embed, String)>, BundledDocumentError> {
    let mut paths: Vec<(Embed, String)> = Vec::new();
    for (embed, file) in paths_in(document, folder)? {
        if let Some(path) = file.into_path()
            && !paths.iter().any(|(_, known)| *known == path)
        {
            paths.push((embed, path));
        }
    }
    Ok(paths)
}

/// The media type a data URI gives the file at `path`, by its extension.
///
/// ```rust
/// use arky::bundle::media_type;
///
/// assert_eq!(media_type("media/logo.PNG"), "image/png");
/// assert_eq!(media_type("notes/plan.md"), "text/markdown");
/// assert_eq!(media_type("spec.pdf"), "application/octet-stream");
/// ```
#[must_use]
pub fn media_type(path: &str) -> &'static str {
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("gif") => "image/gif",
        Some("jpeg" | "jpg") => "image/jpeg",
        Some("md") => "text/markdown",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Each embed of `document`, the text at `folder`, that names a file, as the embed as written
/// and what it names in the bundle, in document order.
fn paths_in(
    document: &RawDocument,
    folder: &str,
) -> Result<Vec<(Embed, Resolved)>, BundledDocumentError> {
    let mut paths = Vec::new();
    for embed in embeds(document) {
        let file = match relative_of(&embed) {
            Relative::NoFile => continue,
            Relative::NotPortable => Resolved::NotPortable,
            Relative::NotText => Resolved::NotText,
            Relative::Path(relative) => match joined(folder, &relative, &embed.target) {
                Ok(Some(path)) => Resolved::Path(path),
                Ok(None) => continue,
                Err(BundledDocumentError::Escapes { target: _ }) => Resolved::Escapes,
                Err(error) => return Err(error),
            },
        };
        paths.push((embed, file));
    }
    Ok(paths)
}

/// Every file that `document` embeds, an embedded note's own embeds included, each path once, as
/// the embed as written and what it names in the bundle. `text_of` gives an embedded note's
/// bytes, or `None` when it is not there.
fn named<F>(
    document: &RawDocument,
    mut text_of: F,
) -> Result<Vec<(Embed, Resolved)>, BundledDocumentError>
where
    F: FnMut(&str) -> Result<Option<Vec<u8>>, BundledDocumentError>,
{
    let mut named: Vec<(Embed, Resolved)> = Vec::new();
    let mut pending: Vec<(String, RawDocument)> = vec![(String::new(), document.clone())];
    while let Some((folder, note)) = pending.pop() {
        for (embed, file) in paths_in(&note, &folder)? {
            if matches!(file, Resolved::Path(_)) && named.iter().any(|(_, known)| *known == file) {
                continue;
            }
            named.push((embed, file.clone()));
            if let Some(path) = file.into_path()
                && is_note(&path)
                && let Some(bytes) = text_of(&path)?
            {
                pending.push((
                    parent_of(&path).to_owned(),
                    parse(&text(&path, bytes)?).document,
                ));
            }
        }
    }
    Ok(named)
}

/// Each embed written as a data URI, in `document`, the text at `folder`, and in every note of
/// `files` it embeds, an embedded note's own notes included, made a file of `files` as
/// `data_uri_to_file` names it. A note whose tree changed is written back into `files`.
fn expand(
    document: &mut RawDocument,
    folder: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), BundledDocumentError> {
    data_uris_to_files(document, folder, files)?;
    let mut notes = notes_of(document, folder, files)?;
    let mut expanded: Vec<String> = Vec::new();
    while let Some(path) = notes.pop() {
        if expanded.contains(&path) {
            continue;
        }
        let mut note = parse(&text(&path, files[&path].clone())?).document;
        let before = note.clone();
        data_uris_to_files(&mut note, parent_of(&path), files)?;
        notes.extend(notes_of(&note, parent_of(&path), files)?);
        if note != before {
            let written = writer::write(&note, &WriteOptions::default())?;
            files.insert(path.clone(), written.into_bytes());
        }
        expanded.push(path);
    }
    Ok(())
}

/// The path of each note of `files` that `document`, the text at `folder`, embeds.
fn notes_of(
    document: &RawDocument,
    folder: &str,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<String>, BundledDocumentError> {
    Ok(paths_in(document, folder)?
        .into_iter()
        .filter_map(|(_, file)| file.into_path())
        .filter(|path| is_note(path) && files.contains_key(path))
        .collect())
}

/// Each embed of `document`, the text at `folder`, written as a data URI made a file of
/// `files` as `data_uri_to_file` names it.
fn data_uris_to_files(
    document: &mut RawDocument,
    folder: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), BundledDocumentError> {
    let mut failure = None;
    images_mut(document, &mut |image| {
        if failure.is_none()
            && let Err(error) = data_uri_to_file(image, folder, files)
        {
            failure = Some(error);
        }
    })?;
    failure.map_or(Ok(()), Err)
}

/// The data URI `image` targets, in the text at `folder`, made a file of `files`, and its target
/// that file's path from `folder`: the data URI's `name` when `named_path` keeps it and that path
/// holds no other bytes, else the first name `hashed_name` gives, numbered from zero, whose path
/// holds no other bytes.
fn data_uri_to_file(
    image: &mut Image,
    folder: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), BundledDocumentError> {
    let Some(Data { bytes, media, name }) = data_of(&image.target)? else {
        return Ok(());
    };
    let fragment = fragment_of(&image.target);
    let kept = match name {
        Some(named) => {
            let target = target_of(image.form, &named, fragment);
            named_path(folder, image.form, &target, &media)?
                .filter(|path| files.get(path).is_none_or(|held| *held == bytes))
                .map(|path| (target, path))
        }
        None => None,
    };
    let (target, path) = kept.unwrap_or_else(|| {
        let mut number = 0_usize;
        loop {
            let hashed = hashed_name(&bytes, &media, number);
            let path = beside(folder, &hashed);
            if files.get(&path).is_none_or(|held| *held == bytes) {
                break (target_of(image.form, &hashed, fragment), path);
            }
            number += 1;
        }
    });
    image.target = target;
    files.insert(path, bytes);
    Ok(())
}

/// The path in the bundle that `target`, the target of an embed of `form` in the text at `folder`,
/// names once the embed is written and read back, for a data URI of `media` type. `None` when it
/// reads back as another target, names no file, `index.md` or a path above the bundle, or names a
/// note and `media` is not `text/markdown`.
fn named_path(
    folder: &str,
    form: LinkForm,
    target: &str,
    media: &str,
) -> Result<Option<String>, BundledDocumentError> {
    let embed = Embed {
        alt: String::new(),
        form,
        target: target.to_owned(),
    };
    if !reads_back(&embed)? {
        return Ok(None);
    }
    let relative = match relative_of(&embed) {
        Relative::NoFile | Relative::NotPortable | Relative::NotText => return Ok(None),
        Relative::Path(relative) => relative,
    };
    match joined(folder, &relative, target) {
        Ok(path) => Ok(path.filter(|resolved| {
            is_inside(resolved) && (media == "text/markdown" || !is_note(resolved))
        })),
        Err(BundledDocumentError::Escapes { target: _ }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether `embed`, written as an image alone, reads back as the same embed.
fn reads_back(embed: &Embed) -> Result<bool, WriteError> {
    let image = Image::new(embed.form, embed.target.clone(), None, embed.alt.clone());
    let line = Line::new(vec![Inline::Image(image)]);
    let section = Section::new(
        None,
        vec![Block::Paragraph(Paragraph::new(vec![line]))],
        Vec::new(),
    );
    let document: RawDocument =
        RawDocument::Valid(Document::new(MarkdownBlock::new(vec![section]), None));
    let text = writer::write(&document, &WriteOptions::default())?;
    Ok(embeds(&parse(&text).document) == slice::from_ref(embed))
}

/// The target an embed of `form` takes to name `name` from the folder of its text, followed by
/// `fragment`: percent-encoded in `![alt](target)`, as written in `![[target]]`.
fn target_of(form: LinkForm, name: &str, fragment: &str) -> String {
    match form {
        LinkForm::Markdown => format!("{}{fragment}", percent_encoded(name)),
        LinkForm::Wiki => format!("{name}{fragment}"),
    }
}

/// What the data URI `target` carries, or `None` when it is not a data URI. A `#` part after the
/// data is not part of it; `NotDataUri` names `target` when no comma comes before its data, and
/// `NotBase64` when its base64 does not decode.
fn data_of(target: &str) -> Result<Option<Data>, BundledDocumentError> {
    let uri = &target[..target.find('#').unwrap_or(target.len())];
    let Some(data) = uri
        .get(..5)
        .filter(|scheme| scheme.eq_ignore_ascii_case("data:"))
        .and_then(|_| uri.get(5..))
    else {
        return Ok(None);
    };
    let Some((header, payload)) = data.split_once(',') else {
        return Err(BundledDocumentError::NotDataUri {
            target: target.to_owned(),
        });
    };
    let mut parts = header.split(';');
    let media = parts.next().unwrap_or_default().to_ascii_lowercase();
    let parameters: Vec<&str> = parts.collect();
    let bytes = if parameters
        .last()
        .is_some_and(|last| last.eq_ignore_ascii_case("base64"))
    {
        STANDARD
            .decode(payload)
            .map_err(|source| BundledDocumentError::NotBase64 {
                target: target.to_owned(),
                source,
            })?
    } else {
        percent_decoded(payload)
    };
    let name = parameters
        .iter()
        .find_map(|parameter| {
            parameter
                .split_once('=')
                .filter(|(key, _)| key.eq_ignore_ascii_case("name"))
                .map(|(_, value)| value)
        })
        .map(percent_decoded)
        .filter(|decoded| {
            decoded
                .utf8_chunks()
                .all(|chunk| chunk.invalid().is_empty())
        })
        .map(|decoded| decoded.utf8_chunks().map(|chunk| chunk.valid()).collect());
    Ok(Some(Data { bytes, media, name }))
}

/// The extension a file of `media` type gets when its data URI carries no name.
fn extension_of(media: &str) -> &'static str {
    match media {
        "image/gif" => "gif",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/svg+xml" => "svg",
        "image/webp" => "webp",
        "text/markdown" => "md",
        _ => "bin",
    }
}

/// `media/`, the first 16 hex digits of the SHA-256 of `bytes`, `-<number>` when `number` is not
/// zero, and the extension of a file of `media` type.
fn hashed_name(bytes: &[u8], media: &str, number: usize) -> String {
    let mut name = String::from("media/");
    for byte in Sha256::digest(bytes).iter().take(8) {
        let _: fmt::Result = write!(name, "{byte:02x}");
    }
    if number != 0 {
        let _: fmt::Result = write!(name, "-{number}");
    }
    name.push('.');
    name.push_str(extension_of(media));
    name
}

/// The path in the bundle of `relative`, a path with no empty, `.` or `..` part, from `folder`.
fn beside(folder: &str, relative: &str) -> String {
    if folder.is_empty() {
        relative.to_owned()
    } else {
        format!("{folder}/{relative}")
    }
}

/// Whether the file at `path` is a note: its extension is `md`, in any case.
fn is_note(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// The path an embed names, relative to the folder of the text that holds it, not yet resolved.
/// `![alt](target)` is an address: percent-decoded, each `\` read as `/`, its `?` and `#` parts
/// cut off.
/// `![[target]]` is a name: kept as written, its `#` part cut off, and `<target>.md` when it has
/// no extension.
fn relative_of(embed: &Embed) -> Relative {
    let target = embed.target.as_str();
    let cut = match embed.form {
        LinkForm::Markdown => target.find(['?', '#']),
        LinkForm::Wiki => target.find('#'),
    };
    let reference = &target[..cut.unwrap_or(target.len())];
    if reference.is_empty() || reference.starts_with('/') || has_scheme(reference) {
        return Relative::NoFile;
    }
    match embed.form {
        LinkForm::Markdown => {
            let decoded = percent_decoded(reference);
            if decoded
                .utf8_chunks()
                .any(|chunk| !chunk.invalid().is_empty())
            {
                return Relative::NotText;
            }
            let path = decoded
                .utf8_chunks()
                .map(|chunk| chunk.valid())
                .collect::<String>()
                .replace('\\', "/");
            if path.starts_with('/') {
                Relative::NoFile
            } else {
                Relative::Path(path)
            }
        }
        LinkForm::Wiki if reference.contains('\\') => Relative::NotPortable,
        LinkForm::Wiki => {
            let named = Path::new(reference).extension().is_some();
            Relative::Path(if named {
                reference.to_owned()
            } else {
                format!("{reference}.md")
            })
        }
    }
}

/// `relative` resolved from `folder`, a path inside the bundle: empty and `.` parts dropped,
/// `..` removing the part before it, and `Escapes` naming `target` when there is none.
fn joined(
    folder: &str,
    relative: &str,
    target: &str,
) -> Result<Option<String>, BundledDocumentError> {
    let mut parts: Vec<&str> = folder.split('/').filter(|part| !part.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(BundledDocumentError::Escapes {
                        target: target.to_owned(),
                    });
                }
            }
            name => parts.push(name),
        }
    }
    Ok((!parts.is_empty()).then(|| parts.join("/")))
}

/// Whether `reference` opens with a URL scheme: a letter, then letters, digits, `+`, `-` or
/// `.`, then `:`.
fn has_scheme(reference: &str) -> bool {
    let Some(colon) = reference.find(':') else {
        return false;
    };
    let mut characters = reference[..colon].chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|next| next.is_ascii_alphanumeric() || matches!(next, '+' | '-' | '.'))
}

/// `text` with each `%` and two hex digits replaced by the byte they write.
fn percent_decoded(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let hex = |at: usize| {
        let byte = bytes.get(at)?;
        char::from(*byte).to_digit(16)
    };
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0_usize;
    while at < bytes.len() {
        if let (b'%', Some(high), Some(low)) = (bytes[at], hex(at + 1), hex(at + 2)) {
            decoded.push(u8::try_from(high * 16 + low).unwrap_or_default());
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    decoded
}

/// A data URI of `bytes` that carries `name`, with the media type of `name`'s extension.
fn data_uri(name: &str, bytes: &[u8]) -> String {
    format!(
        "data:{};name={};base64,{}",
        media_type(name),
        percent_encoded(name),
        STANDARD.encode(bytes)
    )
}

/// `path` with each byte other than a letter, a digit, `-`, `.`, `_`, `~` or `/` written as
/// `%` and two hex digits.
fn percent_encoded(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            let _: fmt::Result = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// The `#` part of `target`, `#` included, or nothing.
fn fragment_of(target: &str) -> &str {
    target.find('#').map_or("", |at| &target[at..])
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// Whether `name` is a path inside the bundle: no empty, `.` or `..` part, no leading `/`, no
/// `\`, and not the zip's own `index.md`.
fn is_inside(name: &str) -> bool {
    name != BundledDocument::INDEX
        && !name.starts_with('/')
        && !name.contains('\\')
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn text(path: &str, bytes: Vec<u8>) -> Result<String, BundledDocumentError> {
    String::from_utf8(bytes).map_err(|error| BundledDocumentError::NotUtf8 {
        path: path.to_owned(),
        offset: error.utf8_error().valid_up_to(),
    })
}

fn read<S>(system: &S, path: &Path) -> io::Result<Vec<u8>>
where
    S: System + ?Sized,
{
    let mut bytes = Vec::new();
    system.open(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The text of each block quote of `document`, in document order, a quote inside a quote left
/// in its text.
fn quotes_in(document: &RawDocument) -> Vec<String> {
    found_in(document)
        .into_iter()
        .filter_map(|found| match found {
            Found::Embed(_) => None,
            Found::Quote(quoted) => Some(quoted),
        })
        .collect()
}

/// Gives each image of `document` and of the quotes of `levels` to `visit`, in document order.
/// `levels[0]` holds the quotes of `document`, and each level the quotes of the one before it.
fn visit_in_order(
    document: &mut RawDocument,
    levels: &mut [Vec<Quote>],
    visit: &mut dyn FnMut(&mut Image),
) {
    let mut below: Vec<_> = levels
        .iter_mut()
        .map(|level| {
            level
                .iter_mut()
                .map(|quote| places_mut(&mut quote.document).into_iter())
                .collect::<Vec<_>>()
                .into_iter()
        })
        .collect();
    let mut stack = vec![(0_usize, places_mut(document).into_iter())];
    while let Some((depth, mut places)) = stack.pop() {
        let Some(place) = places.next() else {
            continue;
        };
        stack.push((depth, places));
        match place {
            Place::Image(image) => visit(image),
            // A level lists its quotes in the order a walk in document order meets them.
            Place::Quote(_) => {
                if let Some(quote) = below.get_mut(depth).and_then(Iterator::next) {
                    stack.push((depth + 1, quote));
                }
            }
        }
    }
}

/// Each quote of `levels` whose tree changed, written into the text of the block quote it was
/// read from, the deepest level first.
fn write_back(document: &mut RawDocument, levels: &mut [Vec<Quote>]) -> Result<(), WriteError> {
    for depth in (0..levels.len()).rev() {
        let (above, rest) = levels.split_at_mut(depth);
        let Some(level) = rest.first() else {
            continue;
        };
        let values: Vec<&mut String> = above.last_mut().map_or_else(
            || quote_values(document),
            |parents| {
                parents
                    .iter_mut()
                    .flat_map(|parent| quote_values(&mut parent.document))
                    .collect()
            },
        );
        for (value, quote) in values.into_iter().zip(level) {
            if quote.document != quote.before {
                writer::write(&quote.document, &WriteOptions::default())?
                    .trim_end_matches('\n')
                    .clone_into(value);
            }
        }
    }
    Ok(())
}

fn quote_values(document: &mut RawDocument) -> Vec<&mut String> {
    places_mut(document)
        .into_iter()
        .filter_map(|place| match place {
            Place::Image(_) => None,
            Place::Quote(value) => Some(value),
        })
        .collect()
}

/// The embeds and the block quotes' texts of `document`, in document order.
fn found_in(document: &RawDocument) -> Vec<Found> {
    let root = match document {
        RawDocument::Invalid(invalid) => &invalid.body,
        RawDocument::Valid(valid) => &valid.body,
    };
    let embed = |image: &Image| {
        Found::Embed(Embed {
            alt: image.alt.clone(),
            form: image.form,
            target: image.target.clone(),
        })
    };
    let mut found = Vec::new();
    let mut stack = vec![Node::Body(root)];
    while let Some(node) = stack.pop() {
        match node {
            Node::Block(block) => match block {
                Block::BlockQuote(quote) => found.push(Found::Quote(quote.value.clone())),
                Block::CodeBlock(_) | Block::Html(_) | Block::ThematicBreak(_) => {}
                Block::List(list) => {
                    stack.extend(list.items.iter().rev().map(|item| Node::Body(&item.body)));
                }
                Block::Paragraph(paragraph) => {
                    for line in paragraph.lines.iter().rev() {
                        stack.extend(line.children.iter().rev().map(Node::Inline));
                    }
                }
                Block::Table(table) => {
                    for row in table.head.iter().chain(&table.rows).rev() {
                        stack.extend(row.cells.iter().rev().map(|cell| Node::Body(&cell.body)));
                    }
                }
            },
            Node::Body(body) => stack.extend(body.sections.iter().rev().map(Node::Section)),
            Node::Inline(inline) => match inline {
                Inline::Code(_) | Inline::Html(_) | Inline::LineBreak(_) | Inline::Text(_) => {}
                Inline::Emphasis(emphasis) => {
                    stack.extend(emphasis.children.iter().rev().map(Node::Inline));
                }
                Inline::Image(image) => found.push(embed(image)),
                Inline::Link(link) => {
                    stack.extend(link.children.iter().rev().map(Node::LinkInline));
                }
                Inline::Strong(strong) => {
                    stack.extend(strong.children.iter().rev().map(Node::Inline));
                }
            },
            Node::LinkInline(inline) => match inline {
                LinkInline::Code(_)
                | LinkInline::Html(_)
                | LinkInline::LineBreak(_)
                | LinkInline::Text(_) => {}
                LinkInline::Emphasis(emphasis) => {
                    stack.extend(emphasis.children.iter().rev().map(Node::LinkInline));
                }
                LinkInline::Image(image) => found.push(embed(image)),
                LinkInline::Strong(strong) => {
                    stack.extend(strong.children.iter().rev().map(Node::LinkInline));
                }
            },
            Node::Section(section) => {
                stack.extend(section.subsections.iter().rev().map(Node::Section));
                stack.extend(section.blocks.iter().rev().map(Node::Block));
                if let Some(heading) = &section.heading {
                    stack.extend(heading.children.iter().rev().map(Node::Inline));
                }
            }
        }
    }
    found
}

/// The images and the block quotes' texts of `document`, in document order, to change in
/// place.
fn places_mut(document: &mut RawDocument) -> Vec<Place<'_>> {
    let root = match document {
        RawDocument::Invalid(invalid) => &mut invalid.body,
        RawDocument::Valid(valid) => &mut valid.body,
    };
    let mut places = Vec::new();
    let mut stack = vec![NodeMut::Body(root)];
    while let Some(node) = stack.pop() {
        match node {
            NodeMut::Block(block) => match block {
                Block::BlockQuote(quote) => places.push(Place::Quote(&mut quote.value)),
                Block::CodeBlock(_) | Block::Html(_) | Block::ThematicBreak(_) => {}
                Block::List(list) => {
                    stack.extend(
                        list.items
                            .iter_mut()
                            .rev()
                            .map(|item| NodeMut::Body(&mut item.body)),
                    );
                }
                Block::Paragraph(paragraph) => {
                    for line in paragraph.lines.iter_mut().rev() {
                        stack.extend(line.children.iter_mut().rev().map(NodeMut::Inline));
                    }
                }
                Block::Table(table) => {
                    for row in table.head.iter_mut().chain(table.rows.iter_mut()).rev() {
                        stack.extend(
                            row.cells
                                .iter_mut()
                                .rev()
                                .map(|cell| NodeMut::Body(&mut cell.body)),
                        );
                    }
                }
            },
            NodeMut::Body(body) => {
                stack.extend(body.sections.iter_mut().rev().map(NodeMut::Section));
            }
            NodeMut::Inline(inline) => match inline {
                Inline::Code(_) | Inline::Html(_) | Inline::LineBreak(_) | Inline::Text(_) => {}
                Inline::Emphasis(emphasis) => {
                    stack.extend(emphasis.children.iter_mut().rev().map(NodeMut::Inline));
                }
                Inline::Image(image) => places.push(Place::Image(image)),
                Inline::Link(link) => {
                    stack.extend(link.children.iter_mut().rev().map(NodeMut::LinkInline));
                }
                Inline::Strong(strong) => {
                    stack.extend(strong.children.iter_mut().rev().map(NodeMut::Inline));
                }
            },
            NodeMut::LinkInline(inline) => match inline {
                LinkInline::Code(_)
                | LinkInline::Html(_)
                | LinkInline::LineBreak(_)
                | LinkInline::Text(_) => {}
                LinkInline::Emphasis(emphasis) => {
                    stack.extend(emphasis.children.iter_mut().rev().map(NodeMut::LinkInline));
                }
                LinkInline::Image(image) => places.push(Place::Image(image)),
                LinkInline::Strong(strong) => {
                    stack.extend(strong.children.iter_mut().rev().map(NodeMut::LinkInline));
                }
            },
            NodeMut::Section(section) => {
                let Section {
                    blocks,
                    heading,
                    kind: _,
                    subsections,
                } = section;
                stack.extend(subsections.iter_mut().rev().map(NodeMut::Section));
                stack.extend(blocks.iter_mut().rev().map(NodeMut::Block));
                if let Some(title) = heading {
                    stack.extend(title.children.iter_mut().rev().map(NodeMut::Inline));
                }
            }
        }
    }
    places
}

#[cfg(test)]
mod tests;
