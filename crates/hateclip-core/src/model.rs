use serde::{Deserialize, Serialize};

/// Uniform type identifiers used for the representations we keep.
pub mod uti {
    pub const PLAIN_TEXT: &str = "public.utf8-plain-text";
    pub const RTF: &str = "public.rtf";
    pub const HTML: &str = "public.html";
    pub const PNG: &str = "public.png";
    pub const TIFF: &str = "public.tiff";
    pub const FILE_URL: &str = "public.file-url";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Text,
    RichText,
    Image,
    Files,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Text => "text",
            ItemKind::RichText => "richtext",
            ItemKind::Image => "image",
            ItemKind::Files => "files",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "text" => Some(ItemKind::Text),
            "richtext" => Some(ItemKind::RichText),
            "image" => Some(ItemKind::Image),
            "files" => Some(ItemKind::Files),
            _ => None,
        }
    }
}

/// Raw bytes of one pasteboard type, kept so rich content pastes back intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Representation {
    pub uti: String,
    pub data: Vec<u8>,
}

/// Content read from the system clipboard, before it is stored.
#[derive(Debug, Clone, Default)]
pub struct Capture {
    /// Plain text, when the clipboard has any.
    pub text: Option<String>,
    /// Extra rich representations (RTF, HTML) that accompany the text.
    pub rich: Vec<Representation>,
    /// Image bytes with their type (PNG or TIFF).
    pub image: Option<Representation>,
    /// Absolute paths of copied files.
    pub files: Vec<String>,
    pub source_bundle_id: Option<String>,
    pub source_name: Option<String>,
}

impl Capture {
    /// Decide what kind of item this capture becomes. Files win over images,
    /// and text wins over images (copying table cells often adds a picture).
    pub fn kind(&self) -> Option<ItemKind> {
        if !self.files.is_empty() {
            Some(ItemKind::Files)
        } else if self.text.as_deref().is_some_and(|t| !t.trim().is_empty()) {
            if self.rich.is_empty() {
                Some(ItemKind::Text)
            } else {
                Some(ItemKind::RichText)
            }
        } else if self.image.is_some() {
            Some(ItemKind::Image)
        } else {
            None
        }
    }
}

/// One row of history as shown in the list.
#[derive(Debug, Clone, Serialize)]
pub struct ClipItem {
    pub id: i64,
    pub kind: ItemKind,
    pub created_at: i64,
    pub last_used_at: i64,
    pub source_bundle_id: Option<String>,
    pub source_name: Option<String>,
    pub preview: String,
    pub byte_size: i64,
    pub pinned: bool,
    pub thumb_path: Option<String>,
    pub image_width: Option<i64>,
    pub image_height: Option<i64>,
    pub file_count: i64,
}

/// Full content for the preview pane.
#[derive(Debug, Clone, Serialize)]
pub struct ItemDetail {
    pub item: ClipItem,
    pub text: Option<String>,
    pub text_truncated: bool,
    pub image_path: Option<String>,
    pub files: Vec<String>,
}

/// Everything needed to put an item back on the clipboard.
#[derive(Debug, Clone, Default)]
pub struct Payload {
    pub text: Option<String>,
    pub rich: Vec<Representation>,
    pub image: Option<Representation>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Query {
    pub text: String,
    pub kind: Option<ItemKind>,
    pub source_bundle_id: Option<String>,
    pub pinned_only: bool,
    pub limit: u32,
    pub offset: u32,
}
