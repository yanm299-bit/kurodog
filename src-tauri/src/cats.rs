use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatSpot {
    /// "head" | "body" | "leftArm" | "rightArm"
    pub part: String,
    /// Normalized offset (-1..1 for head/body; 0..1 along arm from shoulder to paw)
    pub x: f64,
    /// Normalized vertical offset (-1..1 for head/body; lateral px for arm)
    pub y: f64,
    pub rx: f64,
    pub ry: f64,
    #[serde(default)]
    pub rotation: f64,
    pub color: String,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
}

fn default_opacity() -> f64 {
    0.85
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatSkinConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub emoji: Option<String>,
    #[serde(default)]
    pub builtin: bool,

    // Core fur & outline
    pub fur_color: String,
    #[serde(default)]
    pub fur_secondary_color: Option<String>,
    #[serde(default)]
    pub outline_color: Option<String>,
    #[serde(default)]
    pub leg_line_color: Option<String>,

    // Pattern preset: "solid" | "bicolor" | "tuxedo" | "tabby" | "calico" | "siamese"
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub mask_color: Option<String>,
    #[serde(default)]
    pub forehead_bridge_color: Option<String>,
    #[serde(default)]
    pub flank_color: Option<String>,
    #[serde(default)]
    pub belly_bib_color: Option<String>,
    #[serde(default)]
    pub stripe_color: Option<String>,
    #[serde(default)]
    pub calico_patch_color: Option<String>,

    // Ears
    #[serde(default)]
    pub ear_outer_color: Option<String>,
    #[serde(default)]
    pub ear_tip_color: Option<String>,
    #[serde(default)]
    pub ear_inner_color: Option<String>,
    #[serde(default)]
    pub ear_tuft_color: Option<String>,

    // Tail
    #[serde(default)]
    pub tail_color: Option<String>,
    #[serde(default)]
    pub tail_tip_color: Option<String>,
    #[serde(default)]
    pub tail_fluffy: Option<bool>,
    #[serde(default)]
    pub tail_stripes: Option<bool>,

    // Eyes & face
    #[serde(default = "default_eye_top")]
    pub eye_color_top: String,
    #[serde(default)]
    pub eye_color_bottom: Option<String>,
    #[serde(default)]
    pub pupil_color: Option<String>,
    #[serde(default)]
    pub eye_catchlight: Option<bool>,
    #[serde(default)]
    pub eye_rim_color: Option<String>,
    #[serde(default)]
    pub squint_color: Option<String>,
    #[serde(default)]
    pub squint_glow_color: Option<String>,
    #[serde(default)]
    pub nose_color: Option<String>,
    #[serde(default)]
    pub mouth_color: Option<String>,
    #[serde(default)]
    pub blush_color: Option<String>,
    #[serde(default)]
    pub whisker_color: Option<String>,

    // Arms & paws
    #[serde(default)]
    pub arm_color: Option<String>,
    #[serde(default)]
    pub paw_color: Option<String>,
    #[serde(default)]
    pub paw_outline_color: Option<String>,
    #[serde(default)]
    pub arm_crease_color: Option<String>,

    // Keyboard & mouse accent theme
    #[serde(default)]
    pub accent_color: Option<String>,
    #[serde(default)]
    pub accent_side_color: Option<String>,

    // Unique pet birthmarks / patches
    #[serde(default)]
    pub spots: Vec<CatSpot>,
}

fn default_eye_top() -> String {
    "#d4f044".to_string()
}

pub fn cats_path() -> PathBuf {
    crate::config::app_data_dir().join("cats.json")
}

pub fn sanitize_cat_id(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "custom-cat".to_string()
    } else {
        trimmed.chars().take(36).collect()
    }
}

