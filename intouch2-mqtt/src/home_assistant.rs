#[derive(serde::Serialize)]
pub struct ConfigureDevice<'a> {
    pub identifiers: Box<[&'a str]>,
    pub name: &'a str,
}

#[derive(serde::Serialize)]
pub struct ConfigureBase<'a> {
    pub name: &'a str,
    pub optimistic: bool,
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
    pub unique_id: &'a str,
    pub color_mode: Option<&'a str>,
    pub device: &'a ConfigureDevice<'a>,
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
    pub unique_id: &'a str,
    pub device: &'a ConfigureDevice<'a>,
}
