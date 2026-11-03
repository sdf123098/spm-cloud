//! Embedded HarmonyOS Sans SC subsets (WOFF2, 1812 glyphs).
//! Full TTF originals live in web/fonts/ but are no longer embedded.
pub fn font(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "HarmonyOS_Sans_SC_Regular.woff2" => {
            include_bytes!("../web/fonts-subset/HarmonyOS_Sans_SC_Regular.woff2")
        }
        "HarmonyOS_Sans_SC_Medium.woff2" => {
            include_bytes!("../web/fonts-subset/HarmonyOS_Sans_SC_Medium.woff2")
        }
        "HarmonyOS_Sans_SC_Bold.woff2" => {
            include_bytes!("../web/fonts-subset/HarmonyOS_Sans_SC_Bold.woff2")
        }
        _ => return None,
    })
}