pub fn builtin_cats() -> Vec<CatSkinConfig> {
    vec![
        CatSkinConfig {
            id: "black".to_string(),
            name: "小黑狗".to_string(),
            emoji: Some("🐕".to_string()),
            builtin: true,
            fur_color: "#141314".to_string(),
            fur_secondary_color: None,
            outline_color: None,
            leg_line_color: Some("rgba(210, 212, 216, 0.64)".to_string()),
            pattern: Some("solid".to_string()),
            mask_color: None,
            forehead_bridge_color: None,
            flank_color: None,
            belly_bib_color: None,
            stripe_color: None,
            calico_patch_color: None,
            ear_outer_color: Some("#141314".to_string()),
            ear_tip_color: None,
            ear_inner_color: None,
            ear_tuft_color: None,
            tail_color: Some("#141314".to_string()),
            tail_tip_color: None,
            tail_fluffy: Some(false),
            tail_stripes: Some(false),
            eye_color_top: "#d4f044".to_string(),
            eye_color_bottom: Some("#d4f044".to_string()),
            pupil_color: Some("#0e0e10".to_string()),
            eye_catchlight: Some(false),
            eye_rim_color: None,
            squint_color: Some("#d4f044".to_string()),
            squint_glow_color: None,
            nose_color: None,
            mouth_color: None,
            blush_color: None,
            whisker_color: Some("rgba(205, 208, 214, 0.54)".to_string()),
            arm_color: Some("#141314".to_string()),
            paw_color: Some("#141314".to_string()),
            paw_outline_color: None,
            arm_crease_color: Some("rgba(216, 218, 222, 0.64)".to_string()),
            accent_color: Some("#d2f244".to_string()),
            accent_side_color: Some("#8cb014".to_string()),
            spots: vec![],
        },
    ]
}

pub fn load_custom_cats() -> Vec<CatSkinConfig> {
    let path = cats_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(mut parsed) = serde_json::from_str::<Vec<CatSkinConfig>>(&content) {
            for c in &mut parsed {
                c.id = sanitize_cat_id(&c.id);
                c.builtin = false;
            }
            return parsed;
        }
    }
    vec![]
}

