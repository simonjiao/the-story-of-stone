use std::fmt;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmMode {
    Disabled,
    Shadow,
    Enforced,
}

impl LlmMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim() {
            "" | "disabled" => Ok(Self::Disabled),
            "shadow" => Ok(Self::Shadow),
            "enforced" => Ok(Self::Enforced),
            other => Err(anyhow!("unsupported llm mode: {other}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Shadow => "shadow",
            Self::Enforced => "enforced",
        }
    }
}

impl fmt::Display for LlmMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_mode_parse_defaults_disabled_for_empty_value() {
        assert_eq!(LlmMode::parse("").expect("parse"), LlmMode::Disabled);
        assert_eq!(LlmMode::parse("shadow").expect("parse"), LlmMode::Shadow);
        assert!(LlmMode::parse("auto").is_err());
    }
}
