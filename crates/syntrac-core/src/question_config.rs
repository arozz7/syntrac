//! Phase 5, slice B3 (docs/PHASE5-PLAN.md): the wording of the question box. Loaded from `config/ai/question.toml`, so prompts and tool
//! descriptions are changed without touching code.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct QuestionConfig {
    pub system: String,
    /// Sent with the last turn, which is made without tools.
    pub final_request: String,
    /// What each tool does, in the model's words, by tool name.
    pub tools: BTreeMap<String, String>,
}

impl QuestionConfig {
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// The wording shipped with the program.
    pub fn builtin() -> Self {
        Self::from_toml(include_str!("../../../config/ai/question.toml")).expect("config/ai/question.toml is valid")
    }

    /// What the tool does, or an empty text for a name the config does not know.
    pub fn describe(&self, tool: &str) -> String {
        self.tools.get(tool).map(|text| text.trim().to_owned()).unwrap_or_default()
    }
}
