//! First-run user setup.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    config::GlobalConfig,
    errors::SfumatoResult as Result,
    repositories::{GlobalConfigRepository, ThemeRepository},
};

/// What to write when initialising a user.
#[derive(Clone, Debug)]
pub struct UserSetupRequest {
    /// The complete user-global configuration to persist. Assembled by the caller —
    /// interactively by the CLI, from defaults with `--yes` — so this service does
    /// not own the questions asked.
    pub config: GlobalConfig,
}

/// Where setup put things.
#[derive(Clone, Debug)]
pub struct UserSetupResult {
    /// The configuration file written, so the caller can name it back to the user.
    pub path: PathBuf,
}

/// Initialises a user's configuration and the theme it needs to render anything.
pub struct SetupService {
    user_config_path: PathBuf,
    global_repository: Arc<dyn GlobalConfigRepository>,
    theme_repository: Arc<dyn ThemeRepository>,
}

impl SetupService {
    /// Creates the service over the persistence ports it writes through.
    pub fn new(
        user_config_path: PathBuf,
        global_repository: Arc<dyn GlobalConfigRepository>,
        theme_repository: Arc<dyn ThemeRepository>,
    ) -> Self {
        Self {
            user_config_path,
            global_repository,
            theme_repository,
        }
    }

    /// Whether a user configuration is already present.
    ///
    /// The caller uses this to refuse overwriting one without `--force`.
    pub fn user_config_exists(&self) -> bool {
        self.global_repository.exists()
    }

    /// Where the user configuration lives, whether or not it exists yet.
    pub fn user_config_path(&self) -> &Path {
        &self.user_config_path
    }

    /// Persists the configuration and installs the bundled default theme.
    ///
    /// The theme is not optional: without one, the first `generate` would fail on a
    /// machine that had just been told it was set up.
    pub fn setup_user(&self, request: UserSetupRequest) -> Result<UserSetupResult> {
        self.global_repository.save(&request.config)?;
        self.theme_repository.install_default()?;
        Ok(UserSetupResult {
            path: self.user_config_path.clone(),
        })
    }
}
