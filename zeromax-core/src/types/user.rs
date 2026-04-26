use serde::{Deserialize, Serialize};

/// User name entry.
///
/// Mirrors `Name` / `Names` from `pymax/types.py:46-100`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(rename = "type", default)]
    pub name_type: Option<String>,
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name.as_deref().unwrap_or(""))
    }
}

/// Presence (last seen).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presence {
    #[serde(default)]
    pub seen: Option<i64>,
}

/// Contact entry from sync/contact list.
///
/// Mirrors `Contact` from `pymax/types.py:103-164`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub account_status: Option<i32>,
    #[serde(default)]
    pub base_raw_url: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub names: Vec<Name>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub photo_id: Option<i64>,
    #[serde(default)]
    pub update_time: Option<i64>,
}

/// Chat member.
///
/// Mirrors `Member` from `pymax/types.py:167-213`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    #[serde(default)]
    pub contact: Option<Contact>,
    #[serde(default)]
    pub presence: Option<Presence>,
    #[serde(default)]
    pub read_mark: Option<i64>,
}

/// Full user profile.
///
/// Mirrors `User` from `pymax/types.py:952-1007`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub account_status: i32,
    #[serde(default)]
    pub update_time: i64,
    #[serde(default)]
    pub names: Vec<Name>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub base_raw_url: Option<String>,
    #[serde(default)]
    pub photo_id: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub gender: Option<i32>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub web_app: Option<String>,
    #[serde(default)]
    pub menu_button: Option<serde_json::Value>,
}

/// Current user profile (self).
///
/// Mirrors `Me` from `pymax/types.py:514-548`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub id: i64,
    #[serde(default)]
    pub account_status: i32,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub phone: String,
    #[serde(default)]
    pub names: Vec<Name>,
    #[serde(default)]
    pub update_time: i64,
    #[serde(default)]
    pub options: Option<Vec<String>>,
}

/// Deserialize a field that can be either a string or a number into String.
fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrNumber;
    impl<'de> de::Visitor<'de> for StringOrNumber {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("string or number")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> { Ok(v.to_string()) }
        fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> { Ok(v) }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> { Ok(v.to_string()) }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> { Ok(v.to_string()) }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<String, E> { Ok(v.to_string()) }
    }
    deserializer.deserialize_any(StringOrNumber)
}
