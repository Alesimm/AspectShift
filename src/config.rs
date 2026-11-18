use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::presets::{PresetCategory, ResolutionPreset};

/// Entrada de resolución personalizada guardada por el usuario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPresetEntry {
    pub width: u32,
    pub height: u32,
    pub label: String,
}

/// Configuración persistente y portable almacenada en `aspectshift_config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub favorites: Vec<(u32, u32)>,
    pub custom_presets: Vec<CustomPresetEntry>,
    pub last_stretch_res: (u32, u32),
    pub force_gpu_stretch_flag: bool,
    pub restore_native_on_exit: bool,
    pub always_on_top: bool,
    pub confirm_resolution_change: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            favorites: vec![
                (1440, 1080),
                (1280, 960),
                (1280, 1024),
                (1350, 1080),
                (1280, 1080),
                (1152, 864),
                (1024, 768),
                (1680, 1050),
                (1920, 1080),
            ],
            custom_presets: Vec::new(),
            last_stretch_res: (1440, 1080),
            force_gpu_stretch_flag: true,
            restore_native_on_exit: false,
            always_on_top: false,
            confirm_resolution_change: false,
        }
    }
}

impl AppConfig {
    /// Resuelve la ruta del archivo JSON junto al ejecutable o en `%LOCALAPPDATA%`.
    pub fn config_path() -> PathBuf {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                let candidate = dir.join("aspectshift_config.json");
                if candidate.exists() {
                    return candidate;
                }
                let probe = dir.join(".aspectshift_write_probe");
                if fs::write(&probe, b"ok").is_ok() {
                    let _ = fs::remove_file(&probe);
                    return candidate;
                }
            }
        }

        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let dir = PathBuf::from(local_app_data).join("AspectShift");
            let _ = fs::create_dir_all(&dir);
            return dir.join("aspectshift_config.json");
        }

        PathBuf::from("aspectshift_config.json")
    }

    /// Carga la configuración desde disco o devuelve los valores predeterminados.
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&raw) {
                return cfg;
            }
        }
        Self::default()
    }

    /// Guarda el estado actual de forma legible en disco.
    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    pub fn is_favorite(&self, width: u32, height: u32) -> bool {
        self.favorites.contains(&(width, height))
    }

    pub fn toggle_favorite(&mut self, width: u32, height: u32) -> bool {
        if let Some(pos) = self.favorites.iter().position(|&x| x == (width, height)) {
            self.favorites.remove(pos);
            self.save();
            false
        } else {
            self.favorites.push((width, height));
            self.save();
            true
        }
    }

    pub fn add_custom_preset(&mut self, width: u32, height: u32, label: Option<String>) {
        let tag = label
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Personalizada".to_string());

        if let Some(existing) = self
            .custom_presets
            .iter_mut()
            .find(|p| p.width == width && p.height == height)
        {
            existing.label = tag;
        } else {
            self.custom_presets.push(CustomPresetEntry {
                width,
                height,
                label: tag,
            });
        }

        if !self.is_favorite(width, height) {
            self.favorites.push((width, height));
        }
        self.save();
    }

    pub fn remove_custom_preset(&mut self, width: u32, height: u32) {
        self.custom_presets
            .retain(|p| !(p.width == width && p.height == height));
        self.favorites.retain(|&f| f != (width, height));
        self.save();
    }

    pub fn custom_as_presets(&self) -> Vec<ResolutionPreset> {
        self.custom_presets
            .iter()
            .map(|c| ResolutionPreset {
                width: c.width,
                height: c.height,
                tag: c.label.clone(),
                category: PresetCategory::Custom,
                is_custom: true,
            })
            .collect()
    }
}
