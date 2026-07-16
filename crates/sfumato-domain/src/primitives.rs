use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

const MAX_VALUE_LENGTH: usize = 128;
const MAX_SECRET_REF_LENGTH: usize = 512;

/// An error returned when a domain string does not satisfy its invariant.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ValueError {
    /// The value is empty.
    #[error("{kind} cannot be empty")]
    Empty {
        /// The kind of value being validated.
        kind: &'static str,
    },
    /// The value is longer than the domain permits.
    #[error("{kind} cannot exceed {max} bytes")]
    TooLong {
        /// The kind of value being validated.
        kind: &'static str,
        /// The maximum accepted byte length.
        max: usize,
    },
    /// The value has leading or trailing whitespace.
    #[error("{kind} cannot have leading or trailing whitespace")]
    SurroundingWhitespace {
        /// The kind of value being validated.
        kind: &'static str,
    },
    /// The value does not match the portable token syntax.
    #[error("invalid {kind} `{value}`; use letters, numbers, dots, underscores, and hyphens")]
    InvalidToken {
        /// The kind of value being validated.
        kind: &'static str,
        /// The rejected value.
        value: String,
    },
    /// The value does not match the lowercase slug syntax.
    #[error("invalid {kind} `{value}`; use lowercase letters, numbers, and hyphens")]
    InvalidSlug {
        /// The kind of value being validated.
        kind: &'static str,
        /// The rejected value.
        value: String,
    },
    /// A project name could be interpreted as a path.
    #[error("project name `{value}` cannot contain path separators or traversal")]
    InvalidProjectName {
        /// The rejected project name.
        value: String,
    },
    /// A secret reference does not contain a valid scheme and target.
    #[error("invalid secret reference `{value}`; expected `<scheme>:<target>`")]
    InvalidSecretRef {
        /// The rejected reference.
        value: String,
    },
}

fn validate_length(value: &str, kind: &'static str, max: usize) -> Result<(), ValueError> {
    if value.is_empty() {
        return Err(ValueError::Empty { kind });
    }
    if value.len() > max {
        return Err(ValueError::TooLong { kind, max });
    }
    if value.trim() != value {
        return Err(ValueError::SurroundingWhitespace { kind });
    }
    Ok(())
}