pub fn save_custom_cats(cats: &[CatSkinConfig]) -> Result<(), String> {
    let path = cats_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(cats).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

pub fn all_cats() -> Vec<CatSkinConfig> {
    builtin_cats()
}

pub fn upsert_custom_cat(mut cat: CatSkinConfig) -> Result<CatSkinConfig, String> {
    cat.id = sanitize_cat_id(&cat.id);
    if cat.name.trim().is_empty() {
        cat.name = cat.id.clone();
    }
    cat.builtin = false;

    let mut custom = load_custom_cats();
    if let Some(existing) = custom.iter_mut().find(|c| c.id == cat.id) {
        *existing = cat.clone();
    } else {
        custom.push(cat.clone());
    }
    save_custom_cats(&custom)?;
    Ok(cat)
}

pub fn delete_custom_cat(id: &str) -> Result<bool, String> {
    let clean_id = sanitize_cat_id(id);
    let mut custom = load_custom_cats();
    let before = custom.len();
    custom.retain(|c| c.id != clean_id);
    if custom.len() == before {
        return Ok(false);
    }
    save_custom_cats(&custom)?;
    Ok(true)
}

pub fn cat_exists(id: &str) -> bool {
    let clean = sanitize_cat_id(id);
    all_cats().iter().any(|c| c.id == clean)
}

pub fn cat_schema_guide() -> serde_json::Value {
    serde_json::json!({
        "description": "KuroDog Legacy Skin Schema. Pass any custom cat JSON to upsert_cat (with activate: true) to immediately add and render it live on the user's desktop!",
        "patterns": {
            "solid": "Single coat color (e.g., British Shorthair blue, white Persian, golden chinchilla, black cat)",
            "bicolor": "Ragdoll / bicolor inverted-V face mask (uses maskColor, foreheadBridgeColor, flankColor)",
            "tuxedo": "Black/dark hood & back with clean white chin/chest bib and white mittens (uses furColor + bellyBibColor + pawColor)",
            "tabby": "Forehead M stripes, cheek stripes, back/flank stripes, and ringed tail (uses furColor + stripeColor + bellyBibColor + tailStripes: true)",
            "calico": "Creamy white base with asymmetric warm orange & dark espresso patches (uses furColor + maskColor + calicoPatchColor)",
            "siamese": "Creamy body with dark seal/chocolate face mask, dark ears, dark paws, dark tail, sapphire eyes"
        },
        "spotsGuide": "Optional array of unique pet markings (spots) on 'head', 'body', 'leftArm', or 'rightArm'. For head/body: x and y are normalized (-1..1). For leftArm/rightArm: x is 0..1 from shoulder to paw, y is lateral offset in px.",
        "examples": {
            "orangeTabby": {
                "id": "orange-tabby",
                "name": "大橘猫",
                "emoji": "🍊",
                "pattern": "tabby",
                "furColor": "#f2a452",
                "furSecondaryColor": "#e38c38",
                "outlineColor": "#6b4322",
                "bellyBibColor": "#fdf6e8",
                "stripeColor": "#b85d19",
                "earOuterColor": "#e38c38",
                "earInnerColor": "#f3b9b5",
                "tailColor": "#eb9640",
                "tailTipColor": "#b85d19",
                "tailStripes": true,
                "eyeColorTop": "#b86b14",
                "eyeColorBottom": "#ffd369",
                "eyeCatchlight": true,
                "noseColor": "#ee8d98",
                "blushColor": "#f596a2",
                "armColor": "#f2a452",
                "pawColor": "#fdf6e8",
                "accentColor": "#ffb347",
                "accentSideColor": "#cc7a16"
            },
            "calico": {
                "id": "calico",
                "name": "三花猫",
                "emoji": "🌸",
                "pattern": "calico",
                "furColor": "#fcfaf5",
                "outlineColor": "#4f4640",
                "maskColor": "#ea9446",
                "calicoPatchColor": "#3c3634",
                "earOuterColor": "#3c3634",
                "earInnerColor": "#f2bfc4",
                "tailColor": "#3c3634",
                "tailTipColor": "#ea9446",
                "eyeColorTop": "#5a8f3d",
                "eyeColorBottom": "#c9e87c",
                "eyeCatchlight": true,
                "noseColor": "#ef95a2",
                "blushColor": "#f498a8",
                "pawColor": "#fcfaf5",
                "accentColor": "#ffa6c1",
                "accentSideColor": "#cc6c8b"
            },
            "tuxedo": {
                "id": "tuxedo",
                "name": "奶牛猫",
                "emoji": "🐄",
                "pattern": "tuxedo",
                "furColor": "#1d1f24",
                "outlineColor": "#141519",
                "bellyBibColor": "#faf9f5",
                "earInnerColor": "#dfb6bc",
                "tailColor": "#1d1f24",
                "tailTipColor": "#faf9f5",
                "eyeColorTop": "#6db840",
                "eyeColorBottom": "#d8f568",
                "eyeCatchlight": true,
                "noseColor": "#ee9aa6",
                "armColor": "#1d1f24",
                "pawColor": "#faf9f5",
                "pawOutlineColor": "#2c2f38",
                "accentColor": "#7ee8a6",
                "accentSideColor": "#3aa666"
            },
            "siamese": {
                "id": "siamese",
                "name": "暹罗猫",
                "emoji": "🤎",
                "pattern": "siamese",
                "furColor": "#f3eadb",
                "furSecondaryColor": "#e2d4be",
                "outlineColor": "#4c3b30",
                "maskColor": "#4a382c",
                "earOuterColor": "#3e2e24",
                "earTipColor": "#2c2018",
                "tailColor": "#4a382c",
                "tailTipColor": "#2c2018",
                "eyeColorTop": "#245ab8",
                "eyeColorBottom": "#8ad4ff",
                "eyeCatchlight": true,
                "noseColor": "#3c2b22",
                "armColor": "#5a4638",
                "pawColor": "#3c2c22",
                "accentColor": "#72c4ff",
                "accentSideColor": "#3a84c8"
            }
        }
    })
}
