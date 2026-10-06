//! Opt-in live regression: uses saved provider/credentials, sends synthetic text only.
//! cargo run -p designcraft-ui-egui --example ai_live_check -- --live /tmp/ai-live
use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services};
use serde_json::json;
use std::time::{Duration, Instant};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--live") {
        return Err("Explicit --live required: this sends synthetic text to your saved AI provider.".into());
    }
    let dir = args.get(2).ok_or("Provide an output directory")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let services = Services {
        write: Some(Box::new(|p, b| std::fs::write(p, b).map_err(|e| e.to_string()))),
        read: Some(Box::new(|p| std::fs::read(p).map_err(|e| e.to_string()))),
        ..Default::default()
    };
    let mut app = DesignApp::new(Session::new(), services);
    let ctx = egui::Context::default();
    for (case, padding) in [("short", 0), ("long", 90)] {
        app.run("file.new", json!({"title":format!("AI live regression {case}")}))?;
        let outside = "选区外：今天天汽很好。\n";
        let selection = format!(
            "😀今天天汽很好，我们一起去公园散布。\n{}另一段：今天天汽很好，适合晒太阳。\n",
            "这是合成校对文本，正文应保持原有含义和段落。\n".repeat(padding)
        );
        let text = format!("{outside}{selection}选区外结尾。 ");
        let frame = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":text}))?;
        let sid = frame["story"].as_u64().ok_or("missing story")?;
        let emphasis = text.find("我们").unwrap();
        app.run("text.select", json!({"story":sid,"anchor":emphasis,"focus":emphasis+"我们".len()}))?;
        app.run("type.char", json!({"attrs":{"size":24}}))?;
        let original_story = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().clone();
        let original_frames = serde_json::to_value(&app.session.active().unwrap().doc.spreads).unwrap();
        app.run("text.select", json!({"story":sid,"anchor":outside.len(),"focus":outside.len()+selection.len()}))?;
        app.run("ai.proofreadSelection", json!({}))?;
        let deadline = Instant::now() + Duration::from_secs(240);
        loop {
            app.logic(&ctx);
            let state = app.run("ai.task.inspect", json!({}))?;
            if state["busy"] == false {
                println!("{case}: {}", state["notice"]);
                break;
            }
            if Instant::now() > deadline {
                app.run("ai.task.stop", json!({}))?;
                return Err(format!("{case}: live request timed out"));
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        let review = app.run("ai.suggestion.list", json!({}))?;
        std::fs::write(format!("{dir}/{case}-review.json"), serde_json::to_vec_pretty(&review).unwrap()).map_err(|e| e.to_string())?;
        if review["taskStatus"] != "completed" {
            return Err(format!("{case}: task failed; see review artifact"));
        }
        let n = review["suggestions"].as_array().ok_or("missing suggestions")?.len();
        if n == 0 {
            return Err(format!("{case}: expected corrections, got {n}"));
        }
        let before = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
        if before != text {
            return Err("AI changed text before acceptance".into());
        }
        app.run("ai.suggestion.acceptAll", json!({}))?;
        let after = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
        // Verify application of the actual proposals, not model-dependent proofreading recall.
        let mut expected = text.clone();
        let mut suggestions = review["suggestions"].as_array().unwrap().iter().collect::<Vec<_>>();
        suggestions.sort_by_key(|x| x["start"].as_u64().unwrap());
        for x in suggestions.into_iter().rev() {
            let start = x["start"].as_u64().unwrap() as usize;
            let end = x["end"].as_u64().unwrap() as usize;
            if start < outside.len() || end > outside.len() + selection.len() || expected.get(start..end) != x["original"].as_str() {
                return Err("Suggestion did not match the selected source".into());
            }
            expected.replace_range(start..end, x["replacement"].as_str().unwrap());
        }
        if after != expected || after == text {
            return Err(format!("{case}: accepted text differs from the exact proposals"));
        }
        let edited = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap();
        if edited.chars != original_story.chars
            || edited.paras != original_story.paras
            || serde_json::to_value(&app.session.active().unwrap().doc.spreads).unwrap() != original_frames
        {
            return Err(format!("{case}: existing text formatting or frames changed"));
        }
        app.run("edit.undo", json!({}))?;
        if app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text != text {
            return Err("Undo did not restore original".into());
        }
        let path = format!("{dir}/{case}.designcraft");
        app.run("file.saveAs", json!({"path":path}))?;
        app.run("file.open", json!({"path":path}))?;
        if app.run("ai.suggestion.list", json!({}))?["suggestions"] != review["suggestions"] {
            return Err("Save/reopen changed suggestions".into());
        }
        println!("{case}: {n} suggestions; accepted with existing formatting intact, undone, saved and reopened successfully");
    }
    Ok(())
}
