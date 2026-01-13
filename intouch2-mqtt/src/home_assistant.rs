use std::{borrow::Cow, collections::HashMap, sync::Arc};

#[derive(serde::Serialize, Clone)]
pub struct ConfigureDevice {
    pub identifiers: Box<[Arc<str>]>,
    pub name: Arc<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sw_version: Option<Arc<str>>,
    //#[serde(skip_serializing_if = "Option::is_none")]
    // pub configuration_url: Option<Arc<str>>,
    #[serde(flatten)]
    pub extra_args: HashMap<&'static str, serde_json::Value>,
}

#[derive(serde::Serialize)]
pub struct ConfigureBase<'a> {
    pub name: Cow<'a, str>,
    pub unique_id: Cow<'a, str>,
    pub device: Cow<'a, ConfigureDevice>,
    pub qos: u8,
}

#[derive(serde::Serialize)]
pub struct ConfigureGeneric<'a> {
    #[serde(flatten)]
    pub base: ConfigureBase<'a>,
    #[serde(flatten)]
    pub args: HashMap<&'a str, serde_json::Value>,
}
