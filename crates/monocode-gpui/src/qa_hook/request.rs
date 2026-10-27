//! The hook's requests: one JSON object per line, named by `op`.
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// What the QA lane asks the window to do.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum Request {
    /// Type `text` into session `session`'s composer and press Enter.
    Submit { session: i64, text: String },
    /// Dispatch the action registered as `name`, built from `data`.
    Action {
        name: String,
        #[serde(default)]
        data: Option<Value>,
    },
    /// Show a session, or a Mono's view.
    View {
        #[serde(default)]
        session: Option<i64>,
        #[serde(default)]
        mono: Option<String>,
    },
    /// Write the window as a PNG to `path`, inside the data folder.
    Capture { path: PathBuf },
    /// What the window shows, in text.
    State {},
    /// Quit as the person does, with normal persistence.
    Quit {},
}

/// What a `view` request shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum View {
    Session(i64),
    Mono(String),
}

impl Request {
    /// The request on `line`, checked: a view names exactly one target and
    /// a capture's path lies inside `data`.
    pub(crate) fn parse(line: &str, data: &Path) -> Result<Self, String> {
        let request: Self = serde_json::from_str(line).map_err(|e| e.to_string())?;
        match &request {
            Self::View { .. } => {
                request.view()?;
            }
            Self::Capture { path } => {
                return Ok(Self::Capture {
                    path: inside(path, data)?,
                });
            }
            _ => {}
        }
        Ok(request)
    }
    /// A view request's one target.
    pub(crate) fn view(&self) -> Result<View, String> {
        match self {
            Self::View {
                session: Some(id),
                mono: None,
            } => Ok(View::Session(*id)),
            Self::View {
                session: None,
                mono: Some(id),
            } => Ok(View::Mono(id.clone())),
            Self::View { .. } => Err("view takes exactly one of session or mono".into()),
            _ => Err("not a view request".into()),
        }
    }
}

/// `path` when it is a PNG file inside the data folder `data`, resolved
/// through its existing parent so `..` and links cannot leave the folder.
pub(crate) fn inside(path: &Path, data: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("{} is not an absolute path", path.display()));
    }
    if path.extension().and_then(|e| e.to_str()) != Some("png") {
        return Err(format!("{} is not a .png file", path.display()));
    }
    let name = path.file_name().ok_or("the capture path names no file")?;
    let parent = path
        .parent()
        .ok_or("the capture path has no folder")?
        .canonicalize()
        .map_err(|e| format!("the capture folder cannot be resolved: {e}"))?;
    let data = data
        .canonicalize()
        .map_err(|e| format!("the data folder cannot be resolved: {e}"))?;
    if !parent.starts_with(&data) {
        return Err(format!(
            "{} is outside the data folder {}",
            path.display(),
            data.display()
        ));
    }
    Ok(parent.join(name))
}
