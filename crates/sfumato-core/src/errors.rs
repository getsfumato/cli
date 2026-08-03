//! Stable errors returned by Sfumato application operations.
//!
//! [`ErrorCode`] identifies the subsystem that rejected an operation, while
//! [`ErrorClass`] describes the recovery strategy available to a caller. This
//! separation keeps presentation and retry policy independent from adapter
//! implementation details.

use std::{any::Any, collections::BTreeMap, error::Error, fmt};

use serde::{Deserialize, Serialize};

/// Stable subsystem-level code for a public Sfumato error.
///
/// Codes are intentionally broader than adapter-specific failures. Details
/// such as HTTP status text, process output, and secret-bearing payloads must
/// be sanitized before they cross an application boundary.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Configuration could not be loaded, resolved, or validated.
    Config,
    /// User input or generated data violated a domain invariant.
    Validation,
    /// A requested project, model, theme, prompt, or artifact was not found.
    NotFound,
    /// A model provider request failed.
    Provider,
    /// A model-requested tool failed.
    Tool,
    /// A renderer or layout inspector failed.
    Render,
    /// Artifact staging, commit, or publication failed.
    Artifact,
    /// The operation was cancelled or exceeded its deadline.
    Cancelled,
    /// An unexpected application failure occurred.
    Internal,
}

impl ErrorCode {
    /// Returns the stable snake-case representation used by presentation DTOs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Validation => "validation",
            Self::NotFound => "not_found",
            Self::Provider => "provider",
            Self::Tool => "tool",
            Self::Render => "render",
            Self::Artifact => "artifact",
            Self::Cancelled => "cancelled",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Recovery classification for a public Sfumato error.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// Repeating the same operation may succeed after bounded backoff.
    Retry,
    /// The model context must be compacted before another attempt.
    ContextLimit,
    /// Generated output may be retried with focused validation feedback.
    InvalidOutput,
    /// A dependency is unavailable and the operation may be retried later.
    Unavailable,
    /// The caller or operation deadline deliberately stopped the operation.
    Cancelled,
    /// Retrying unchanged input cannot resolve the failure.
    Permanent,
}

impl ErrorClass {
    /// Returns whether a bounded recovery attempt can be meaningful.
    ///
    /// Callers must still apply the strategy implied by the class: context
    /// limits require compaction and invalid output requires corrective
    /// feedback rather than an identical blind retry.
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Retry | Self::ContextLimit | Self::InvalidOutput | Self::Unavailable
        )
    }

    /// Returns the stable snake-case representation used by presentation DTOs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::ContextLimit => "context_limit",
            Self::InvalidOutput => "invalid_output",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
            Self::Permanent => "permanent",
        }
    }
}

impl fmt::Display for ErrorClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Coarse workflow stage associated with an operation error or event.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStage {
    /// Resolve configuration, project, models, and themes.
    Resolve,
    /// Discover and read source material.
    ReadSources,
    /// Resolve and render prompt templates.
    RenderPrompt,
    /// Generate an initial resource draft.
    Draft,
    /// Apply focused edits to an existing resource.
    Edit,
    /// Review generated content.
    Review,
    /// Inspect rendered layout and media fitting.
    InspectLayout,
    /// Repair invalid content, diagrams, or layout.
    Repair,
    /// Render final resource files.
    Render,
    /// Validate and commit an artifact transaction.
    CommitArtifacts,
    /// Publish processed artifacts outside the managed store.
    Publish,
}

impl OperationStage {
    /// Returns the stable snake-case representation used by presentation DTOs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resolve => "resolve",
            Self::ReadSources => "read_sources",
            Self::RenderPrompt => "render_prompt",
            Self::Draft => "draft",
            Self::Edit => "edit",
            Self::Review => "review",
            Self::InspectLayout => "inspect_layout",
            Self::Repair => "repair",
            Self::Render => "render",
            Self::CommitArtifacts => "commit_artifacts",
            Self::Publish => "publish",
        }
    }
}

