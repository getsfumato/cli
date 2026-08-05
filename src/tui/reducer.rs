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
                    // Every preview path in the feed points into the staging directory
                    // the tool wrote to, and committing moves that tree into an
                    // immutable revision and deletes the staging root. So the paths the
                    // feed collected are all dead by the time this screen is drawn, and
                    // selecting a chart reported "Could not preview" for a file that
                    // exists — one directory over.
                    reroot_previews(&mut app.activities, result.markdown_path());
                    // Where it landed, recorded in the feed rather than only in the
                    // status line: the footer is one row that elides, and it is
                    // overwritten by the completion message anyway, so a finished run
                    // used to leave no reachable answer to "where is the file".
                    for (label, path) in result.artifacts() {
                        app.activities.push(Activity {
                            kind: ActivityKind::Output,
                            title: label.to_string(),
                            detail: path.display().to_string(),
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
        UiMessage::ConnectorQueryFinished { connector, result } => {
            // The task has sent its result, so the handle is spent either way.
            app.connector_query = None;
            if app.screen != Screen::Browse(Section::Connectors) {
                return;
            }
            match result {
                Ok(rows) => {
                    app.browse_rows = rows;
                    app.connector_query_source = Some(connector.clone());
                    app.browse_index = 0;
                    app.browse_focus = BrowseFocus::Rows;
                    app.browse_detail_scroll = 0;
                    app.status = Some((
                        format!("Loaded native data from {connector}; press r to return"),
                        false,
                    ));
                }
                Err(error) => app.status = Some((error, true)),
            }
        }
    }
}

impl App {
    pub(super) fn new(picker: Picker, application: Arc<SfumatoApplication>) -> Self {
        let (sender, messages) = channel(256);
        let installed_plugins = application
            .list_installed_page_plugins()
            .map(|listing| listing.entries)
            .unwrap_or_default();
        let page_ui = installed_plugins
            .iter()
            .filter(|plugin| plugin.category == sfumato_core::page_plugins::PagePluginCategory::Ui)
            .map(|plugin| plugin.id.clone())
            .collect();
        let page_utilities = installed_plugins
            .into_iter()
            .filter(|plugin| {
                plugin.category == sfumato_core::page_plugins::PagePluginCategory::Utility
            })
            .map(|plugin| plugin.id)
            .collect();
        // Collected before the struct takes ownership of the facade.
        let snapshot = WorkspaceSnapshot::collect(&application);
        Self {
            application,
            screen: Screen::Home,
            nav_index: 0,
            browse_rows: Vec::new(),
            browse_index: 0,
            browse_focus: BrowseFocus::Rows,
            browse_action_index: 0,
            browse_detail_scroll: 0,
            connector_query_source: None,
            operation: None,
            form: GenerateForm::with_plugins(page_ui, page_utilities),
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
            connector_query: None,
            snapshot,
            started_at: None,
            overlay: None,
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
        // Refreshed on the way into a screen, not while drawing one. Transitions are
        // user-driven and rare; draws happen at the tick rate.
        self.refresh_snapshot();
        self.effects.add_unique_effect("screen", fx::coalesce(260));
        self.dirty = true;
    }

    /// Re-reads the workspace state the chrome and home screen display.
    ///
    /// Call after anything that could change it — activating a project, finishing a
    /// setup or a generation — so the views never have to ask mid-frame.
    pub(super) fn refresh_snapshot(&mut self) {
        self.snapshot = WorkspaceSnapshot::collect(&self.application);
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        // Checked before the overlay and form dispatch below, so the exit gesture
        // works from every screen — including one with a form or a picker open,
        // which would otherwise swallow the key as input.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('c'))
        {
            self.request_quit();
            return;
        }
        // The overlay owns every key while it is open, so a jump cannot half-apply to
        // the screen underneath.
        if self.overlay.is_some() {
            self.handle_overlay_key(key);
            return;
        }
        if self.operation.is_some() {
            self.handle_operation_key(key);
            return;
        }
        // Available from every screen: with eleven destinations, walking the menu is
        // the slow path and should not be the only one.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('k') {
            self.overlay = Some(Overlay::palette());
            self.dirty = true;
            return;
        }
        // Only where it cannot be a value: a form field takes `?` as text.
        if key.code == KeyCode::Char('?') && !matches!(self.screen, Screen::Generate | Screen::Edit)
        {
            self.overlay = Some(Overlay::Help);
            self.dirty = true;
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

    /// Handles keys while the palette or help overlay is open.
    pub(super) fn handle_overlay_key(&mut self, key: KeyEvent) {
        self.dirty = true;
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        if overlay == Overlay::Quit {
            match key.code {
                // Enter is deliberately not a confirmation: it is the key that
                // submits every form in this UI, so accepting it here would make a
                // stray enter end the session.
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.cancel_active_job();
                    self.should_quit = true;
                }
                // Anything else keeps the session, so a mistyped key cannot leave.
                _ => self.status = None,
            }
            return;
        }
        // The field picker shares the palette's matching and keys; only the list it
        // offers and where the answer goes differ.
        if let Overlay::Choice {
            target,
            mut query,
            mut selected,
        } = overlay
        {
            let values = self.choice_values(target);
            let labels: Vec<&str> = values.iter().map(|choice| choice.value.as_str()).collect();
            match key.code {
                KeyCode::Esc => return,
                KeyCode::Enter => {
                    if let Some(picked) = palette::matches(&labels, &query).get(selected) {
                        let picked = (*picked).to_owned();
                        self.set_choice(target, &picked);
                    }
                    return;
                }
                // Clearing is how a caller goes back to "whatever the project decides",
                // which is what an empty value means to every one of these fields.
                KeyCode::Delete => {
                    self.set_choice(target, "");
                    return;
                }
                KeyCode::Backspace => {
                    query.pop();
                    selected = 0;
                }
                KeyCode::Up => selected = selected.saturating_sub(1),
                KeyCode::Down => {
                    let count = palette::matches(&labels, &query).len();
                    selected = (selected + 1).min(count.saturating_sub(1));
                }
                KeyCode::Char(character) => {
                    query.push(character);
                    selected = 0;
                }
                _ => {}
            }
            self.overlay = Some(Overlay::Choice {
                target,
                query,
                selected,
            });
            return;
        }
        let Overlay::Palette {
            mut query,
            mut selected,
        } = overlay
        else {
            // Help closes on any key: it has nothing to interact with, and needing a
            // specific key to dismiss a reference card is its own small puzzle.
            return;
        };
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                let labels = Self::palette_labels();
                if let Some(label) = palette::matches(&labels, &query).get(selected).copied() {
                    self.jump_to(label);
                }
                return;
            }
            KeyCode::Backspace => {
                query.pop();
                selected = 0;
            }
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Down => {
                let count = palette::matches(&Self::palette_labels(), &query).len();
                selected = (selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Char(character) => {
                query.push(character);
                selected = 0;
            }
            _ => {}
        }
        self.overlay = Some(Overlay::Palette { query, selected });
    }

    /// Opens whichever menu entry `nav_index` points at.
    ///
    /// Shared by the menu and the palette so a destination cannot be reachable from
    /// one and not the other — the index-to-screen mapping lived inline in the menu's
    /// `Enter` arm, which is why the palette needed it lifted out.
    pub(super) fn open_nav_index(&mut self) {
        match NAV_ITEMS.get(self.nav_index).map(|item| item.title) {
            Some("Generate") => self.transition(Screen::Generate),
            Some("Edit") => self.transition(Screen::Edit),
            Some("Projects") => self.open_section(Section::Projects),
            Some("Models") => self.open_section(Section::Models),
            Some("Connectors") => self.open_section(Section::Connectors),
            Some("Themes") => self.open_section(Section::Themes),
            Some("Templates") => self.open_section(Section::Templates),
            Some("Artifacts") => self.open_section(Section::Artifacts),
            Some("Prompts") => self.open_section(Section::Prompts),
            Some("Tools") => self.open_section(Section::Tools),
            Some("Plugins") => self.open_section(Section::Plugins),
            Some("Configuration") => self.open_section(Section::Configuration),
            Some("Setup") => self.open_section(Section::Setup),
            _ => {}
        }
    }

    /// The values one picker field offers, resolved against the snapshot.
    pub(super) fn choice_values(&self, target: ChoiceTarget) -> Vec<Choice> {
        self.choice_source(target)
            .map(|source| source.choices(&self.snapshot.options).to_vec())
            .unwrap_or_default()
    }

    /// Which list a picker target offers.
    pub(super) fn choice_source(&self, target: ChoiceTarget) -> Option<ChoiceSource> {
        match target {
            ChoiceTarget::Generate(field) => self.form.choice_source(field),
            ChoiceTarget::Operation(index) => match self.operation.as_ref()?.fields.get(index)? {
                FormField::Choice { source, .. } => Some(*source),
                _ => None,
            },
        }
    }

    /// The label of a picker target, for the overlay's own title.
    pub(super) fn choice_label(&self, target: ChoiceTarget) -> &'static str {
        match target {
            ChoiceTarget::Generate(field) => self
                .form
                .field_ids
                .iter()
                .position(|candidate| *candidate == field)
                .and_then(|index| self.form.fields.get(index))
                .map(|entry| entry.label()),
            ChoiceTarget::Operation(index) => self
                .operation
                .as_ref()
                .and_then(|operation| operation.fields.get(index))
                .map(|entry| entry.label()),
        }
        .unwrap_or("CHOOSE")
    }

    /// Writes a picked value back, or clears it when `value` is empty.
    pub(super) fn set_choice(&mut self, target: ChoiceTarget, value: &str) {
        match target {
            ChoiceTarget::Generate(field) => self.form.set_choice(field, value),
            ChoiceTarget::Operation(index) => {
                if let Some(operation) = self.operation.as_mut()
                    && let Some(FormField::Choice { value: current, .. }) =
                        operation.fields.get_mut(index)
                {
                    *current = value.to_owned();
                }
            }
        }
    }

    /// Opens the picker for the focused operation-form field, when it has one.
    pub(super) fn open_operation_choice_picker(&mut self) {
        let Some(operation) = self.operation.as_ref() else {
            return;
        };
        let index = operation.selected;
        if matches!(operation.fields.get(index), Some(FormField::Choice { .. })) {
            self.overlay = Some(Overlay::choice(ChoiceTarget::Operation(index)));
        }
    }

    /// Every destination the palette can reach.
    pub(super) fn palette_labels() -> Vec<&'static str> {
        NAV_ITEMS.iter().map(|item| item.title).collect()
    }

    /// Opens the destination the palette selected.
    pub(super) fn jump_to(&mut self, label: &str) {
        let Some(index) = NAV_ITEMS.iter().position(|item| item.title == label) else {
            return;
        };
        self.nav_index = index;
        self.open_nav_index();
    }

    pub(super) fn handle_home_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.nav_index = self.nav_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.nav_index = (self.nav_index + 1).min(NAV_ITEMS.len() - 1);
            }
            KeyCode::Enter => self.open_nav_index(),
            // The home screen is the one place with nothing to go back to, so `Esc`
            // only clears the last message. It used to end the session from here,
            // which made backing out of a form one key away from losing the run.
            KeyCode::Esc => self.status = None,
            _ => {}
        }
    }

    /// Asks whether the user means to leave, naming what leaving would interrupt.
    pub(super) fn request_quit(&mut self) {
        self.status = Some((
            if self.screen == Screen::Running {
                "Leaving now cancels the running operation".to_string()
            } else {
                "Press y to leave sfumato".to_string()
            },
            false,
        ));
        self.overlay = Some(Overlay::Quit);
        self.dirty = true;
    }

    pub(super) fn open_section(&mut self, section: Section) {
        match load_section(section, &self.application) {
            Ok(rows) => {
                self.browse_rows = rows;
                self.browse_index = 0;
                self.browse_focus = if self.browse_rows.is_empty() {
                    BrowseFocus::Actions
                } else {
                    BrowseFocus::Rows
                };
                self.browse_action_index = 0;
                self.browse_detail_scroll = 0;
                self.connector_query_source = None;
                // Leaving the view abandons its result, so stop the work too.
                self.cancel_connector_query();
                self.status = None;
                self.transition(Screen::Browse(section));
            }
            Err(error) => self.status = Some((format!("{error:#}"), true)),
        }
    }

    pub(super) fn handle_browse_key(&mut self, section: Section, key: KeyEvent) {
        match key.code {
            // A running connector read takes priority: `Esc` stops it and keeps
            // the view, which is what the key was doing nothing about before.
            // With nothing running it leaves the section, as it always did.
            KeyCode::Esc | KeyCode::Backspace if self.cancel_connector_query() => {
                self.status = Some(("Stopped the connector read".to_string(), false));
            }
            KeyCode::Esc | KeyCode::Backspace => self.transition(Screen::Home),
            KeyCode::Tab | KeyCode::BackTab => {
                self.browse_focus = match self.browse_focus {
                    BrowseFocus::Actions => BrowseFocus::Rows,
                    BrowseFocus::Rows => BrowseFocus::Actions,
                };
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.browse_focus = BrowseFocus::Actions;
                self.browse_action_index = self.browse_action_index.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.browse_focus = BrowseFocus::Actions;
                self.browse_action_index = (self.browse_action_index + 1)
                    .min(section_actions(section).len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') if !self.browse_rows.is_empty() => {
                self.browse_focus = BrowseFocus::Rows;
                self.browse_index = self.browse_index.saturating_sub(1);
                self.browse_detail_scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') if !self.browse_rows.is_empty() => {
                self.browse_focus = BrowseFocus::Rows;
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
            KeyCode::Enter if self.browse_focus == BrowseFocus::Rows => {
                self.browse_focus = BrowseFocus::Actions;
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
        if matches!(
            action,
            BrowseAction::ConnectorModels | BrowseAction::ConnectorStatus
        ) {
            let Some(connector) = self.connector_query_source.clone().or_else(|| {
                self.browse_rows
                    .get(self.browse_index)
                    .map(|row| row.title.clone())
            }) else {
                self.status = Some(("Select a configured connector first".into(), true));
                return;
            };
            self.status = Some((
                format!("Loading native data from {connector}... press Esc to stop"),
                false,
            ));
            self.cancel_connector_query();
            self.connector_query = Some(spawn_connector_query(
                Arc::clone(&self.application),
                connector,
                action == BrowseAction::ConnectorModels,
                self.sender.clone(),
            ));
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
            BrowseAction::ConnectorSetup => OperationForm {
                title: "Setup connector",
                kind: OperationKind::ConnectorSetup,
                target: None,
                fields: vec![
                    FormField::Select {
                        label: "Preset",
                        options: ConnectorPreset::ALL
                            .into_iter()
                            .map(|preset| preset.as_str().to_string())
                            .collect(),
                        selected: 0,
                    },
                    // Left blank so the preset's own default name applies; the
                    // form cannot prefill it because the preset is chosen here.
                    text_field("Name", "", "connector name; defaults to the preset"),
                    // Present only while the selected preset accepts it; the
                    // preset-dependency pass adds and removes it as the choice
                    // moves, because `into_config` rejects it for Codex.
                    text_field(API_KEY_ENV_FIELD, "", "optional CI environment variable"),
                    submit_field("Save connector"),
                ],
                selected: 0,
            },
            BrowseAction::ConnectorModels | BrowseAction::ConnectorStatus => {
                anyhow::bail!("Connector discovery actions run asynchronously")
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
            BrowseAction::ThemeImport => OperationForm {
                title: "Import DESIGN.md",
                kind: OperationKind::ThemeImport,
                target: None,
                fields: vec![
                    text_field("Path", "", "/path/to/DESIGN.md"),
                    text_field("Name", "", "optional theme name"),
                    submit_field("Import theme"),
                ],
                selected: 0,
            },
            BrowseAction::ThemeExport => {
                let theme = selected.context("Select a theme to export")?;
                OperationForm {
                    title: "Export DESIGN.md",
                    kind: OperationKind::ThemeExport,
                    target: Some(theme.title.clone()),
                    fields: vec![
                        text_field("Path", "DESIGN.md", "output DESIGN.md"),
                        submit_field("Export theme"),
                    ],
                    selected: 0,
                }
            }
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
            BrowseAction::TemplateCreate => OperationForm {
                title: "Create template",
                kind: OperationKind::TemplateCreate,
                target: None,
                fields: vec![
                    text_field("Name", "", "lecture"),
                    text_field("Kind", "slides", "slides or page"),
                    text_field("Source", "", "optional source with SFUMATO_CONTENT marker"),
                    submit_field("Create template"),
                ],
                selected: 0,
            },
            BrowseAction::ArtifactAdd => OperationForm {
                title: "Add project artifact",
                kind: OperationKind::ArtifactAdd,
                target: None,
                fields: vec![
                    text_field("Path", "", "/path/to/logo.png"),
                    text_field("Name", "", "optional portable name"),
                    text_field("Description", "", "how generators should use it"),
                    text_field("Project", "", "blank for active project"),
                    submit_field("Add artifact"),
                ],
                selected: 0,
            },
            BrowseAction::ArtifactRemove => {
                let asset = selected.context("Select an artifact to remove")?;
                confirmation_form(
                    "Remove project artifact",
                    OperationKind::ArtifactRemove,
                    asset.title.clone(),
                    "Remove artifact",
                )
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
            BrowseAction::ProjectEdit => {
                let project = selected.context("Select a project to edit")?;
                // Prefilled from the project's own config, so the form shows what is set
                // rather than asking the caller to remember it. An empty field means the
                // project inherits, and clearing one writes that back as a delete.
                let config = self.application.show_project(Some(&project.title))?;
                let default_for = |capability: Capability| {
                    config
                        .model_defaults
                        .get(&capability)
                        .cloned()
                        .unwrap_or_default()
                };
                OperationForm {
                    title: "Edit project",
                    kind: OperationKind::ProjectEdit,
                    target: Some(project.title.clone()),
                    fields: vec![
                        choice_operation_field(
                            "Theme",
                            &config.theme,
                            "project theme",
                            ChoiceSource::Themes,
                        ),
                        choice_operation_field(
                            "Text model",
                            &default_for(Capability::Text),
                            "inherit from user config",
                            ChoiceSource::TextModels,
                        ),
                        choice_operation_field(
                            "Code model",
                            &default_for(Capability::Code),
                            "inherit from user config",
                            ChoiceSource::CodeModels,
                        ),
                        choice_operation_field(
                            "Image model",
                            &default_for(Capability::Image),
                            "inherit from user config",
                            ChoiceSource::ImageModels,
                        ),
                        choice_operation_field(
                            "Video model",
                            &default_for(Capability::Video),
                            "inherit from user config",
                            ChoiceSource::VideoModels,
                        ),
                        choice_operation_field(
                            "Speech model",
                            &default_for(Capability::Speech),
                            "inherit from user config",
                            ChoiceSource::SpeechModels,
                        ),
                        choice_operation_field(
                            "Reviewer",
                            config
                                .model_roles
                                .get(&ModelRole::Reviewer)
                                .map(String::as_str)
                                .unwrap_or_default(),
                            "inherit from user config",
                            ChoiceSource::ReviewerModels,
                        ),
                        submit_field("Save project"),
                    ],
                    selected: 0,
                }
            }
            BrowseAction::ToolEnable | BrowseAction::ToolDisable => {
                let tool = selected.context("Select a tool to switch")?;
                let enabling = action == BrowseAction::ToolEnable;
                confirmation_form(
                    if enabling {
                        "Enable tool"
                    } else {
                        "Disable tool"
                    },
                    OperationKind::ToolSet(enabling),
                    tool.title.clone(),
                    if enabling { "Enable" } else { "Disable" },
                )
            }
            BrowseAction::PluginEnable | BrowseAction::PluginDisable => {
                let plugin = selected.context("Select a plugin to switch")?;
                let enabling = action == BrowseAction::PluginEnable;
                confirmation_form(
                    if enabling {
                        "Enable plugin"
                    } else {
                        "Disable plugin"
                    },
                    OperationKind::PluginSet(enabling),
                    plugin.title.clone(),
                    if enabling { "Enable" } else { "Disable" },
                )
            }
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
                    text_field("Name", &crate::init::default_user_name(), "your name"),
                    text_field(
                        "Learning styles",
                        "visual, step-by-step",
                        "comma-separated preferences",
                    ),
                    // A select rather than free text: every preset is accepted
                    // here now, and the Profile and Model ID defaults below have
                    // to follow the chosen one instead of staying Ollama-shaped.
                    FormField::Select {
                        label: "Connector",
                        // Text-capable only: this form seeds the drafting
                        // profile, which a speech connector cannot back.
                        options: ConnectorPreset::text_capable()
                            .into_iter()
                            .map(|preset| preset.as_str().to_string())
                            .collect(),
                        selected: 0,
                    },
                    text_field(
                        "Profile",
                        ConnectorPreset::ALL[0].default_profile_name(),
                        "model profile name",
                    ),
                    text_field(
                        "Model ID",
                        ConnectorPreset::ALL[0].default_model(),
                        "provider model identifier",
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
            KeyCode::Left | KeyCode::Right => {
                if let Some(FormField::Select {
                    options, selected, ..
                }) = operation.fields.get_mut(operation.selected)
                    && !options.is_empty()
                {
                    *selected = if key.code == KeyCode::Left {
                        selected.checked_sub(1).unwrap_or(options.len() - 1)
                    } else {
                        (*selected + 1) % options.len()
                    };
                    operation.apply_select_dependencies();
                }
            }
            KeyCode::Enter => match operation.fields.get(operation.selected) {
                // A picker field has nothing to type into: enter opens the list.
                Some(FormField::Choice { .. }) => self.open_operation_choice_picker(),
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
            // Typing on a picker opens it with the first character already queried, so a
            // long list is searchable without a separate key first.
            KeyCode::Char(character)
                if matches!(
                    operation.fields.get(operation.selected),
                    Some(FormField::Choice { .. })
                ) =>
            {
                self.open_operation_choice_picker();
                if let Some(Overlay::Choice { query, .. }) = &mut self.overlay {
                    query.push(character);
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
        let before = self.form.field_id(self.form.selected);
        self.dispatch_generate_key(key);
        // Offered when focus leaves the sources field, not on every keystroke: a
        // half-typed path must never land in publish, and this way the filesystem is
        // consulted once per edit instead of once per character.
        if before == Some(GenerateFieldId::Sources)
            && self.form.field_id(self.form.selected) != Some(GenerateFieldId::Sources)
        {
            self.offer_publish_from_sources();
        }
    }

    /// Resolves the first source to a folder and offers it as the publish destination.
    ///
    /// A source may be a file or a directory, and "beside the sources" means the
    /// directory either way, so a file is resolved to its parent.
    fn offer_publish_from_sources(&mut self) {
        let sources = self.form.text(GenerateFieldId::Sources);
        let Some(first) = split_values(&sources).into_iter().next() else {
            return;
        };
        let path = Path::new(&first);
        let folder = if path.is_dir() {
            Some(path)
        } else {
            path.parent().filter(|parent| parent.is_dir())
        };
        if let Some(folder) = folder {
            self.form
                .offer_publish_folder(&folder.display().to_string());
        }
    }

    fn dispatch_generate_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.transition(Screen::Home),
            KeyCode::Up => self.form.selected = self.form.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                self.form.selected = (self.form.selected + 1).min(self.form.fields.len() - 1)
            }
            KeyCode::BackTab => self.form.selected = self.form.selected.saturating_sub(1),
            KeyCode::Left => self.move_form_choice(false),
            KeyCode::Right => self.move_form_choice(true),
            KeyCode::Enter => {
                let selected = self.form.selected;
                match self.form.fields.get(selected) {
                    // A picker field has nothing to type into: enter opens the list.
                    Some(FormField::Choice { .. }) => self.open_choice_picker(),
                    Some(FormField::Toggle { .. }) => self.toggle_form_field(),
                    Some(FormField::Select { .. }) => self.move_form_choice(true),
                    Some(FormField::MultiSelect { .. }) => self.toggle_form_field(),
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
                    Some(FormField::Toggle { .. } | FormField::MultiSelect { .. })
                ) =>
            {
                self.toggle_form_field();
            }
            // Typing on a picker opens it with the first character already in the
            // query, so searching a long list does not need a separate key first.
            KeyCode::Char(character)
                if matches!(
                    self.form.fields.get(self.form.selected),
                    Some(FormField::Choice { .. })
                ) =>
            {
                self.open_choice_picker();
                if let Some(Overlay::Choice { query, .. }) = &mut self.overlay {
                    query.push(character);
                }
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

    /// Opens the picker for the focused field, when it has one.
    pub(super) fn open_choice_picker(&mut self) {
        if let Some(field) = self.form.focused_choice() {
            self.overlay = Some(Overlay::choice(ChoiceTarget::Generate(field)));
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
        match self.form.fields.get_mut(self.form.selected) {
            Some(FormField::Toggle { value, .. }) => *value = !*value,
            Some(FormField::MultiSelect {
                cursor,
                selected,
                options,
                ..
            }) if !options.is_empty() => {
                let was_selected = !selected.insert(*cursor);
                if was_selected {
                    selected.remove(cursor);
                }
            }
            _ => {}
        }
    }

    pub(super) fn move_form_choice(&mut self, forward: bool) {
        let field_id = self.form.field_id(self.form.selected);
        match self.form.fields.get_mut(self.form.selected) {
            Some(FormField::Select {
                options, selected, ..
            }) if !options.is_empty() => {
                *selected = if forward {
                    (*selected + 1) % options.len()
                } else {
                    selected.checked_sub(1).unwrap_or(options.len() - 1)
                };
            }
            Some(FormField::MultiSelect {
                options, cursor, ..
            }) if !options.is_empty() => {
                *cursor = if forward {
                    (*cursor + 1) % options.len()
                } else {
                    cursor.checked_sub(1).unwrap_or(options.len() - 1)
                };
            }
            _ => {}
        }
        if field_id == Some(GenerateFieldId::Resource) {
            self.form.switch_resource_from_selector();
        } else if field_id == Some(GenerateFieldId::Engine) {
            self.form.switch_video_engine_from_selector();
        }
    }

    pub(super) fn start_generation(&mut self) {
        enum PreparedGeneration {
            Slides(SlidesArgs),
            Document(DocumentArgs),
            Page(PageArgs),
            Video(VideoArgs),
        }
        let prepared = match self.form.resource {
            GenerateResource::Slides => self.form.to_slides_args().map(PreparedGeneration::Slides),
            GenerateResource::Page => self.form.to_page_args().map(PreparedGeneration::Page),
            GenerateResource::Video => self.form.to_video_args().map(PreparedGeneration::Video),
            GenerateResource::Document => self
                .form
                .to_document_args()
                .map(PreparedGeneration::Document),
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
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
        self.resource_operation = match self.form.resource {
            GenerateResource::Slides => ResourceOperation::Generate,
            GenerateResource::Page => ResourceOperation::GeneratePage,
            GenerateResource::Video => ResourceOperation::GenerateVideo,
            GenerateResource::Document => ResourceOperation::GenerateDocument,
        };
        self.transition(Screen::Running);

        let (job_id, operation) = self.begin_job();
        let sender = self.sender.clone();
        let application = Arc::clone(&self.application);
        let sink = effects::generation_event_sink(job_id, sender.clone());
        self.active_task = Some(match prepared {
            PreparedGeneration::Page(args) => {
                effects::spawn_page_generation(job_id, application, args, sink, operation, sender)
            }
            PreparedGeneration::Slides(args) => {
                effects::spawn_generation(job_id, application, args, sink, operation, sender)
            }
            PreparedGeneration::Video(args) => {
                effects::spawn_video_generation(job_id, application, args, sink, operation, sender)
            }
            PreparedGeneration::Document(args) => effects::spawn_document_generation(
                job_id,
                application,
                args,
                sink,
                operation,
                sender,
            ),
        });
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
        self.started_at = Some(std::time::Instant::now());
        let job_id = self.jobs.next_job_id();
        let events = effects::operation_event_sink(job_id, self.sender.clone());
        self.jobs.begin(events)
    }

    pub(super) fn cancel_active_job(&self) {
        self.jobs.cancel();
    }

    /// Stops an in-flight native connector read, if there is one.
    pub(super) fn cancel_connector_query(&mut self) -> bool {
        match self.connector_query.take() {
            Some(query) => {
                query.cancel();
                true
            }
            None => false,
        }
    }

    pub(super) async fn shutdown(&mut self) {
        self.cancel_active_job();
        if let Some(query) = self.connector_query.take() {
            query.cancel_and_join().await;
        }
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
                ResourceOperation::Generate
                | ResourceOperation::GenerateDocument
                | ResourceOperation::GeneratePage
                | ResourceOperation::GenerateVideo => Screen::Generate,
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
                // A run that failed has already deleted its staging tree, so its assets
                // are gone by design. Reporting that as an error made a correct discard
                // read like a second fault on top of the first.
                self.status = Some(if path.exists() {
                    (
                        format!("Could not preview {}: {error}", path.display()),
                        true,
                    )
                } else {
                    (
                        "That preview is no longer on disk: an unfinished run discards \
                         everything it generated"
                            .to_string(),
                        false,
                    )
                });
            }
        }
    }
}

/// Rewrites staging preview paths onto the revision the run committed.
///
/// A generation writes assets into `.staging/<job-id>/…` and commits by moving that tree
/// to `revisions/<rev-id>/…`, so the suffix after the job directory is preserved exactly.
/// `committed` is any file inside the new revision — the resource's own output — and its
/// parent is the revision root.
///
/// A path that cannot be re-rooted is left as it is rather than guessed at: an unchanged
/// path fails with the name the tool actually reported, which is the more useful error.
pub(super) fn reroot_previews(activities: &mut [Activity], committed: &std::path::Path) {
    let Some(revision_root) = committed.parent() else {
        return;
    };
    for activity in activities {
        let Some(path) = activity.image_path.as_ref() else {
            continue;
        };
        if path.exists() {
            continue;
        }
        if let Some(relative) = staging_suffix(path) {
            let candidate = revision_root.join(relative);
            if candidate.exists() {
                activity.image_path = Some(candidate);
            }
        }
    }
}

/// The part of a staging path below its job directory.
///
/// Matched on the `.staging` component rather than on a prefix, because the caller knows
/// the revision root but not the job root, and the two share only this suffix.
fn staging_suffix(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut components = path.components();
    components.find(|component| component.as_os_str() == ".staging")?;
    // Immediately after `.staging` comes the job directory, which the revision replaces.
    components.next()?;
    let suffix: std::path::PathBuf = components.collect();
    (!suffix.as_os_str().is_empty()).then_some(suffix)
}
