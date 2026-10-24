//! AI command consent for authenticated automation sessions.
use crate::{EngineError, Result, Session};
pub fn sends_images(id: &str) -> bool {
    matches!(id, "edit.generativeFill" | "layer.generative.more" | "image.generateImage" | "edit.generativeExpand" | "layer.aiSeamlessBlend")
}
pub fn authorize(s: &Session, id: &str) -> Result<()> {
    if s.authorize.is_some()
        && sends_images(id)
        && s.prefs().dialogs.get("customAi").and_then(|v| v.get("allowAutomation")).and_then(serde_json::Value::as_bool) != Some(true)
    {
        return Err(EngineError::Other("AI requests from automation are disabled. Enable them explicitly in Generative AI Settings.".into()));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automation_cannot_grant_itself_image_uploads() {
        let mut s = Session::new();
        s.authorize = Some(|_, _| Ok(()));
        for id in ["edit.generativeFill", "layer.generative.more", "image.generateImage", "edit.generativeExpand", "layer.aiSeamlessBlend"] {
            assert!(authorize(&s, id).is_err());
        }
        assert!(authorize(&s, "ai.providers").is_ok());
        s.prefs.edit(|p| p.dialogs.insert("customAi".into(), serde_json::json!({"allowAutomation": true})));
        assert!(authorize(&s, "edit.generativeFill").is_ok());
    }
}
