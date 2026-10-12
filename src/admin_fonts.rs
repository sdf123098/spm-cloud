//! Embedded original HarmonyOS Sans font assets. See web/fonts/manifest.json.
pub fn font(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "HarmonyOS_Sans_Black.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_Black.ttf"),
        "HarmonyOS_Sans_Bold.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_Bold.ttf"),
        "HarmonyOS_Sans_Light.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_Light.ttf"),
        "HarmonyOS_Sans_Medium.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_Medium.ttf"),
        "HarmonyOS_Sans_Regular.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_Regular.ttf"),
        "HarmonyOS_Sans_Thin.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_Thin.ttf"),
        "HarmonyOS_Sans_Condensed_Black.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Black.ttf")
        }
        "HarmonyOS_Sans_Condensed_Bold.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Bold.ttf")
        }
        "HarmonyOS_Sans_Condensed_Light.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Light.ttf")
        }
        "HarmonyOS_Sans_Condensed_Medium.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Medium.ttf")
        }
        "HarmonyOS_Sans_Condensed_Regular.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Regular.ttf")
        }
        "HarmonyOS_Sans_Condensed_Thin.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Thin.ttf")
        }
        "HarmonyOS_Sans_Condensed_Black_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Black_Italic.ttf")
        }
        "HarmonyOS_Sans_Condensed_Bold_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Bold_Italic.ttf")
        }
        "HarmonyOS_Sans_Condensed_Light_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Light_Italic.ttf")
        }
        "HarmonyOS_Sans_Condensed_Medium_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Medium_Italic.ttf")
        }
        "HarmonyOS_Sans_Condensed_Regular_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Regular_Italic.ttf")
        }
        "HarmonyOS_Sans_Condensed_Thin_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Condensed_Thin_Italic.ttf")
        }
        "HarmonyOS_Sans_Black_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Black_Italic.ttf")
        }
        "HarmonyOS_Sans_Bold_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Bold_Italic.ttf")
        }
        "HarmonyOS_Sans_Light_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Light_Italic.ttf")
        }
        "HarmonyOS_Sans_Medium_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Medium_Italic.ttf")
        }
        "HarmonyOS_Sans_Regular_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Regular_Italic.ttf")
        }
        "HarmonyOS_Sans_Thin_Italic.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Thin_Italic.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_Black.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_Black.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_Bold.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_Bold.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_Light.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_Light.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_Medium.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_Medium.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_Regular.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_Regular.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_Thin.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_Thin.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_UI_Black.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_UI_Black.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_UI_Bold.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_UI_Bold.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_UI_Light.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_UI_Light.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_UI_Medium.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_UI_Medium.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_UI_Regular.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_UI_Regular.ttf")
        }
        "HarmonyOS_Sans_Naskh_Arabic_UI_Thin.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_Naskh_Arabic_UI_Thin.ttf")
        }
        "HarmonyOS_Sans_SC_Black.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_SC_Black.ttf"),
        "HarmonyOS_Sans_SC_Bold.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_SC_Bold.ttf"),
        "HarmonyOS_Sans_SC_Light.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_SC_Light.ttf"),
        "HarmonyOS_Sans_SC_Medium.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_SC_Medium.ttf")
        }
        "HarmonyOS_Sans_SC_Regular.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_SC_Regular.ttf")
        }
        "HarmonyOS_Sans_SC_Thin.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_SC_Thin.ttf"),
        "HarmonyOS_Sans_TC_Black.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_TC_Black.ttf"),
        "HarmonyOS_Sans_TC_Bold.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_TC_Bold.ttf"),
        "HarmonyOS_Sans_TC_Light.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_TC_Light.ttf"),
        "HarmonyOS_Sans_TC_Medium.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_TC_Medium.ttf")
        }
        "HarmonyOS_Sans_TC_Regular.ttf" => {
            include_bytes!("../web/fonts/HarmonyOS_Sans_TC_Regular.ttf")
        }
        "HarmonyOS_Sans_TC_Thin.ttf" => include_bytes!("../web/fonts/HarmonyOS_Sans_TC_Thin.ttf"),
        _ => return None,
    })
}
