use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    config::GlobalConfig,
    repositories::{GlobalConfigRepository, ThemeRepository},
};
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct UserSetupRequest {
    pub config: GlobalConfig,
}

#[derive(Clone, Debug)]
pub struct UserSetupResult {
    pub path: PathBuf,
}

pub struct SetupService {
    user_config_path: PathBuf,
    global_repository: Arc<dyn GlobalConfigRepository>,
    theme_repository: Arc<dyn ThemeRepository>,
}

impl SetupService {
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

    pub fn user_config_exists(&self) -> bool {
        self.user_config_path.exists()
    }

    pub fn user_config_path(&self) -> &Path {
        &self.user_config_path
    }

    pub fn setup_user(&self, request: UserSetupRequest) -> Result<UserSetupResult> {
        self.global_repository.save(&request.config)?;
        self.theme_repository.install_default()?;
        Ok(UserSetupResult {
            path: self.user_config_path.clone(),
        })
    }
}