impl fmt::Display for OperationStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A stable, user-safe failure returned by a Sfumato operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SfumatoError {
    /// Stable subsystem code.
    pub code: ErrorCode,
    /// Recovery strategy available to the caller.
    pub class: ErrorClass,
    /// Whether a bounded recovery attempt can be meaningful.
    pub retryable: bool,
    /// Workflow stage at which the failure occurred, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<OperationStage>,
    /// Human-readable message that is safe to present to a user.
    pub message: String,
    /// Sanitized structured context for machine-readable presentation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

impl SfumatoError {
    /// Creates a typed public error.
    ///
    /// Use [`SfumatoError::cancelled`] for cancellation so the cancellation
    /// code and class cannot diverge.
    pub fn new(code: ErrorCode, class: ErrorClass, message: impl Into<String>) -> Self {
        let (code, class) = if code == ErrorCode::Cancelled || class == ErrorClass::Cancelled {
            (ErrorCode::Cancelled, ErrorClass::Cancelled)
        } else {
            (code, class)
        };
        Self {
            code,
            class,
            retryable: class.is_retryable(),
            stage: None,
            message: message.into(),
            details: BTreeMap::new(),
        }
    }

    /// Creates a public error from adapter or workflow text after redaction.
    pub fn sanitized(code: ErrorCode, class: ErrorClass, message: impl fmt::Display) -> Self {
        Self::new(code, class, sanitize_message(&message.to_string()))
    }

    /// Creates a permanent configuration error.
    pub fn config(message: impl fmt::Display) -> Self {
        Self::sanitized(ErrorCode::Config, ErrorClass::Permanent, message)
    }

    /// Creates a permanent validation error.
    pub fn validation(message: impl fmt::Display) -> Self {
        Self::sanitized(ErrorCode::Validation, ErrorClass::Permanent, message)
    }

    /// Creates a permanent missing-resource error.
    pub fn not_found(message: impl fmt::Display) -> Self {
        Self::sanitized(ErrorCode::NotFound, ErrorClass::Permanent, message)
    }

    /// Creates a provider error with an explicit recovery classification.
    pub fn provider(class: ErrorClass, message: impl fmt::Display) -> Self {
        Self::sanitized(ErrorCode::Provider, class, message)
    }

    /// Creates a tool error with an explicit recovery classification.
    pub fn tool(class: ErrorClass, message: impl fmt::Display) -> Self {
        Self::sanitized(ErrorCode::Tool, class, message)
    }

    /// Creates a renderer error with an explicit recovery classification.
    pub fn render(class: ErrorClass, message: impl fmt::Display) -> Self {
        Self::sanitized(ErrorCode::Render, class, message)
    }

    /// Creates an artifact error with an explicit recovery classification.
    pub fn artifact(class: ErrorClass, message: impl fmt::Display) -> Self {
        Self::sanitized(ErrorCode::Artifact, class, message)
    }

    /// Creates a permanent unexpected application error.
    pub fn internal(message: impl fmt::Display) -> Self {
        Self::sanitized(ErrorCode::Internal, ErrorClass::Permanent, message)
    }

    /// Creates a cooperative-cancellation error at an optional stage.
    pub fn cancelled(stage: Option<OperationStage>) -> Self {
        Self {
            code: ErrorCode::Cancelled,
            class: ErrorClass::Cancelled,
            retryable: false,
            stage,
            message: "operation cancelled".to_string(),
            details: BTreeMap::new(),
        }
    }

    /// Creates a deadline-expiration error at an optional stage.
    pub fn deadline_exceeded(stage: Option<OperationStage>) -> Self {
        Self {
            code: ErrorCode::Cancelled,
            class: ErrorClass::Cancelled,
            retryable: false,
            stage,
            message: "operation deadline exceeded".to_string(),
            details: BTreeMap::from([("reason".to_string(), "deadline_exceeded".to_string())]),
        }
    }

    /// Associates this error with a workflow stage.
    #[must_use]
    pub fn at_stage(mut self, stage: OperationStage) -> Self {
        self.stage = Some(stage);
        self
    }

    /// Adds one sanitized detail value.
    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }

    /// Prepends safe operation context while preserving classification details.
    #[must_use]
    pub fn context(mut self, context: impl fmt::Display) -> Self {
        self.message = sanitize_message(&format!("{context}: {}", self.message));
        self
    }

    /// Returns whether a bounded recovery attempt can be meaningful.
    pub const fn is_retryable(&self) -> bool {
        self.retryable
    }
}

impl fmt::Display for SfumatoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(stage) = self.stage {
            write!(formatter, "{} during {stage}: {}", self.code, self.message)
        } else {
            write!(formatter, "{}: {}", self.code, self.message)
        }
    }
}

impl Error for SfumatoError {}

