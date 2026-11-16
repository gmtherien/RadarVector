//! User settings, saved as versioned TOML in the OS config directory.

use std::path::PathBuf;

use radarvector_core::Filter;
use radarvector_geo::{Camera, Projection};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

pub type Rgba = [u8; 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    System,
    English,
    French,
}

/// Map colours. Each theme has its own palette; values are sRGB + alpha.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Colors {
    pub background: Rgba,
    pub land: Rgba,
    pub coastline: Rgba,
    pub borders: Rgba,
    pub fir_inactive: Rgba,
    pub fir_active: Rgba,
    pub fir_active_fill: Rgba,
    pub uir_active: Rgba,
    pub uir_active_fill: Rgba,
    pub idl: Rgba,
    pub aircraft: Rgba,
    pub aircraft_selected: Rgba,
    pub aircraft_label: Rgba,
    pub airport_delivery: Rgba,
    pub airport_ground: Rgba,
    pub airport_tower: Rgba,
    pub airport_approach: Rgba,
    pub airport_atis: Rgba,
    pub airport_label: Rgba,
    pub fir_label: Rgba,
    pub route: Rgba,
    pub measure: Rgba,
    pub tooltip_background: Rgba,
    pub tooltip_text: Rgba,
}

impl Colors {
    /// Original palette (Okabe-Ito based, colour-blind safe accents).
    pub fn dark() -> Self {
        Self {
            background: [14, 22, 33, 255],
            land: [28, 38, 50, 255],
            coastline: [70, 86, 104, 255],
            borders: [44, 54, 66, 255],
            fir_inactive: [78, 100, 128, 255],
            fir_active: [86, 180, 233, 255],
            fir_active_fill: [86, 180, 233, 40],
            uir_active: [204, 121, 167, 255],
            uir_active_fill: [204, 121, 167, 28],
            idl: [120, 120, 120, 160],
            aircraft: [240, 228, 66, 255],
            aircraft_selected: [255, 255, 255, 255],
            aircraft_label: [220, 220, 200, 255],
            airport_delivery: [0, 158, 115, 255],
            airport_ground: [86, 180, 233, 255],
            airport_tower: [230, 159, 0, 255],
            airport_approach: [213, 94, 0, 255],
            airport_atis: [180, 180, 180, 255],
            airport_label: [230, 230, 230, 255],
            fir_label: [150, 200, 235, 255],
            route: [0, 158, 115, 220],
            measure: [255, 255, 255, 255],
            tooltip_background: [24, 30, 40, 240],
            tooltip_text: [235, 235, 235, 255],
        }
    }

