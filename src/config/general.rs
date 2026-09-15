use std::collections::HashMap;

use super::model::{ConfigSection, get_or, parse_bool, parse_list};

/// Mirrors `aria_shell.config.AriaConfigGeneralModel` (the `[general]`
/// section). Not used for anything functional in this spike (the Clock
/// module is hard-registered, not read from `modules`, see
/// `modules::request_gadget`) but implemented fully now since every
/// future module hangs off of it.
// Not constructed anywhere yet in this spike -- see the `#[allow(dead_code)]`
// note on `AriaConfig::general()`.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct GeneralConfig {
    pub modules: Vec<String>,
    pub style: String,
    pub reload_config: bool,
    pub reload_style: bool,
}

impl ConfigSection for GeneralConfig {
    const SECTION: &'static str = "general";

    fn from_section(raw: &HashMap<String, String>) -> Self {
        Self {
            modules: raw
                .get("modules")
                .map(|v| parse_list(v))
                .unwrap_or_default(),
            style: get_or(raw, "style", ""),
            reload_config: raw
                .get("reload_config")
                .and_then(|v| parse_bool(v))
                .unwrap_or(false),
            reload_style: raw
                .get("reload_style")
                .and_then(|v| parse_bool(v))
                .unwrap_or(false),
        }
    }
}
