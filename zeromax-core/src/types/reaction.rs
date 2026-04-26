use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactionCounter {
    pub count: i32,
    pub reaction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactionInfo {
    #[serde(default)]
    pub total_count: i32,
    #[serde(default)]
    pub counters: Vec<ReactionCounter>,
    #[serde(default)]
    pub your_reaction: Option<String>,
}