    pub fn light() -> Self {
        Self {
            background: [196, 214, 230, 255],
            land: [242, 240, 234, 255],
            coastline: [140, 150, 160, 255],
            borders: [205, 205, 205, 255],
            fir_inactive: [130, 150, 175, 255],
            fir_active: [0, 114, 178, 255],
            fir_active_fill: [0, 114, 178, 36],
            uir_active: [170, 60, 130, 255],
            uir_active_fill: [170, 60, 130, 26],
            idl: [120, 120, 120, 160],
            aircraft: [200, 60, 0, 255],
            aircraft_selected: [0, 0, 0, 255],
            aircraft_label: [40, 40, 40, 255],
            airport_delivery: [0, 130, 95, 255],
            airport_ground: [0, 114, 178, 255],
            airport_tower: [200, 130, 0, 255],
            airport_approach: [190, 70, 0, 255],
            airport_atis: [110, 110, 110, 255],
            airport_label: [30, 30, 30, 255],
            fir_label: [0, 80, 140, 255],
            route: [0, 130, 95, 220],
            measure: [0, 0, 0, 255],
            tooltip_background: [250, 250, 250, 245],
            tooltip_text: [20, 20, 20, 255],
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        Self::dark()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    pub camera: Camera,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Favorite {
    Country(String),
    Fir(String),
    Airport(String),
}

impl Favorite {
    pub fn code(&self) -> &str {
        match self {
            Self::Country(c) | Self::Fir(c) | Self::Airport(c) => c,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub theme: Theme,
    pub language: Language,
    pub dark_colors: Colors,
    pub light_colors: Colors,
    pub font_scale: f32,
    pub auto_refresh: bool,
    pub refresh_interval_s: u64,
    pub invert_mouse_buttons: bool,
    pub projection: Projection,
    pub approach_radius_nm: f64,
    pub hide_unstaffed: bool,
    pub show_fir_labels: bool,
    pub show_airport_labels: bool,
    pub show_aircraft_labels: bool,
    pub label_altitude: bool,
    pub label_speed: bool,
    pub show_aircraft: bool,
    pub show_ground_traffic: bool,
    pub show_idl: bool,
    pub show_tracon_polygons: bool,
    pub aircraft_size: f32,
    pub interpolate_positions: bool,
    pub notifications: bool,
    pub auto_check_updates: bool,
    pub alert_followed_cids: bool,
    pub alert_favorite_atc: bool,
    pub followed_cids: Vec<i64>,
    pub data_dir_override: Option<PathBuf>,
    pub last_camera: Option<Camera>,
    /// Ctrl+Alt+digit bookmarks, keyed by digit (`"0"`..`"9"`).
    pub hotkey_bookmarks: std::collections::BTreeMap<String, Bookmark>,
    pub bookmarks: Vec<Bookmark>,
    pub favorites: Vec<Favorite>,
    pub filters: Vec<Filter>,
    pub list_columns_controllers: Vec<String>,
    pub list_columns_flights: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            theme: Theme::Dark,
            language: Language::System,
            dark_colors: Colors::dark(),
            light_colors: Colors::light(),
            font_scale: 1.0,
            auto_refresh: true,
            refresh_interval_s: 60,
            invert_mouse_buttons: false,
            projection: Projection::Equirectangular,
            approach_radius_nm: 30.0,
            hide_unstaffed: false,
            show_fir_labels: true,
            show_airport_labels: true,
            show_aircraft_labels: true,
            label_altitude: false,
            label_speed: false,
            show_aircraft: true,
            show_ground_traffic: true,
            show_idl: true,
            show_tracon_polygons: true,
            aircraft_size: 7.0,
            interpolate_positions: false,
            notifications: true,
            auto_check_updates: true,
            alert_followed_cids: true,
            alert_favorite_atc: true,
            followed_cids: Vec::new(),
            data_dir_override: None,
            last_camera: None,
            hotkey_bookmarks: Default::default(),
            bookmarks: Vec::new(),
            favorites: Vec::new(),
            filters: Vec::new(),
            list_columns_controllers: Vec::new(),
            list_columns_flights: Vec::new(),
        }
    }
}

impl Settings {
    pub fn colors(&self) -> &Colors {
        match self.theme {
            Theme::Dark => &self.dark_colors,
            Theme::Light => &self.light_colors,
        }
    }

    pub fn colors_mut(&mut self) -> &mut Colors {
        match self.theme {
            Theme::Dark => &mut self.dark_colors,
            Theme::Light => &mut self.light_colors,
        }
    }

    /// Reset display preferences, keeping user content (filters, favourites, bookmarks, followed CIDs).
    pub fn restore_defaults(&mut self) {
        let keep = (
            std::mem::take(&mut self.filters),
            std::mem::take(&mut self.favorites),
            std::mem::take(&mut self.bookmarks),
            std::mem::take(&mut self.hotkey_bookmarks),
            std::mem::take(&mut self.followed_cids),
            self.last_camera,
        );
        *self = Settings::default();
        (
            self.filters,
            self.favorites,
            self.bookmarks,
            self.hotkey_bookmarks,
            self.followed_cids,
            self.last_camera,
        ) = keep;
    }

    /// Apply migrations from older schema versions.
    pub fn migrate(mut self) -> Self {
        if self.schema_version < SCHEMA_VERSION {
            self.schema_version = SCHEMA_VERSION;
        }
        self.hotkey_bookmarks
            .retain(|k, _| k.len() == 1 && k.chars().all(|c| c.is_ascii_digit()));
        self.refresh_interval_s = self.refresh_interval_s.max(15);
        self
    }

    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        Ok(toml::from_str::<Settings>(text)?.migrate())
    }

    pub fn to_toml(&self) -> String {
        self.try_to_toml().unwrap_or_default()
    }

    pub fn try_to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    pub fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "RadarVector").map(|d| d.config_dir().join("settings.toml"))
    }

    pub fn load() -> Self {
        let Some(p) = Self::path() else { return Self::default() };
        match std::fs::read_to_string(&p) {
            Ok(t) => Self::from_toml(&t).unwrap_or_else(|e| {
                tracing::warn!("settings unreadable ({e}), using defaults");
                let _ = std::fs::rename(&p, p.with_extension("toml.bad"));
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(p) = Self::path() else { return Ok(()) };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = p.with_extension("toml.tmp");
        let text = self.try_to_toml().map_err(std::io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip() {
        let mut s = Settings::default();
        s.filters.push(Filter {
            name: "Amis".into(),
            cids: vec![1],
            ..Default::default()
        });
        s.favorites.push(Favorite::Airport("CYUL".into()));
        s.hotkey_bookmarks.insert(
            "3".into(),
            Bookmark {
                name: "Montréal".into(),
                camera: Camera::default(),
            },
        );
        let t = s.to_toml();
        let back = Settings::from_toml(&t).unwrap();
        assert_eq!(back.filters, s.filters);
        assert_eq!(back.favorites, s.favorites);
        assert_eq!(
            back.hotkey_bookmarks.get("3").map(|b| b.name.as_str()),
            Some("Montréal")
        );
    }

    #[test]
    fn migrates_old_and_partial_files() {
        let s = Settings::from_toml("refresh_interval_s = 5\ntheme = \"Light\"\n").unwrap();
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert_eq!(s.refresh_interval_s, 15);
        assert_eq!(s.theme, Theme::Light);
        assert!(s.hotkey_bookmarks.is_empty());
        assert!(s.auto_refresh);
    }

    #[test]
    fn restore_keeps_user_content() {
        let mut s = Settings {
            font_scale: 2.0,
            ..Default::default()
        };
        s.favorites.push(Favorite::Fir("CZUL".into()));
        s.restore_defaults();
        assert_eq!(s.font_scale, 1.0);
        assert_eq!(s.favorites.len(), 1);
    }
}
