//! Native AI assistant window and background-task bridge.
use crate::{DesignApp, theme::Tokens};
use designcraft_doc::ai::Status;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowState {
    pub open: bool,
    pub floating: bool,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub tab: String,
}
impl Default for WindowState {
    fn default() -> Self {
        Self { open: false, floating: false, position: [700.0, 140.0], size: [390.0, 580.0], tab: "chat".into() }
    }
}
#[derive(Default)]
pub struct Assistant {
    pub drafts: std::collections::HashMap<u64, String>,
    pub notice: String,
    #[cfg(not(target_arch = "wasm32"))]
    pub native: runtime::Runtime,
}
pub fn busy(app: &DesignApp) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.ai.native.task.is_some()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        false
    }
}
pub fn poll(app: &mut DesignApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    runtime::poll(app, ctx);
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ctx);
}
pub fn command(app: &mut DesignApp, id: &str, p: &Value) -> Result<Value, String> {
    match id {
        "window.ai" => {
            app.ui.ai.open = p["open"].as_bool().unwrap_or(true);
            Ok(json!(app.ui.ai))
        }
        "window.ai.layout" => {
            if let Some(f) = p["floating"].as_bool() {
                app.ui.ai.floating = f;
            }
            if let Some(t) = p["tab"].as_str() {
                if !matches!(t, "chat" | "suggestions" | "settings") {
                    return Err("invalid tab".into());
                }
                app.ui.ai.tab = t.into();
            }
            if let Some(pos) = p["position"].as_array()
                && pos.len() == 2
            {
                app.ui.ai.position = [pos[0].as_f64().unwrap_or(700.0) as f32, pos[1].as_f64().unwrap_or(140.0) as f32];
            }
            app.ui.ai.open = true;
            Ok(json!(app.ui.ai))
        }
        "ai.suggestion.show" => {
            let r = app.run("ai.suggestion.locate", p.clone())?;
            if let Some(page) = r["page"].as_u64() {
                crate::canvas::go_to_page(app, page as usize);
            } else if let Some(sid) = r["story"].as_u64() {
                app.story_editor = Some(designcraft_doc::StoryId(sid));
            }
            Ok(r)
        }
        _ => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                runtime::command(app, id, p)
            }
            #[cfg(target_arch = "wasm32")]
            {
                Err("AI 网络和登录功能仅适用于桌面版。".into())
            }
        }
    }
}
fn action(app: &mut DesignApp, id: &str, p: Value) {
    if let Err(e) = app.run(id, p) {
        app.ai.notice = e;
    }
}
pub fn docked(app: &mut DesignApp, ui: &mut egui::Ui) {
    if !app.ui.ai.open || app.ui.ai.floating {
        return;
    }
    let t = Tokens::get(ui.ctx());
    egui::Panel::right("ai_assistant_dock")
        .default_size(app.ui.ai.size[0])
        .size_range(320.0..=620.0)
        .resizable(true)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(10))
        .show(ui, |ui| {
            app.ui.ai.size[0] = ui.available_width();
            ui.horizontal(|ui| {
                ui.heading("AI 助手");
                if ui.small_button("浮动").clicked() {
                    action(app, "window.ai.layout", json!({"floating":true}));
                }
                if ui.small_button("关闭").clicked() {
                    action(app, "window.ai", json!({"open":false}));
                }
            });
            body(app, ui);
        });
}
pub fn floating(app: &mut DesignApp, ctx: &egui::Context) {
    if !app.ui.ai.open || !app.ui.ai.floating {
        return;
    }
    let mut open = true;
    let at = app.ui.ai.position;
    let size = app.ui.ai.size;
    let r = egui::Window::new("AI 助手")
        .id(egui::Id::new("ai_assistant_window"))
        .open(&mut open)
        .default_pos(egui::pos2(at[0], at[1]))
        .default_size(egui::vec2(size[0], size[1]))
        .min_width(320.0)
        .show(ctx, |ui| {
            if ui.small_button("停靠右侧").clicked() {
                action(app, "window.ai.layout", json!({"floating":false}));
            }
            body(app, ui);
        });
    if let Some(r) = r {
        app.ui.ai.position = [r.response.rect.min.x, r.response.rect.min.y];
        app.ui.ai.size = [r.response.rect.width(), r.response.rect.height()];
    }
    if !open {
        action(app, "window.ai", json!({"open":false}));
    }
}
fn body(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let running = busy(app);
    ui.horizontal(|ui| {
        for (id, label) in [("chat", "对话"), ("suggestions", "修改建议"), ("settings", "设置")] {
            if ui.selectable_label(app.ui.ai.tab == id, label).clicked() {
                action(app, "window.ai.layout", json!({"tab":id}));
            }
        }
    });
    ui.separator();
    if running {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("处理中…");
            if ui.button("停止").clicked() {
                action(app, "ai.task.stop", json!({}));
            }
        });
    }
    if !app.ai.notice.is_empty() {
        ui.label(egui::RichText::new(&app.ai.notice).color(t.accent));
    }
    if app.ui.ai.tab == "settings" {
        #[cfg(not(target_arch = "wasm32"))]
        runtime::settings(app, ui);
        #[cfg(target_arch = "wasm32")]
        {
            ui.label("AI 服务与登录仅在桌面版可用。");
        }
        return;
    }
    if app.ui.ai.tab == "suggestions" {
        let _ = app.run("ai.suggestion.list", json!({}));
    }
    let Some(st) = app.session.active() else {
        ui.label("打开文档后开始校对。");
        return;
    };
    let uid = st.uid;
    let title = st.title();
    let data = st.doc.ai.clone().unwrap_or_default();
    ui.label(egui::RichText::new(title).strong());
    if app.ui.ai.tab == "suggestions" {
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!running, egui::Button::new("全部接受")).clicked() {
                action(app, "ai.suggestion.acceptAll", json!({}));
            }
            if ui.add_enabled(!running, egui::Button::new("导出 HTML 报告")).clicked() {
                action(app, "ai.report.export", json!({}));
            }
        });
        if data.suggestions.is_empty() {
            ui.label("暂无建议。选择正文并点击“修复标点和错别字”。");
        }
        egui::ScrollArea::vertical().id_salt("ai_suggestions").auto_shrink([false, false]).show(ui, |ui| {
            for s in &data.suggestions {
                ui.push_id(s.id, |ui| {
                    ui.group(|ui| {
                        ui.label(format!("#{} · {} · {}", s.id, s.status.label(), s.page_label.as_deref().unwrap_or("溢出/页码未知")));
                        ui.label(egui::RichText::new(&s.original).strikethrough().color(t.text_dim));
                        ui.label(egui::RichText::new(&s.replacement).color(t.accent));
                        ui.label(&s.reason);
                        if s.status == Status::Pending {
                            ui.horizontal(|ui| {
                                if ui.button("定位原文").clicked() {
                                    action(app, "ai.suggestion.show", json!({"id":s.id}));
                                }
                                if ui.add_enabled(!running, egui::Button::new("接受")).clicked() {
                                    action(app, "ai.suggestion.accept", json!({"id":s.id}));
                                }
                                if ui.add_enabled(!running, egui::Button::new("拒绝")).clicked() {
                                    action(app, "ai.suggestion.reject", json!({"id":s.id}));
                                }
                            });
                        }
                    });
                });
            }
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!running && cfg!(not(target_arch = "wasm32")), egui::Button::new("修复标点和错别字")).clicked() {
                action(app, "ai.proofreadSelection", json!({}));
            }
            if ui.add_enabled(!running, egui::Button::new("新对话")).clicked() {
                action(app, "ai.chat.clear", json!({}));
            }
        });
        let draft = app.ai.drafts.entry(uid).or_default();
        ui.add(egui::TextEdit::multiline(draft).desired_rows(3).desired_width(f32::INFINITY).hint_text("说明需要校对或改写的内容…"));
        let text = draft.clone();
        if ui.add_enabled(!running && !text.trim().is_empty() && cfg!(not(target_arch = "wasm32")), egui::Button::new("发送")).clicked() {
            match app.run("ai.chat.send", json!({"text":text})) {
                Ok(_) => {
                    app.ai.drafts.remove(&uid);
                }
                Err(e) => app.ai.notice = e,
            }
        }
        ui.label(egui::RichText::new("正文只会在接受建议后修改。聊天和建议随文档保存。").small().color(t.text_dim));
        ui.separator();
        egui::ScrollArea::vertical().id_salt(("ai_chat", uid)).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            if data.messages.is_empty() {
                ui.label("选择正文进行校对，或直接提出问题。");
            }
            for m in &data.messages {
                ui.group(|ui| {
                    ui.label(egui::RichText::new(if m.role == "user" { "你" } else { "AI 助手" }).strong());
                    ui.label(&m.text);
                });
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod runtime {
    use super::*;
    use designcraft_ai::native::{self, Cancellation, Settings};
    use std::{
        collections::HashSet,
        sync::mpsc::{self, Receiver, Sender},
        time::Duration,
    };
    pub enum Event {
        Tool { task: u64, name: String, params: Value, reply: Sender<Result<Value, String>> },
        Done { task: u64, result: Result<String, String> },
        Settings { task: u64, result: Result<Settings, String> },
        Models { task: u64, result: Result<Vec<String>, String> },
    }
    pub struct Task {
        id: u64,
        doc: Option<u64>,
        cancel: Cancellation,
        allowed: Option<HashSet<u64>>,
        stories: Vec<(designcraft_doc::StoryId, String)>,
        initial_suggestions: usize,
    }
    pub struct Runtime {
        pub settings: Settings,
        saved: Settings,
        pub key: String,
        pub models: Vec<String>,
        pub task: Option<Task>,
        tx: Sender<Event>,
        rx: Receiver<Event>,
        next: u64,
        seen: HashSet<u64>,
    }
    impl Default for Runtime {
        fn default() -> Self {
            let (tx, rx) = mpsc::channel();
            let settings = Settings::load().unwrap_or_default();
            Self { saved: settings.clone(), settings, key: String::new(), models: vec![], task: None, tx, rx, next: 0, seen: HashSet::new() }
        }
    }
    fn start(app: &mut DesignApp, doc: Option<u64>, allowed: Option<HashSet<u64>>) -> Result<(u64, Cancellation, Sender<Event>), String> {
        if busy(app) {
            return Err("请等待当前任务完成，或先停止。".into());
        }
        app.ai.native.next += 1;
        let id = app.ai.native.next;
        let cancel = Cancellation::default();
        let stories = if doc.is_some() {
            app.session.active().map(|st| st.doc.stories.iter().map(|(id, s)| (*id, s.text.clone())).collect()).unwrap_or_default()
        } else {
            vec![]
        };
        let initial_suggestions = app.session.active().and_then(|st| st.doc.ai.as_ref()).map_or(0, |a| a.suggestions.len());
        app.ai.native.task = Some(Task { id, doc, cancel: cancel.clone(), allowed, stories, initial_suggestions });
        app.ai.notice = "处理中…".into();
        Ok((id, cancel, app.ai.native.tx.clone()))
    }
    pub fn poll(app: &mut DesignApp, ctx: &egui::Context) {
        if let Some(uid) = app.session.active().map(|s| s.uid)
            && app.ai.native.seen.insert(uid)
        {
            if app.session.active().and_then(|s| s.doc.ai.as_ref()).is_some_and(|a| a.task_status == "running") {
                let _ = app.run("ai.task.status", json!({"status":"interrupted"}));
            }
            let _ = app.run("ai.suggestion.list", json!({}));
        }
        let invalid = app.ai.native.task.as_ref().is_some_and(|t| {
            t.doc.is_some()
                && (t.doc != app.session.active().map(|s| s.uid)
                    || app.session.active().is_some_and(|st| t.stories.iter().any(|(id, text)| st.doc.story(*id).is_none_or(|s| s.text != *text))))
        });
        if invalid {
            stop(app, "文档已切换、关闭或正文已变化；任务已停止。");
        }
        while let Ok(event) = app.ai.native.rx.try_recv() {
            let id = match &event {
                Event::Tool { task, .. } | Event::Done { task, .. } | Event::Settings { task, .. } | Event::Models { task, .. } => *task,
            };
            if app.ai.native.task.as_ref().is_none_or(|t| t.id != id) {
                continue;
            }
            match event {
                Event::Tool { name, params, reply, .. } => {
                    let allowed = app.ai.native.task.as_ref().and_then(|t| t.allowed.as_ref());
                    let permitted =
                        allowed.is_none_or(|ids| name == "propose_text_change" && params["snapshot"].as_u64().is_some_and(|id| ids.contains(&id)));
                    let result = if !permitted {
                        Err("选区校对仅允许对捕获的快照提出建议。".into())
                    } else {
                        match name.as_str() {
                            "get_text_frames" => app.run("ai.text.frames", params),
                            "read_text" => app.run("ai.text.read", params),
                            "propose_text_change" => app.run("ai.suggestion.propose", params),
                            _ => Err("不允许的工具".into()),
                        }
                    };
                    let _ = reply.send(result);
                }
                Event::Done { result, .. } => {
                    let is_doc = app.ai.native.task.as_ref().is_some_and(|t| t.doc.is_some());
                    let before = app.ai.native.task.as_ref().map_or(0, |t| t.initial_suggestions);
                    let after = app.session.active().and_then(|st| st.doc.ai.as_ref()).map_or(0, |a| a.suggestions.len());
                    let generated = after.saturating_sub(before);
                    app.ai.native.task = None;
                    if is_doc {
                        let _ = app.run("ai.task.status", json!({"status":if result.is_ok(){"completed"}else{"failed"}}));
                    }
                    match result {
                        Ok(text) => {
                            if is_doc {
                                let _ = app.run("ai.chat.append", json!({"role":"assistant","text":text}));
                            }
                            app.ai.notice = if !is_doc {
                                text
                            } else if generated == 0 {
                                "本次未生成修改建议，请查看下方回复。".into()
                            } else {
                                format!("已生成 {generated} 条修改建议，请审核后接受。")
                            };
                        }
                        Err(e) => {
                            app.ai.notice = e;
                        }
                    }
                }
                Event::Settings { result, .. } => {
                    app.ai.native.task = None;
                    match result {
                        Ok(s) => {
                            app.ai.native.saved = s.clone();
                            app.ai.native.settings = s;
                            app.ai.native.models.clear();
                            app.ai.notice = "账户设置已更新，请加载模型。".into();
                        }
                        Err(e) => app.ai.notice = e,
                    }
                }
                Event::Models { result, .. } => {
                    app.ai.native.task = None;
                    match result {
                        Ok(m) => {
                            app.ai.native.models = m;
                            app.ai.notice = "模型列表已更新。".into();
                        }
                        Err(e) => app.ai.notice = e,
                    }
                }
            }
        }
        if busy(app) {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
    fn stop(app: &mut DesignApp, notice: &str) {
        if let Some(task) = app.ai.native.task.take() {
            task.cancel.cancel();
            if let Some(uid) = task.doc {
                // Mark the original document, even after a tab switch. No selection switch required.
                let _ = app.run("ai.task.status", json!({"document":uid,"status":"interrupted"}));
            }
        }
        app.ai.notice = notice.into();
    }
    pub fn command(app: &mut DesignApp, id: &str, p: &Value) -> Result<Value, String> {
        if id == "ai.task.inspect" {
            return Ok(
                json!({"busy":busy(app),"notice":app.ai.notice,"models":app.ai.native.models,"task":app.ai.native.task.as_ref().map(|t|t.id)}),
            );
        }
        if id == "ai.task.stop" {
            stop(app, "已停止；已有建议仍可查看。");
            return Ok(Value::Null);
        }
        if id == "ai.settings.get" {
            return Ok(json!(app.ai.native.saved));
        }
        if busy(app) {
            return Err("请等待当前任务完成，或先停止。".into());
        }
        match id {
            "ai.settings.save" => {
                let mut s = app.ai.native.settings.clone();
                for (key, target) in
                    [("provider", &mut s.provider), ("endpoint", &mut s.endpoint), ("model", &mut s.model), ("account", &mut s.account)]
                {
                    if let Some(v) = p[key].as_str() {
                        *target = v.into();
                    }
                }
                s.save(p["apiKey"].as_str())?;
                app.ai.native.saved = s.clone();
                app.ai.native.settings = s;
                app.ai.native.key.clear();
                app.ai.notice = "设置已保存。".into();
                Ok(Value::Null)
            }
            "ai.auth.usage" => {
                web_usage()?;
                Ok(Value::Null)
            }
            "ai.auth.login" | "ai.auth.logout" | "ai.models" | "ai.connection.test" => {
                let (task, cancel, tx) = start(app, None, None)?;
                let mut s = if id.starts_with("ai.auth.") { app.ai.native.settings.clone() } else { app.ai.native.saved.clone() };
                if p["newAccount"] == true {
                    s.account.clear();
                }
                let id = id.to_string();
                std::thread::spawn(move || {
                    let e = match id.as_str() {
                        "ai.auth.login" => Event::Settings { task, result: native::login(s, &cancel) },
                        "ai.auth.logout" => Event::Settings { task, result: native::logout(s) },
                        "ai.models" => Event::Models { task, result: native::models(&s) },
                        _ => Event::Done { task, result: native::test_connection(&s, &cancel) },
                    };
                    let _ = tx.send(e);
                });
                Ok(json!({"task":task}))
            }
            "ai.chat.send" | "ai.proofreadSelection" => {
                let st = app.session.active().ok_or("请先打开文档")?;
                let uid = st.uid;
                let settings = app.ai.native.saved.clone();
                if settings.model.trim().is_empty() {
                    return Err("请先在设置中选择模型并保存。".into());
                }
                let proof = id == "ai.proofreadSelection";
                let history = if proof { vec![] } else { st.doc.ai.as_ref().map(|a| a.messages.clone()).unwrap_or_default() };
                let mut prompts = vec![];
                let mut allowed = None;
                if proof {
                    let t = st.selection.text.filter(|t| !t.is_caret() && t.cell.is_none()).ok_or("请先用文字工具选择普通正文。")?;
                    let text = st.doc.story(t.story).and_then(|s| s.text.get(t.range())).ok_or("选区无效")?.to_string();
                    if text.trim().is_empty() {
                        return Err("选区没有正文".into());
                    }
                    let mut ids = HashSet::new();
                    for range in designcraft_ai::batches(&text, 16000) {
                        let offset = text[..range.start].chars().count();
                        let length = text[range.clone()].chars().count();
                        let snap = app.run("ai.text.read", json!({"offset":offset,"length":length}))?;
                        ids.insert(snap["snapshot"].as_u64().unwrap());
                        prompts.push(format!("仅校对这份选区快照中的标点和明确错别字，保留原意、数字、专名、段落与语言习惯，不确定项说明供人工核实。不要读取其他内容。使用 propose_text_change 提出修改，offset 相对于本快照的 Unicode 字符位置。快照：{}",snap));
                    }
                    allowed = Some(ids);
                } else {
                    let text = p["text"].as_str().filter(|s| !s.trim().is_empty()).ok_or("请输入内容")?;
                    prompts.push(text.to_string());
                }
                let (task, cancel, tx) = start(app, Some(uid), allowed)?;
                let user = if proof { "修复选区中的标点和错别字".to_string() } else { prompts[0].clone() };
                app.run("ai.chat.append", json!({"role":"user","text":user}))?;
                app.run("ai.task.status", json!({"status":"running"}))?;
                app.ui.ai.open = true;
                app.ui.ai.tab = "chat".into();
                std::thread::spawn(move || {
                    let result = (|| {
                        let mut summaries = vec![];
                        for prompt in prompts {
                            cancel.check()?;
                            let summary = native::conversation(&settings, &history, &prompt, &cancel, proof, |name, params| {
                                let (reply, rx) = mpsc::channel();
                                tx.send(Event::Tool { task, name: name.into(), params, reply }).map_err(|_| "窗口已关闭")?;
                                loop {
                                    cancel.check()?;
                                    match rx.recv_timeout(Duration::from_millis(100)) {
                                        Ok(v) => return v,
                                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                                        Err(_) => return Err("任务已结束".into()),
                                    }
                                }
                            })?;
                            summaries.push(summary);
                        }
                        Ok(summaries.join("\n"))
                    })();
                    let _ = tx.send(Event::Done { task, result });
                });
                Ok(json!({"task":task}))
            }
            "ai.report.export" => {
                let html = app.run("ai.report.html", json!({}))?.as_str().ok_or("报告生成失败")?.to_string();
                let st = app.session.active().ok_or("没有文档")?;
                let default = st.path.as_ref().map(|p| std::path::Path::new(p).with_extension("校对报告.html").to_string_lossy().to_string());
                let path = p["path"]
                    .as_str()
                    .map(str::to_owned)
                    .or(default)
                    .or_else(|| app.services.pick_save.as_mut().and_then(|pick| pick("校对报告.html")))
                    .ok_or("未选择保存路径")?;
                use std::io::Write;
                let base = std::path::PathBuf::from(path);
                let mut path = base.clone();
                let mut n = 1;
                loop {
                    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                        Ok(mut f) => {
                            if let Err(e) = f.write_all(html.as_bytes()) {
                                drop(f);
                                let _ = std::fs::remove_file(&path);
                                return Err(e.to_string());
                            }
                            break;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                            n += 1;
                            path = base.with_file_name(format!("{}_{}.html", base.file_stem().unwrap_or_default().to_string_lossy(), n));
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                }
                app.ai.notice = format!("报告已保存：{}", path.display());
                Ok(json!({"path":path}))
            }
            _ => Err(format!("unknown AI command: {id}")),
        }
    }
    fn web_usage() -> Result<(), String> {
        native::open_usage()
    }

    impl Drop for Runtime {
        fn drop(&mut self) {
            if let Some(t) = &self.task {
                t.cancel.cancel();
            }
        }
    }
    pub fn settings(app: &mut DesignApp, ui: &mut egui::Ui) {
        let running = busy(app);
        ui.add_enabled_ui(!running, |ui| {
            let s = &mut app.ai.native.settings;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.provider, "compatible".into(), "兼容 API");
                ui.selectable_value(&mut s.provider, "chatgpt".into(), "ChatGPT 登录");
            });
            if s.provider == "compatible" {
                ui.label("服务地址");
                ui.text_edit_singleline(&mut s.endpoint);
                ui.label("API Key（保存在系统凭据存储）");
                ui.add(egui::TextEdit::singleline(&mut app.ai.native.key).password(true).hint_text("留空保留已有 Key"));
            } else {
                let old = s.account.clone();
                egui::ComboBox::from_id_salt("ai_account")
                    .selected_text(s.accounts.iter().find(|a| a.id == s.account).map(|a| a.label.as_str()).unwrap_or("选择账户"))
                    .show_ui(ui, |ui| {
                        for a in &s.accounts {
                            ui.selectable_value(&mut s.account, a.id.clone(), &a.label);
                        }
                    });
                if old != s.account {
                    s.model.clear();
                    app.ai.native.models.clear();
                }
                if s.accounts.iter().any(|a| a.id == s.account && !a.sharing) {
                    ui.label("此账户尚未授予订阅推理权限，请重新授权。");
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Continue with ChatGPT").clicked() {
                        action(app, "ai.auth.login", json!({}));
                    }
                    if ui.button("添加账户").clicked() {
                        action(app, "ai.auth.login", json!({"newAccount":true}));
                    }
                    if ui.button("退出登录").clicked() {
                        action(app, "ai.auth.logout", json!({}));
                    }
                    if ui.button("管理用量").clicked() {
                        action(app, "ai.auth.usage", json!({}));
                    }
                });
            }
            ui.separator();
            ui.label("模型");
            ui.text_edit_singleline(&mut app.ai.native.settings.model);
            if !app.ai.native.models.is_empty() {
                egui::ComboBox::from_id_salt("ai_models").selected_text("从可用模型中选择").show_ui(ui, |ui| {
                    for m in &app.ai.native.models {
                        ui.selectable_value(&mut app.ai.native.settings.model, m.clone(), m);
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("保存设置").clicked() {
                    let mut p = json!({});
                    if !app.ai.native.key.is_empty() {
                        p["apiKey"] = json!(app.ai.native.key);
                    }
                    action(app, "ai.settings.save", p);
                }
                if ui.button("加载模型").clicked() {
                    action(app, "ai.models", json!({}));
                }
                if ui.button("测试连接").clicked() {
                    action(app, "ai.connection.test", json!({}));
                }
            });
            ui.separator();
            ui.label("聊天与修改建议保存在文档中，分享文档时会一并包含。");
            if ui.button("清除当前文档 AI 记录").clicked() {
                action(app, "ai.records.clear", json!({}));
            }
        });
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        fn app() -> DesignApp {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
            app.run("file.new", json!({})).unwrap();
            app.run("frame.create", json!({"rect":[0,0,200,200],"content":"text","text":"hello"})).unwrap();
            app
        }
        #[test]
        fn close_keeps_task_but_switch_cancels_and_drops_late_results() {
            let mut app = app();
            let uid = app.session.active().unwrap().uid;
            let (task, cancel, tx) = start(&mut app, Some(uid), None).unwrap();
            app.run("window.ai", json!({"open":false})).unwrap();
            assert!(busy(&app));
            assert!(cancel.check().is_ok());
            app.run("file.new", json!({})).unwrap();
            tx.send(Event::Done { task, result: Ok("late response".into()) }).unwrap();
            poll(&mut app, &egui::Context::default());
            assert!(!busy(&app));
            assert!(cancel.check().is_err());
            assert!(app.session.active().unwrap().doc.ai.is_none());
        }
        #[test]
        fn worker_cannot_accept_and_scope_rejects_other_snapshots() {
            let mut app = app();
            let uid = app.session.active().unwrap().uid;
            let (task, _, tx) = start(&mut app, Some(uid), Some(HashSet::from([42]))).unwrap();
            let (reply, rx) = mpsc::channel();
            tx.send(Event::Tool { task, name: "ai.suggestion.accept".into(), params: json!({"id":1}), reply }).unwrap();
            poll(&mut app, &egui::Context::default());
            assert!(rx.try_recv().unwrap().is_err());
            let (reply, rx) = mpsc::channel();
            tx.send(Event::Tool { task, name: "propose_text_change".into(), params: json!({"snapshot":43}), reply }).unwrap();
            poll(&mut app, &egui::Context::default());
            assert!(rx.try_recv().unwrap().is_err());
        }
        #[test]
        fn captured_selection_proposal_survives_review_and_undo() {
            let mut app = app();
            let story = app.session.active().unwrap().doc.stories.keys().next().unwrap().0;
            let snap = app.run("ai.text.read", json!({"story":story})).unwrap();
            let uid = app.session.active().unwrap().uid;
            let (task, _, tx) = start(&mut app, Some(uid), Some(HashSet::from([snap["snapshot"].as_u64().unwrap()]))).unwrap();
            let (reply, rx) = mpsc::channel();
            tx.send(Event::Tool {
                task,
                name: "propose_text_change".into(),
                params: json!({"snapshot":snap["snapshot"],"offset":0,"original":"hello","replacement":"Hello","reason":"首字母"}),
                reply,
            })
            .unwrap();
            poll(&mut app, &egui::Context::default());
            assert!(rx.try_recv().unwrap().is_ok());
            tx.send(Event::Done { task, result: Ok("请审核".into()) }).unwrap();
            poll(&mut app, &egui::Context::default());
            assert!(app.ai.notice.contains("1 条"));
            app.run("ai.suggestion.acceptAll", json!({})).unwrap();
            assert_eq!(app.session.active().unwrap().doc.stories.values().next().unwrap().text, "Hello");
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.session.active().unwrap().doc.stories.values().next().unwrap().text, "hello");
            let (task, _, tx) = start(&mut app, Some(uid), None).unwrap();
            tx.send(Event::Done { task, result: Ok("没有建议".into()) }).unwrap();
            poll(&mut app, &egui::Context::default());
            assert!(app.ai.notice.contains("未生成"));
        }
        #[test]
        fn external_edit_cancels_before_queued_tool_execution() {
            let mut app = app();
            let uid = app.session.active().unwrap().uid;
            let (task, cancel, tx) = start(&mut app, Some(uid), None).unwrap();
            app.run("text.insert", json!({"text":"changed"})).unwrap();
            let (reply, rx) = mpsc::channel();
            tx.send(Event::Tool { task, name: "read_text".into(), params: json!({}), reply }).unwrap();
            poll(&mut app, &egui::Context::default());
            assert!(cancel.check().is_err());
            assert!(rx.try_recv().is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_state_survives_close_and_ui_serialization() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("window.ai", json!({})).unwrap();
        assert!(app.ui.ai.open);
        assert!(!app.ui.ai.floating);
        app.run("window.ai.layout", json!({"floating":true,"position":[120,160],"tab":"suggestions"})).unwrap();
        app.run("window.ai", json!({"open":false})).unwrap();
        let saved = serde_json::to_vec(&app.ui).unwrap();
        app.ui = serde_json::from_slice(&saved).unwrap();
        app.run("window.ai", json!({})).unwrap();
        assert!(app.ui.ai.floating);
        assert_eq!(app.ui.ai.position, [120.0, 160.0]);
        assert_eq!(app.ui.ai.tab, "suggestions");
        app.run("window.ai.layout", json!({"floating":false})).unwrap();
        assert!(!app.ui.ai.floating);
    }
}