fn validate_token(value: &str, kind: &'static str) -> Result<(), ValueError> {
    validate_length(value, kind, MAX_VALUE_LENGTH)?;
    let valid_character =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.');
    if !value.chars().all(valid_character)
        || !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        || !value
            .chars()
            .last()
            .is_some_and(|character| character.is_ascii_alphanumeric())
    {
        return Err(ValueError::InvalidToken {
            kind,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_slug(value: &str, kind: &'static str) -> Result<(), ValueError> {
    validate_length(value, kind, MAX_VALUE_LENGTH)?;
    if !value.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
    }) || value.starts_with('-')
        || value.ends_with('-')
    {
        return Err(ValueError::InvalidSlug {
            kind,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_project_name(value: &str, _: &'static str) -> Result<(), ValueError> {
    validate_length(value, "project name", MAX_VALUE_LENGTH)?;
    if matches!(value, "." | "..")
        || value.contains(['/', '\\'])
        || value.chars().any(char::is_control)
    {
        return Err(ValueError::InvalidProjectName {
            value: value.to_owned(),
        });
    }
    Ok(())
}

macro_rules! string_value {
    ($(#[$meta:meta])* $name:ident, $kind:literal, $validator:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated value.
            pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
                let value = value.into();
                $validator(&value, $kind)?;
                Ok(Self(value))
            }

            /// Returns the value as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the value and returns its owned string.
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl FromStr for $name {
            type Err = ValueError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ValueError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = ValueError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

string_value!(
    /// Identifies one execution of a Sfumato job.
    JobId,
    "job ID",
    validate_token
);
string_value!(
    /// Identifies an immutable revision of domain content.
    RevisionId,
    "revision ID",
    validate_token
);
string_value!(
    /// Identifies an artifact produced or consumed by a job.
    ArtifactId,
    "artifact ID",
    validate_token
);
string_value!(
    /// Names a project without carrying any filesystem location.
    ProjectName,
    "project name",
    validate_project_name
);
string_value!(
    /// Names a model profile using a portable lowercase slug.
    ModelProfileName,
    "model profile name",
    validate_slug
);
string_value!(
    /// Names a theme using a portable lowercase slug.
    ThemeName,
    "theme name",
    validate_slug
);

/// A capability that a model can provide.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// Natural-language text generation or understanding.
    Text,
    /// Source-code generation or understanding.
    Code,
    /// Image generation or understanding.
    Image,
    /// Video generation or understanding.
    Video,
    /// Speech or audio generation or understanding.
    Speech,
    /// Vector embedding generation.
    Embedding,
}

impl Capability {
    /// Returns the stable lowercase representation used by configuration DTOs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Code => "code",
            Self::Image => "image",
            Self::Video => "video",
            Self::Speech => "speech",
            Self::Embedding => "embedding",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An error returned when parsing an unknown model capability.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("unknown capability `{value}`; use text, code, image, video, speech, or embedding")]
pub struct CapabilityParseError {
    /// The rejected capability name.
    pub value: String,
}

impl FromStr for Capability {
    type Err = CapabilityParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "code" => Ok(Self::Code),
            "image" => Ok(Self::Image),
            "video" => Ok(Self::Video),
            "speech" => Ok(Self::Speech),
            "embedding" => Ok(Self::Embedding),
            _ => Err(CapabilityParseError {
                value: value.to_owned(),
            }),
        }
    }
}

/// An indirect reference to a secret, such as `env:OPENAI_API_KEY`.
///
/// The value identifies where infrastructure code should resolve a secret; it
/// never contains the secret itself.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    /// Creates a reference from a scheme and target.
    pub fn new(scheme: &str, target: &str) -> Result<Self, ValueError> {
        Self::try_from(format!("{scheme}:{target}"))
    }

    /// Creates an environment-variable secret reference.
    pub fn environment(variable: &str) -> Result<Self, ValueError> {
        if variable.is_empty()
            || !variable.chars().all(|character| {
                character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
            })
            || !variable
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_uppercase() || character == '_')
        {
            return Err(ValueError::InvalidSecretRef {
                value: format!("env:{variable}"),
            });
        }
        Self::new("env", variable)
    }

    /// Returns the complete opaque reference.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the reference scheme.
    pub fn scheme(&self) -> &str {
        self.0
            .split_once(':')
            .expect("validated secret reference")
            .0
    }

    /// Returns the scheme-specific target.
    pub fn target(&self) -> &str {
        self.0
            .split_once(':')
            .expect("validated secret reference")
            .1
    }
}

impl TryFrom<String> for SecretRef {
    type Error = ValueError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_length(&value, "secret reference", MAX_SECRET_REF_LENGTH)?;
        let Some((scheme, target)) = value.split_once(':') else {
            return Err(ValueError::InvalidSecretRef { value });
        };
        let valid_scheme = !scheme.is_empty()
            && scheme
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_lowercase())
            && scheme.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '+' | '-' | '.')
            });
        let valid_target = !target.is_empty()
            && !target
                .chars()
                .any(|character| character.is_control() || character.is_whitespace());
        if !valid_scheme || !valid_target {
            return Err(ValueError::InvalidSecretRef { value });
        }
        Ok(Self(value))
    }
}

impl From<SecretRef> for String {
    fn from(value: SecretRef) -> Self {
        value.0
    }
}

impl TryFrom<&str> for SecretRef {
    type Error = ValueError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl FromStr for SecretRef {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value)
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(de::Error::custom)
    }
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/primitives.rs"]
mod tests;
