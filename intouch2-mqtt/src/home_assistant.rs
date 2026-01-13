use std::{borrow::Cow, collections::HashMap, sync::Arc};

#[derive(serde::Serialize, Clone)]
pub struct ConfigureDevice {
    pub identifiers: Box<[Arc<str>]>,
    pub name: Arc<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sw_version: Option<Arc<str>>,
    #[serde(flatten)]
    pub extra_args: HashMap<&'static str, serde_json::Value>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AvailabilityMapping<'a> {
    pub payload_available: Option<Cow<'a, str>>,
    pub payload_not_available: Option<Cow<'a, str>>,
    pub topic: Cow<'a, str>,
    pub value_template: Option<Cow<'a, str>>,
}

#[derive(serde::Serialize)]
pub struct ConfigureBase<'a> {
    pub name: Cow<'a, str>,
    pub unique_id: Cow<'a, str>,
    pub device: Cow<'a, ConfigureDevice>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub availability: Vec<AvailabilityMapping<'a>>,
    pub qos: u8,
}

#[derive(serde::Serialize)]
pub struct ConfigureGeneric<'a> {
    #[serde(flatten)]
    pub base: ConfigureBase<'a>,
    #[serde(flatten)]
    pub args: HashMap<&'a str, serde_json::Value>,
}

#[derive(serde::Serialize)]
pub struct ConfigureLight<'a> {
    #[serde(flatten)]
    pub base: ConfigureBase<'a>,
    pub command_topic: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect_command_topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect_state_topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rgb_command_topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rgb_state_topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect_list: Option<Box<[&'a str]>>,
    pub color_mode: Option<&'a str>,
    pub optimistic: bool,
}

#[derive(serde::Serialize)]
pub struct ConfigureFan<'a> {
    #[serde(flatten)]
    pub base: ConfigureBase<'a>,
    pub command_topic: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage_command_topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage_state_topic: Option<&'a str>,
    pub optimistic: bool,
}

#[derive(serde::Serialize)]
pub struct ConfigureClimate<'a> {
    #[serde(flatten)]
    pub base: ConfigureBase<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_state_topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_temperature_topic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_unit: Option<&'a str>,
    pub optimistic: bool,
}

#[derive(serde::Serialize)]
pub struct ConfigureSelect<'a> {
    #[serde(flatten)]
    pub base: ConfigureBase<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_topic: Option<&'a str>,
    pub command_topic: &'a str,
    pub options: Vec<&'a str>,
    pub optimistic: bool,
}
