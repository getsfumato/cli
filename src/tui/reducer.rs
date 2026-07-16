//! State transitions produced by asynchronous resource-operation messages.

use super::*;

pub(super) fn reduce_message(app: &mut App, message: UiMessage) {
    match message {
        UiMessage::GenerationEvent { job_id, event } => {
            if !app.jobs.is_active(job_id) {
                return;
            }
            if let TextGenerationEvent::StageStarted { stage, .. } = &event {
                app.current_stage = Some(*stage);
            }
            let activity = Activity::from_event(&event);
            let image_path = activity.image_path.clone();
            app.activities.push(activity);
            app.activity_index = app.activities.len().saturating_sub(1);
            if let Some(path) = image_path {
                app.load_image(&path);
            }
        }
        UiMessage::OperationEvent { job_id, event } => {
            if !app.jobs.is_active(job_id) {
                return;
            }
            if let Some(activity) = Activity::from_operation_event(&event) {
                app.activities.push(activity);
                app.activity_index = app.activities.len().saturating_sub(1);
            }
        }
        UiMessage::ResourceFinished { job_id, result } => {
            if !app.jobs.finish(job_id) {
                return;
            }
            app.active_task = None;
            match *result {
                Ok(result) => {
                    for warning in result.warnings() {
                        app.activities.push(Activity {
                            kind: ActivityKind::Warning,
                            title: "Resource warning".to_string(),
                            detail: warning.clone(),
                            image_path: None,
                        });
                    }
                    app.status = Some((result.completion_message().to_string(), false));
                    app.result = Some(result);
                }
                Err(error) => {
                    app.generation_failed = true;
                    app.activities.push(Activity {
                        kind: ActivityKind::Warning,
                        title: "Resource operation failed".to_string(),
                        detail: error.clone(),
                        image_path: None,
                    });
                    app.status = Some((error, true));
                }
            }
            app.activity_index = app.activities.len().saturating_sub(1);
            app.transition(Screen::Complete);
        }
        UiMessage::ResourceCancelled { job_id } => {
            if !app.jobs.finish(job_id) {
                return;
            }
            app.active_task = None;
            app.status = Some(("Operation cancelled".to_string(), false));
            app.activities.push(Activity {
                kind: ActivityKind::Warning,
                title: "Operation cancelled".to_string(),
                detail: "No staged artifacts were committed.".to_string(),
                image_path: None,
            });
            app.activity_index = app.activities.len().saturating_sub(1);
            app.transition(Screen::Complete);
        }
    }
}

impl App {
    pub(super) fn new(picker: Picker, application: Arc<SfumatoApplication>) -> Self {
        let (sender, messages) = channel(256);
        Self {
            application,
            screen: Screen::Home,
            nav_index: 0,
            browse_rows: Vec::new(),
            browse_index: 0,
            browse_focus: BrowseFocus::Rows,
            browse_action_index: 0,
            browse_detail_scroll: 0,
            operation: None,
            form: GenerateForm::default(),
            edit_form: EditForm::default(),
            resource_operation: ResourceOperation::Generate,
            activities: Vec::new(),
            activity_index: 0,
            current_stage: None,
            generation_failed: false,
            result: None,
            status: None,
            tick: 0,
            should_quit: false,
            sender,
            messages,
            jobs: OperationLifecycle::default(),
            active_task: None,
            picker,
            image: None,
            effects: EffectManager::default(),
            dirty: true,
        }
    }