impl From<crate::artifacts::ArtifactStoreError> for SfumatoError {
    fn from(error: crate::artifacts::ArtifactStoreError) -> Self {
        // A held project lock is the one artifact failure that clears on its own,
        // so it is classified as such rather than told to the caller as permanent.
        let class = match error {
            crate::artifacts::ArtifactStoreError::Busy(_) => ErrorClass::Retry,
            _ => ErrorClass::Permanent,
        };
        Self::artifact(class, error).at_stage(OperationStage::CommitArtifacts)
    }
}

impl From<sfumato_domain::ReviewError> for SfumatoError {
    fn from(error: sfumato_domain::ReviewError) -> Self {
        Self::new(
            ErrorCode::Validation,
            ErrorClass::InvalidOutput,
            error.to_string(),
        )
    }
}

impl From<crate::prompts::PromptError> for SfumatoError {
    fn from(error: crate::prompts::PromptError) -> Self {
        Self::config(error).at_stage(OperationStage::RenderPrompt)
    }
}

impl From<serde_json::Error> for SfumatoError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(
            ErrorCode::Validation,
            ErrorClass::InvalidOutput,
            error.to_string(),
        )
    }
}

/// Result type returned by public Sfumato operations.
pub type SfumatoResult<T> = Result<T, SfumatoError>;

/// Adds user-safe context to fallible parsing and validation operations.
///
/// This deliberately classifies otherwise untyped failures as permanent
/// validation errors. Infrastructure ports must classify their own errors
/// before crossing into core, so this helper is reserved for local parsing,
/// serialization, and invariant checks.
pub trait ResultContext<T> {
    /// Adds a static or eagerly formatted context message.
    fn context(self, context: impl fmt::Display) -> SfumatoResult<T>;

    /// Adds a lazily formatted context message.
    fn with_context<M>(self, context: impl FnOnce() -> M) -> SfumatoResult<T>
    where
        M: fmt::Display;
}

impl<T, E> ResultContext<T> for Result<T, E>
where
    E: fmt::Display + 'static,
{
    fn context(self, context: impl fmt::Display) -> SfumatoResult<T> {
        self.map_err(|error| contextualize_error(error, context))
    }

    fn with_context<M>(self, context: impl FnOnce() -> M) -> SfumatoResult<T>
    where
        M: fmt::Display,
    {
        self.map_err(|error| contextualize_error(error, context()))
    }
}

fn contextualize_error(
    error: impl fmt::Display + 'static,
    context: impl fmt::Display,
) -> SfumatoError {
    if let Some(error) = (&error as &dyn Any).downcast_ref::<SfumatoError>() {
        error.clone().context(context)
    } else {
        SfumatoError::validation(format!("{context}: {error}"))
    }
}

impl<T> ResultContext<T> for Option<T> {
    fn context(self, context: impl fmt::Display) -> SfumatoResult<T> {
        self.ok_or_else(|| SfumatoError::validation(context))
    }

    fn with_context<M>(self, context: impl FnOnce() -> M) -> SfumatoResult<T>
    where
        M: fmt::Display,
    {
        self.ok_or_else(|| SfumatoError::validation(context()))
    }
}

/// Returns a permanent validation error from the current function.
#[macro_export]
macro_rules! sfumato_bail {
    ($message:literal $(, $argument:expr)* $(,)?) => {
        return Err($crate::errors::SfumatoError::validation(format!(
            $message $(, $argument)*
        )))
    };
    ($error:expr $(,)?) => {
        return Err($crate::errors::SfumatoError::validation($error))
    };
}

fn sanitize_message(message: &str) -> String {
    let mut sanitized = message
        .split_whitespace()
        .map(redact_token)
        .collect::<Vec<_>>()
        .join(" ");
    const MAX_MESSAGE_CHARS: usize = 2_000;
    if sanitized.chars().count() > MAX_MESSAGE_CHARS {
        let boundary = sanitized
            .char_indices()
            .nth(MAX_MESSAGE_CHARS)
            .map(|(index, _)| index)
            .unwrap_or(sanitized.len());
        sanitized.truncate(boundary);
        sanitized.push_str("...");
    }
    sanitized
}

fn redact_token(token: &str) -> String {
    let normalized = token.trim_matches(|character: char| {
        matches!(character, '"' | '\'' | ',' | ':' | '{' | '}' | '[' | ']')
    });
    if normalized.starts_with("sk-")
        || normalized.starts_with("sk_or_")
        || normalized.starts_with("sk-or-")
        || normalized.len() > 80
            && normalized
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
    {
        token.replace(normalized, "[REDACTED]")
    } else {
        token.to_string()
    }
}
