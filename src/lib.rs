//! Markdown for Flareite: highlighting and a preview.
//!
//! Highlighting needs no code: the manifest declares the Tree-sitter
//! grammars and their queries. This code is the preview:
//!
//! - **Open Preview** (Ctrl+Shift+V) shows the active Markdown document,
//!   rendered, in a tab of its own.
//! - **Open Preview to the Side** (Ctrl+K V) shows it beside the editor.
//!
//! A preview follows its document as it changes, and closes with it. The
//! page is sent as preview nodes (headings, paragraphs, lists, tables,
//! links, pictures), which the workbench draws itself. The editor turns the
//! Markdown into them (`workbench::render-markdown`), as it does for
//! extensions' READMEs.

use std::collections::BTreeSet;

use flareite_extension_api::events::Event;
use flareite_extension_api::types::{
  Activation, ApiError, CommandContext, DocumentId, DocumentInfo, LogLevel, PreviewOptions,
};
use flareite_extension_api::{Extension, host, register, workbench};

/// The preview declared in the manifest. Each document has its own, keyed
/// by the document's id.
const PREVIEW: &str = "preview";
const CHUNK_LINES: u32 = 10_000;

fn describe(error: &ApiError) -> String {
  match error {
    ApiError::PermissionDenied(_) => {
      "Markdown can't read documents; allow it in the Extensions view".into()
    }
    ApiError::UnknownDocument => "the document is no longer open".into(),
    ApiError::StaleVersion(_) => "the document changed; try again".into(),
    ApiError::InvalidRange(message)
    | ApiError::TooLarge(message)
    | ApiError::InvalidArgument(message) => message.clone(),
  }
}

/// The document's file name, for the preview's title.
fn file_name(info: &DocumentInfo) -> &str {
  info
    .path
    .as_deref()
    .and_then(|path| path.rsplit(['/', '\\']).next())
    .unwrap_or("Untitled")
}

fn read_text(info: &DocumentInfo) -> Result<String, ApiError> {
  let mut text = String::with_capacity(usize::try_from(info.length).unwrap_or(0));
  let mut start = 0;
  while start < info.line_count {
    let end = start.saturating_add(CHUNK_LINES).min(info.line_count);
    let slice = host::read_lines(info.id, start, end)?;
    if slice.lines.is_empty() {
      break;
    }
    start += u32::try_from(slice.lines.len()).unwrap_or(u32::MAX);
    for line in slice.lines {
      text.push_str(&line);
      text.push('\n');
    }
  }
  Ok(text)
}

/// Renders a document into its preview. Returns whether the preview is
/// still open: the user may have closed it.
fn update(id: DocumentId) -> Result<bool, ApiError> {
  let info = host::document(id)?;
  let nodes = workbench::render_markdown(&read_text(&info)?)?;
  workbench::set_preview_content(PREVIEW, &id.to_string(), &nodes)
}

/// The documents whose previews are open.
struct Markdown {
  previewed: BTreeSet<DocumentId>,
}

impl Markdown {
  fn refresh(&mut self, id: DocumentId) {
    if !self.previewed.contains(&id) {
      return;
    }
    match update(id) {
      Ok(true) => {}
      Ok(false) => {
        self.previewed.remove(&id);
      }
      Err(error) => host::log(
        LogLevel::Warn,
        &format!("updating the preview failed: {}", describe(&error)),
      ),
    }
  }
}

impl Extension for Markdown {
  fn activate(_: Activation) -> Result<Self, String> {
    Ok(Self {
      previewed: BTreeSet::new(),
    })
  }

  fn run_command(&mut self, command: &str, context: CommandContext) -> Result<(), String> {
    let beside = match command {
      "flareite.markdown.preview" => false,
      "flareite.markdown.previewToSide" => true,
      other => return Err(format!("unknown command {other}")),
    };
    let Some(id) = context.document else {
      return Err("Open a Markdown file to preview it.".into());
    };
    let info = host::document(id).map_err(|error| describe(&error))?;
    if info.language.as_deref() != Some("markdown") {
      return Err(format!("{} isn't a Markdown file.", file_name(&info)));
    }
    let options = PreviewOptions {
      title: format!("Preview {}", file_name(&info)),
      document: Some(id),
      beside,
    };
    workbench::show_preview(PREVIEW, &id.to_string(), &options)
      .map_err(|error| describe(&error))?;
    self.previewed.insert(id);
    if !update(id).map_err(|error| describe(&error))? {
      self.previewed.remove(&id);
    }
    Ok(())
  }

  fn handle_event(&mut self, event: Event) -> Result<(), String> {
    match event {
      Event::DocumentChanged(change) => self.refresh(change.document),
      // The editor closes the document's preview itself.
      Event::DocumentClosed(document) => {
        self.previewed.remove(&document);
      }
      _ => {}
    }
    Ok(())
  }
}

register!(Markdown);
