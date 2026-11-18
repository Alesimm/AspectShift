use serde::{Deserialize, Serialize};

/// Categorías disponibles en el filtro superior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresetCategory {
    Featured,
    FourThree,
    FiveFour,
    SixteenTen,
    SixteenNine,
    Custom,
}

impl PresetCategory {
    pub const ALL: [PresetCategory; 6] = [
        PresetCategory::Featured,
        PresetCategory::FourThree,
        PresetCategory::FiveFour,
        PresetCategory::SixteenTen,
        PresetCategory::SixteenNine,
        PresetCategory::Custom,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            PresetCategory::Featured => "Destacadas",
            PresetCategory::FourThree => "4:3",
            PresetCategory::FiveFour => "5:4",
            PresetCategory::SixteenTen => "16:10",
            PresetCategory::SixteenNine => "16:9",
            PresetCategory::Custom => "Personalizadas",
        }
    }
}

/// Representa un ajuste predefinido o personalizado de resolución.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionPreset {
    pub width: u32,
    pub height: u32,
    pub tag: String,
    pub category: PresetCategory,
    pub is_custom: bool,
}

impl ResolutionPreset {
    pub fn aspect_label(&self) -> String {
        format_aspect_ratio(self.width, self.height)
    }

    /// Calcula el ancho relativo de la barra proporcional respecto a una pantalla 16:9.
    pub fn visual_bar_ratio(&self) -> f32 {
        let ratio = self.width as f32 / self.height.max(1) as f32;
        (ratio / (16.0 / 9.0)).clamp(0.42, 1.0)
    }
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

/// Formatea la relación de aspecto simplificada de un par ancho × alto.
pub fn format_aspect_ratio(width: u32, height: u32) -> String {
    if height == 0 {
        return "—".to_string();
    }
    let ratio = width as f32 / height as f32;
    if (ratio - 4.0 / 3.0).abs() < 0.015 {
        return "4:3".to_string();
    }
    if (ratio - 5.0 / 4.0).abs() < 0.015 {
        return "5:4".to_string();
    }
    if (ratio - 16.0 / 10.0).abs() < 0.025 {
        return "16:10".to_string();
    }
    if (ratio - 16.0 / 9.0).abs() < 0.02 {
        return "16:9".to_string();
    }
    if (ratio - 1.0).abs() < 0.01 {
        return "1:1".to_string();
    }

    let g = gcd(width, height);
    let rw = width / g;
    let rh = height / g;
    if rw <= 32 && rh <= 27 {
        format!("{}:{}", rw, rh)
    } else {
        format!("{:.2}:1", ratio)
    }
}

/// Lista integrada de resoluciones organizadas por relación de aspecto.
pub fn builtin_presets() -> Vec<ResolutionPreset> {
    vec![
        // 4:3
        ResolutionPreset {
            width: 1440,
            height: 1080,
            tag: "4:3 Full HD".to_string(),
            category: PresetCategory::FourThree,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1280,
            height: 960,
            tag: "4:3 Estándar".to_string(),
            category: PresetCategory::FourThree,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1920,
            height: 1440,
            tag: "4:3 QHD".to_string(),
            category: PresetCategory::FourThree,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1600,
            height: 1200,
            tag: "4:3 UXGA".to_string(),
            category: PresetCategory::FourThree,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1152,
            height: 864,
            tag: "4:3 XGA+".to_string(),
            category: PresetCategory::FourThree,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1024,
            height: 768,
            tag: "4:3 XGA".to_string(),
            category: PresetCategory::FourThree,
            is_custom: false,
        },
        ResolutionPreset {
            width: 960,
            height: 720,
            tag: "4:3 Compacto".to_string(),
            category: PresetCategory::FourThree,
            is_custom: false,
        },
        // 5:4 y proporciones estrechas
        ResolutionPreset {
            width: 1280,
            height: 1024,
            tag: "5:4 SXGA".to_string(),
            category: PresetCategory::FiveFour,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1350,
            height: 1080,
            tag: "5:4 Full HD".to_string(),
            category: PresetCategory::FiveFour,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1280,
            height: 1080,
            tag: "32:27 Estirado".to_string(),
            category: PresetCategory::FiveFour,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1560,
            height: 1080,
            tag: "13:9 Híbrido".to_string(),
            category: PresetCategory::FiveFour,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1440,
            height: 1050,
            tag: "SXGA+ Estirado".to_string(),
            category: PresetCategory::FiveFour,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1080,
            height: 1080,
            tag: "1:1 Cuadrado".to_string(),
            category: PresetCategory::FiveFour,
            is_custom: false,
        },
        // 16:10
        ResolutionPreset {
            width: 1728,
            height: 1080,
            tag: "16:10 Full HD".to_string(),
            category: PresetCategory::SixteenTen,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1680,
            height: 1050,
            tag: "16:10 WSXGA+".to_string(),
            category: PresetCategory::SixteenTen,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1600,
            height: 1080,
            tag: "40:27 Intermedio".to_string(),
            category: PresetCategory::SixteenTen,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1920,
            height: 1200,
            tag: "16:10 WUXGA".to_string(),
            category: PresetCategory::SixteenTen,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1440,
            height: 900,
            tag: "16:10 WXGA+".to_string(),
            category: PresetCategory::SixteenTen,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1280,
            height: 800,
            tag: "16:10 WXGA".to_string(),
            category: PresetCategory::SixteenTen,
            is_custom: false,
        },
        // 16:9
        ResolutionPreset {
            width: 1920,
            height: 1080,
            tag: "16:9 Full HD".to_string(),
            category: PresetCategory::SixteenNine,
            is_custom: false,
        },
        ResolutionPreset {
            width: 2560,
            height: 1440,
            tag: "16:9 QHD".to_string(),
            category: PresetCategory::SixteenNine,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1600,
            height: 900,
            tag: "16:9 HD+".to_string(),
            category: PresetCategory::SixteenNine,
            is_custom: false,
        },
        ResolutionPreset {
            width: 1280,
            height: 720,
            tag: "16:9 HD".to_string(),
            category: PresetCategory::SixteenNine,
            is_custom: false,
        },
    ]
}
