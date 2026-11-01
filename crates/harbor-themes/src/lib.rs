use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeId {
    #[default]
    Original,
    Terra,
    Clarity,
    Nocturne,
    TurboClassic,
}
impl ThemeId {
    pub const DESKTOP: [Self; 4] = [Self::Original, Self::Terra, Self::Clarity, Self::Nocturne];
    pub const TERMINAL: [Self; 5] = [
        Self::TurboClassic,
        Self::Original,
        Self::Terra,
        Self::Clarity,
        Self::Nocturne,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Terra => "terra",
            Self::Clarity => "clarity",
            Self::Nocturne => "nocturne",
            Self::TurboClassic => "turbo_classic",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Terra => "Terra",
            Self::Clarity => "Clarity",
            Self::Nocturne => "Nocturne",
            Self::TurboClassic => "Turbo Classic",
        }
    }
    pub fn parse(key: &str) -> Option<Self> {
        Self::TERMINAL.into_iter().find(|t| t.key() == key)
    }
}
#[derive(Deserialize)]
pub struct DesktopTheme {
    pub colors: BTreeMap<String, String>,
    pub typography: Typography,
    pub shape: Shape,
    pub overview_layout: String,
}
#[derive(Deserialize)]
pub struct Typography {
    pub title_size: f32,
}
#[derive(Deserialize)]
pub struct Shape {
    pub card_radius: u8,
}
#[derive(Deserialize)]
pub struct TerminalTheme {
    pub colors: BTreeMap<String, String>,
    pub ansi16_reference: String,
}
#[derive(Deserialize)]
struct Catalog<T> {
    skins: BTreeMap<String, T>,
}
#[derive(Deserialize)]
struct TerminalCatalog {
    skins: BTreeMap<String, TerminalTheme>,
    ansi16_palettes: BTreeMap<String, BTreeMap<String, String>>,
}
pub fn desktop(id: ThemeId) -> &'static DesktopTheme {
    static CATALOG: OnceLock<Catalog<DesktopTheme>> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../design/themes.json"))
            .expect("embedded desktop themes")
    });
    &catalog.skins[if id == ThemeId::TurboClassic {
        "original"
    } else {
        id.key()
    }]
}
pub fn terminal(id: ThemeId, ansi16: bool) -> &'static BTreeMap<String, String> {
    static CATALOG: OnceLock<TerminalCatalog> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../design/tui-themes.json"))
            .expect("embedded terminal themes")
    });
    let theme = &catalog.skins[id.key()];
    if ansi16 {
        &catalog.ansi16_palettes[&theme.ansi16_reference]
    } else {
        &theme.colors
    }
}
pub fn rgb(value: &str) -> [u8; 3] {
    let value = value.trim_start_matches('#');
    [0, 2, 4]
        .map(|offset| u8::from_str_radix(&value[offset..offset + 2], 16).expect("embedded RGB"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_embedded_palettes_have_required_roles() {
        for id in ThemeId::DESKTOP {
            for role in ["bg", "surface", "side", "ink", "accent", "border"] {
                rgb(&desktop(id).colors[role]);
            }
        }
        for id in ThemeId::TERMINAL {
            for role in ["background", "foreground", "border", "accent", "error"] {
                rgb(&terminal(id, false)[role]);
                assert!(terminal(id, true).contains_key(role));
            }
        }
        assert_eq!(desktop(ThemeId::Nocturne).overview_layout, "hero_metric");
    }
}
