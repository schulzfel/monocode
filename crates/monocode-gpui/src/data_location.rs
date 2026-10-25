//! Which database, and so which data folder, this process uses. The window
//! and the `mcp` relay resolve it with the same flags and bundle default, so
//! the relay finds the control socket in the `control/` subfolder of the
//! folder holding the database the window opened.
use monocode_control::ops::app::DataMode;
use std::{path::PathBuf, sync::OnceLock};

/// The data choice from the bundle, the build and the data flags.
#[derive(Clone)]
pub(crate) struct DataLocation {
    monocode: bool,
    monocode_db: PathBuf,
}

impl DataLocation {
    /// Development builds use a separate database; release builds and bundles
    /// marked `monocode-data` use the installed MonoCode database.
    pub(crate) fn new() -> Self {
        Self {
            monocode: crate::bundle_uses_monocode_data().unwrap_or(!cfg!(debug_assertions)),
            monocode_db: monocode_core::store::monocode_database(),
        }
    }
    /// Applies `arg` if it is a data flag (`--dev-data`, `--monocode-data`,
    /// `--monocode-db PATH`), taking its value from `rest`. Returns whether
    /// it was one.
    pub(crate) fn apply(&mut self, arg: &str, rest: &mut impl Iterator<Item = String>) -> bool {
        match arg {
            "--dev-data" => self.monocode = false,
            "--monocode-data" => self.monocode = true,
            "--monocode-db" => {
                self.monocode = true;
                self.monocode_db = PathBuf::from(rest.next().expect("--monocode-db needs a path"));
            }
            _ => return false,
        }
        true
    }
    /// Whether the installed MonoCode data is used.
    pub(crate) fn monocode(&self) -> bool {
        self.monocode
    }
    /// The database to open: the MonoCode one, or `monocode.db` in the
    /// development folder (`MONOCODE_NATIVE_DATA_DIR` when set).
    pub(crate) fn database(&self) -> PathBuf {
        if self.monocode() {
            self.monocode_db.clone()
        } else {
            crate::data_dir().join("monocode.db")
        }
    }
    pub(crate) fn mode(&self) -> DataMode {
        if self.monocode() {
            DataMode::Monocode
        } else {
            DataMode::Dev
        }
    }
    /// The data flags that choose this data, as `mcp` takes them: whatever
    /// the build or bundle default is, the relay then opens the same folder.
    pub(crate) fn flags(&self) -> Vec<String> {
        if !self.monocode() {
            vec!["--dev-data".into()]
        } else if self.monocode_db == monocode_core::store::monocode_database() {
            vec!["--monocode-data".into()]
        } else {
            vec![
                "--monocode-db".into(),
                self.monocode_db.display().to_string(),
            ]
        }
    }
}

static OPENED: OnceLock<DataLocation> = OnceLock::new();

/// Records which data this window opened, for `app_info` and the command
/// Settings shows.
pub(crate) fn record(location: &DataLocation) {
    let _ = OPENED.set(location.clone());
}

/// Which data this window opened; development data until recorded.
pub(crate) fn mode() -> DataMode {
    OPENED.get().map_or(DataMode::Dev, DataLocation::mode)
}

/// The data flags of the data this window opened; `--dev-data` until
/// recorded.
pub(crate) fn flags() -> Vec<String> {
    OPENED
        .get()
        .map_or_else(|| vec!["--dev-data".into()], DataLocation::flags)
}