    pub(super) fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.dirty = true;
    }

    pub(super) fn transition(&mut self, screen: Screen) {
        self.screen = screen;
        self.effects.add_unique_effect("screen", fx::coalesce(260));
        self.dirty = true;
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.cancel_active_job();
            self.should_quit = true;
            return;
        }
        if self.operation.is_some() {
            self.handle_operation_key(key);
            return;
        }
        match self.screen {
            Screen::Home => self.handle_home_key(key),
            Screen::Browse(section) => self.handle_browse_key(section, key),
            Screen::Generate => self.handle_generate_key(key),
            Screen::Edit => self.handle_edit_key(key),
            Screen::Running => self.handle_running_key(key),
            Screen::Complete => self.handle_complete_key(key),
        }
    }

    pub(super) fn handle_home_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.nav_index = self.nav_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.nav_index = (self.nav_index + 1).min(NAV_ITEMS.len() - 1);
            }
            KeyCode::Enter => match self.nav_index {
                0 => self.transition(Screen::Generate),
                1 => self.transition(Screen::Edit),
                2 => self.open_section(Section::Projects),
                3 => self.open_section(Section::Models),
                4 => self.open_section(Section::Connectors),
                5 => self.open_section(Section::Themes),
                6 => self.open_section(Section::Prompts),
                7 => self.open_section(Section::Configuration),
                8 => self.open_section(Section::Setup),
                _ => {}
            },
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            _ => {}
        }
    }

    pub(super) fn open_section(&mut self, section: Section) {
        match load_section(section, &self.application) {
            Ok(rows) => {
                self.browse_rows = rows;
                self.browse_index = 0;
                self.browse_focus = BrowseFocus::Actions;
                self.browse_action_index = 0;
                self.browse_detail_scroll = 0;
                self.status = None;
                self.transition(Screen::Browse(section));
            }
            Err(error) => self.status = Some((format!("{error:#}"), true)),
        }
    }

    pub(super) fn handle_browse_key(&mut self, section: Section, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Backspace => self.transition(Screen::Home),
            KeyCode::Tab | KeyCode::BackTab => {
                self.browse_focus = match self.browse_focus {
                    BrowseFocus::Actions => BrowseFocus::Rows,
                    BrowseFocus::Rows => BrowseFocus::Actions,
                };
            }
            KeyCode::Left | KeyCode::Char('h') if self.browse_focus == BrowseFocus::Actions => {
                self.browse_action_index = self.browse_action_index.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') if self.browse_focus == BrowseFocus::Actions => {
                self.browse_action_index = (self.browse_action_index + 1)
                    .min(section_actions(section).len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') if self.browse_focus == BrowseFocus::Rows => {
                self.browse_index = self.browse_index.saturating_sub(1);
                self.browse_detail_scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') if self.browse_focus == BrowseFocus::Rows => {
                self.browse_index =
                    (self.browse_index + 1).min(self.browse_rows.len().saturating_sub(1));
                self.browse_detail_scroll = 0;
            }
            KeyCode::PageUp => {
                self.browse_detail_scroll = self.browse_detail_scroll.saturating_sub(8);
            }
            KeyCode::PageDown => {
                self.browse_detail_scroll = self.browse_detail_scroll.saturating_add(8);
            }
            KeyCode::Enter if self.browse_focus == BrowseFocus::Actions => {
                self.execute_browse_action(section);
            }
            KeyCode::Char('r') => self.open_section(section),
            _ => {}
        }
    }

    pub(super) fn execute_browse_action(&mut self, section: Section) {
        let Some(action) = section_actions(section)
            .get(self.browse_action_index)
            .copied()
        else {
            return;
        };
        if action == BrowseAction::ProjectActivate {
            self.activate_project();
            return;
        }
        match self.operation_for_action(action) {
            Ok(operation) => self.operation = Some(operation),
            Err(error) => self.status = Some((format!("{error:#}"), true)),
        }
    }

    pub(super) fn operation_for_action(&self, action: BrowseAction) -> Result<OperationForm> {
        let selected = self.browse_rows.get(self.browse_index);
        let operation = match action {
            BrowseAction::ProjectCreate => OperationForm {
                title: "Create project",
                kind: OperationKind::ProjectCreate,
                target: None,
                fields: vec![
                    text_field("Name", "", "university"),
                    text_field("Path", ".", "project working directory"),
                    FormField::Toggle {
                        label: "Make active",
                        value: true,
                    },
                    submit_field("Create project"),
                ],
                selected: 0,
            },
            BrowseAction::ProjectRemove => {
                let row = selected.context("Select a project to remove")?;
                confirmation_form(
                    "Remove project",
                    OperationKind::ProjectRemove,
                    row.title.clone(),
                    "Remove from registry",
                )
            }
            BrowseAction::ModelAdd => OperationForm {
                title: "Add model profile",
                kind: OperationKind::ModelAdd,
                target: None,
                fields: vec![
                    text_field("Name", "", "cloud-draft"),
                    text_field("Connector", "openrouter", "connector profile"),
                    text_field("Model ID", "", "provider model identifier"),
                    text_field("Capabilities", "text", "text, code, image"),
                    text_field("Options", "", "max_tokens=12000, temperature=0.4"),
                    submit_field("Add model"),
                ],
                selected: 0,
            },
            BrowseAction::ModelEdit => {
                let row = selected.context("Select a model profile to edit")?;
                let profile = self.application.show_model(&row.title)?;
                let capabilities = profile
                    .capabilities
                    .iter()
                    .map(|capability| capability.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let options = profile.options.cli_pairs().join(", ");
                OperationForm {
                    title: "Edit model profile",
                    kind: OperationKind::ModelEdit,
                    target: Some(row.title.clone()),
                    fields: vec![
                        text_field("Connector", &profile.connector, "connector profile"),
                        text_field("Model ID", &profile.model, "provider model identifier"),
                        text_field("Capabilities", &capabilities, "text, code, image"),
                        text_field("Options", &options, "key=value pairs"),
                        submit_field("Update model"),
                    ],
                    selected: 0,
                }
            }
            BrowseAction::ModelUse => {
                let profile = selected.map(|row| row.title.as_str()).unwrap_or("");
                OperationForm {
                    title: "Set model default",
                    kind: OperationKind::ModelUse,
                    target: None,
                    fields: vec![
                        text_field("Capability or role", "text", "text or reviewer"),
                        text_field("Profile", profile, "model profile"),
                        text_field("Project", "", "blank for user default"),
                        submit_field("Set default"),
                    ],
                    selected: 0,
                }
            }
            BrowseAction::ModelRemove => {
                let row = selected.context("Select a model profile to remove")?;
                confirmation_form(
                    "Remove model profile",
                    OperationKind::ModelRemove,
                    row.title.clone(),
                    "Remove model",
                )
            }
            BrowseAction::ConnectorOllama | BrowseAction::ConnectorOpenrouter => {
                let is_ollama = action == BrowseAction::ConnectorOllama;
                OperationForm {
                    title: if is_ollama {
                        "Setup Ollama"
                    } else {
                        "Setup OpenRouter"
                    },
                    kind: OperationKind::ConnectorSetup(if is_ollama {
                        ConnectorPreset::Ollama
                    } else {
                        ConnectorPreset::Openrouter
                    }),
                    target: None,
                    fields: vec![
                        text_field(
                            "Name",
                            if is_ollama { "ollama" } else { "openrouter" },
                            "connector name",
                        ),
                        text_field(
                            "API key environment",
                            "OPENROUTER_API_KEY",
                            "used by OpenRouter",
                        ),
                        submit_field("Save connector"),
                    ],
                    selected: 0,
                }
            }
            BrowseAction::ThemeCreate => OperationForm {
                title: "Create theme",
                kind: OperationKind::ThemeCreate,
                target: None,
                fields: vec![
                    text_field("Name", "", "gruvbox"),
                    submit_field("Create theme"),
                ],
                selected: 0,
            },
            BrowseAction::ThemeUse => {
                let theme = selected.context("Select a theme to apply")?;
                OperationForm {
                    title: "Apply theme",
                    kind: OperationKind::ThemeUse,
                    target: Some(theme.title.clone()),
                    fields: vec![
                        text_field("Project", "", "blank for active project"),
                        submit_field("Apply theme"),
                    ],
                    selected: 0,
                }
            }
            BrowseAction::PromptCustomizeUser | BrowseAction::PromptCustomizeProject => {
                let prompt = selected.context("Select a prompt to customize")?;
                let scope = if action == BrowseAction::PromptCustomizeUser {
                    PromptOverrideScope::User
                } else {
                    PromptOverrideScope::Project
                };
                confirmation_form(
                    "Customize prompt",
                    OperationKind::PromptCustomize(scope),
                    prompt.title.clone(),
                    "Create override",
                )
            }
            BrowseAction::PromptValidate => OperationForm {
                title: "Validate prompts",
                kind: OperationKind::PromptValidate,
                target: None,
                fields: vec![submit_field("Validate prompts")],
                selected: 0,
            },
            BrowseAction::ConfigSet => OperationForm {
                title: "Set configuration value",
                kind: OperationKind::ConfigSet,
                target: None,
                fields: vec![
                    text_field("Scope", "user", "user or project"),
                    text_field("Project", "", "blank for active project"),
                    text_field("Key", "", "dotted.key"),
                    text_field("Value", "", "TOML value or string"),
                    submit_field("Set value"),
                ],
                selected: 0,
            },
            BrowseAction::ConfigDelete => OperationForm {
                title: "Delete configuration value",
                kind: OperationKind::ConfigDelete,
                target: None,
                fields: vec![
                    text_field("Scope", "user", "user or project"),
                    text_field("Project", "", "blank for active project"),
                    text_field("Key", "", "dotted.key"),
                    FormField::Toggle {
                        label: "Confirm deletion",
                        value: false,
                    },
                    submit_field("Delete value"),
                ],
                selected: 0,
            },
            BrowseAction::SetupUser => OperationForm {
                title: "Initialize user",
                kind: OperationKind::SetupUser,
                target: None,
                fields: vec![
                    text_field(
                        "Name",
                        &env::var("USER").unwrap_or_else(|_| "Alex".to_string()),
                        "your name",
                    ),
                    text_field(
                        "Learning styles",
                        "visual, step-by-step",
                        "comma-separated preferences",
                    ),
                    text_field("Connector", "ollama", "ollama or openrouter"),
                    text_field("Profile", "local-text", "model profile name"),
                    text_field("Model ID", "llama3.2", "provider model identifier"),
                    text_field(
                        "API key environment",
                        "OPENROUTER_API_KEY",
                        "used by OpenRouter",
                    ),
                    FormField::Toggle {
                        label: "Overwrite existing config",
                        value: false,
                    },
                    submit_field("Initialize user"),
                ],
                selected: 0,
            },
            BrowseAction::ProjectActivate => unreachable!("project activation is immediate"),
        };
        Ok(operation)
    }

    pub(super) fn activate_project(&mut self) {
        let Some(row) = self.browse_rows.get(self.browse_index) else {
            return;
        };
        match self.application.use_project(&row.title) {
            Ok(name) => {
                self.open_section(Section::Projects);
                self.status = Some((format!("Active project: {name}"), false));
            }
            Err(error) => self.status = Some((format!("{error:#}"), true)),
        }
    }

    pub(super) fn handle_operation_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.operation = None;
            return;
        }
        let Some(operation) = &mut self.operation else {
            return;
        };
        match key.code {
            KeyCode::Up => operation.selected = operation.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                operation.selected =
                    (operation.selected + 1).min(operation.fields.len().saturating_sub(1));
            }
            KeyCode::BackTab => operation.selected = operation.selected.saturating_sub(1),
            KeyCode::Enter => match operation.fields.get(operation.selected) {
                Some(FormField::Toggle { .. }) => {
                    if let Some(FormField::Toggle { value, .. }) =
                        operation.fields.get_mut(operation.selected)
                    {
                        *value = !*value;
                    }
                }
                Some(FormField::Submit { .. }) => self.submit_operation(),
                _ => {
                    operation.selected =
                        (operation.selected + 1).min(operation.fields.len().saturating_sub(1));
                }
            },
            KeyCode::Char(' ')
                if matches!(
                    operation.fields.get(operation.selected),
                    Some(FormField::Toggle { .. })
                ) =>
            {
                if let Some(FormField::Toggle { value, .. }) =
                    operation.fields.get_mut(operation.selected)
                {
                    *value = !*value;
                }
            }
            KeyCode::Backspace => {
                if let Some(FormField::Text { value, .. }) =
                    operation.fields.get_mut(operation.selected)
                {
                    value.pop();
                }
            }
            KeyCode::Char(character) => {
                if let Some(FormField::Text { value, .. }) =
                    operation.fields.get_mut(operation.selected)
                {
                    value.push(character);
                }
            }
            _ => {}
        }
    }

    pub(super) fn submit_operation(&mut self) {
        let Some(operation) = self.operation.clone() else {
            return;
        };
        let section = match self.screen {
            Screen::Browse(section) => section,
            _ => return,
        };
        match execute_operation(&operation, &self.application) {
            Ok(message) => {
                self.operation = None;
                self.open_section(section);
                self.status = Some((message, false));
            }
            Err(error) => self.status = Some((format!("{error:#}"), true)),
        }
    }

    pub(super) fn handle_generate_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.transition(Screen::Home),
            KeyCode::Up => self.form.selected = self.form.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                self.form.selected = (self.form.selected + 1).min(self.form.fields.len() - 1)
            }
            KeyCode::BackTab => self.form.selected = self.form.selected.saturating_sub(1),
            KeyCode::Enter => {
                let selected = self.form.selected;
                match self.form.fields.get(selected) {
                    Some(FormField::Toggle { .. }) => self.toggle_form_field(),
                    Some(FormField::Submit { .. }) => self.start_generation(),
                    Some(FormField::Text {
                        multiline: true, ..
                    }) if key.modifiers.contains(KeyModifiers::SHIFT) => self.push_form_char('\n'),
                    _ => {
                        self.form.selected =
                            (self.form.selected + 1).min(self.form.fields.len() - 1)
                    }
                }
            }
            KeyCode::Char(' ')
                if matches!(
                    self.form.fields.get(self.form.selected),
                    Some(FormField::Toggle { .. })
                ) =>
            {
                self.toggle_form_field();
            }
            KeyCode::Backspace => {
                if let Some(FormField::Text { value, .. }) =
                    self.form.fields.get_mut(self.form.selected)
                {
                    value.pop();
                }
            }
            KeyCode::Char(character) => self.push_form_char(character),
            _ => {}
        }
    }

    pub(super) fn push_form_char(&mut self, character: char) {
        if let Some(FormField::Text { value, .. }) = self.form.fields.get_mut(self.form.selected) {
            value.push(character);
        }
    }

    pub(super) fn handle_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.transition(Screen::Home),
            KeyCode::Up => {
                self.edit_form.selected = self.edit_form.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Tab => {
                self.edit_form.selected =
                    (self.edit_form.selected + 1).min(self.edit_form.fields.len() - 1);
            }
            KeyCode::BackTab => {
                self.edit_form.selected = self.edit_form.selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                let selected = self.edit_form.selected;
                match self.edit_form.fields.get(selected) {
                    Some(FormField::Submit { .. }) => self.start_edit(),
                    Some(FormField::Text {
                        multiline: true, ..
                    }) if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        self.push_edit_form_char('\n');
                    }
                    _ => {
                        self.edit_form.selected =
                            (self.edit_form.selected + 1).min(self.edit_form.fields.len() - 1);
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(FormField::Text { value, .. }) =
                    self.edit_form.fields.get_mut(self.edit_form.selected)
                {
                    value.pop();
                }
            }
            KeyCode::Char(character) => self.push_edit_form_char(character),
            _ => {}
        }
    }

    pub(super) fn push_edit_form_char(&mut self, character: char) {
        if let Some(FormField::Text { value, .. }) =
            self.edit_form.fields.get_mut(self.edit_form.selected)
        {
            value.push(character);
        }
    }

    pub(super) fn toggle_form_field(&mut self) {
        if let Some(FormField::Toggle { value, .. }) = self.form.fields.get_mut(self.form.selected)
        {
            *value = !*value;
        }
    }

    pub(super) fn start_generation(&mut self) {
        let args = match self.form.to_args() {
            Ok(args) => args,
            Err(error) => {
                self.status = Some((error.to_string(), true));
                return;
            }
        };
        self.activities.clear();
        self.activity_index = 0;
        self.current_stage = None;
        self.generation_failed = false;
        self.result = None;
        self.image = None;
        self.status = None;
        self.resource_operation = ResourceOperation::Generate;
        self.transition(Screen::Running);

        let (job_id, operation) = self.begin_job();
        let sender = self.sender.clone();
        let application = Arc::clone(&self.application);
        let sink = effects::generation_event_sink(job_id, sender.clone());
        self.active_task = Some(effects::spawn_generation(
            job_id,
            application,
            args,
            sink,
            operation,
            sender,
        ));
    }

    pub(super) fn start_edit(&mut self) {
        let args = match self.edit_form.to_args() {
            Ok(args) => args,
            Err(error) => {
                self.status = Some((error.to_string(), true));
                return;
            }
        };
        self.activities.clear();
        self.activity_index = 0;
        self.current_stage = None;
        self.generation_failed = false;
        self.result = None;
        self.image = None;
        self.status = None;
        self.resource_operation = ResourceOperation::Edit;
        self.transition(Screen::Running);

        let (job_id, operation) = self.begin_job();
        let sender = self.sender.clone();
        let application = Arc::clone(&self.application);
        let sink = effects::generation_event_sink(job_id, sender.clone());
        self.active_task = Some(effects::spawn_edit(
            job_id,
            application,
            args,
            sink,
            operation,
            sender,
        ));
    }

    pub(super) fn begin_job(&mut self) -> (u64, OperationContext) {
        self.cancel_active_job();
        let job_id = self.jobs.next_job_id();
        let events = effects::operation_event_sink(job_id, self.sender.clone());
        self.jobs.begin(events)
    }

    pub(super) fn cancel_active_job(&self) {
        self.jobs.cancel();
    }

    pub(super) async fn shutdown(&mut self) {
        self.cancel_active_job();
        if let Some(task) = self.active_task.take() {
            let _ = task.await;
        }
    }

    pub(super) fn handle_running_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.cancel_active_job();
                self.status = Some(("Cancelling the active operation...".to_string(), false));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.activity_index = self.activity_index.saturating_sub(1);
                self.load_selected_image();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.activity_index =
                    (self.activity_index + 1).min(self.activities.len().saturating_sub(1));
                self.load_selected_image();
            }
            _ => {}
        }
    }

    pub(super) fn handle_complete_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Backspace => self.transition(Screen::Home),
            KeyCode::Enter => self.transition(match self.resource_operation {
                ResourceOperation::Generate => Screen::Generate,
                ResourceOperation::Edit => Screen::Edit,
            }),
            KeyCode::Up | KeyCode::Char('k') => {
                self.activity_index = self.activity_index.saturating_sub(1);
                self.load_selected_image();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.activity_index =
                    (self.activity_index + 1).min(self.activities.len().saturating_sub(1));
                self.load_selected_image();
            }
            _ => {}
        }
    }

    pub(super) fn handle_message(&mut self, message: UiMessage) {
        reducer::reduce_message(self, message);
    }

    pub(super) fn load_selected_image(&mut self) {
        let path = self
            .activities
            .get(self.activity_index)
            .and_then(|activity| activity.image_path.clone());
        if let Some(path) = path {
            self.load_image(&path);
        } else {
            self.image = None;
        }
    }

    pub(super) fn load_image(&mut self, path: &std::path::Path) {
        match image::open(path) {
            Ok(image) => self.image = Some(self.picker.new_resize_protocol(image)),
            Err(error) => {
                self.image = None;
                self.status = Some((
                    format!("Could not preview {}: {error}", path.display()),
                    true,
                ));
            }
        }
    }
}
