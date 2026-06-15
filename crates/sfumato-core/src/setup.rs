use std::path::{Path, PathBuf};

use crate::{
    config::{GlobalConfig, user_config_path},
    repositories::{FilesystemGlobalConfigRepository, GlobalConfigRepository, ThemeRepository},
    themes::FilesystemThemeRepository,
};
use anyhow::{Context, Result};

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
    global_repository: Box<dyn GlobalConfigRepository>,
    theme_repository: Box<dyn ThemeRepository>,
}

impl SetupService {
    pub fn load() -> Result<Self> {
        let user_config_path =
            user_config_path().context("Could not find user configuration directory")?;
        Ok(Self::new(
            user_config_path.clone(),
            Box::new(FilesystemGlobalConfigRepository::new(user_config_path)),
            Box::new(FilesystemThemeRepository::load()?),
        ))
    }

    pub fn new(
        user_config_path: PathBuf,
        global_repository: Box<dyn GlobalConfigRepository>,
        theme_repository: Box<dyn ThemeRepository>,
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
