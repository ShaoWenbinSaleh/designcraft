//! Review commands. The model dispatcher exposes only frames/read/propose.
use super::{CommandSpec, bad, cmd, has_doc, ok};
use crate::{Result, Session};
use designcraft_doc::{
    Document, StoryId, TextSel,
    ai::{AiDocument, Message, Snapshot, Status, Suggestion},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn err(s: impl Into<String>) -> crate::EngineError {
    bad("ai", s)
}
fn number(p: &Value, name: &str) -> Result<u64> {
    p[name].as_u64().ok_or_else(|| err(format!("missing {name}")))
}
fn string<'a>(p: &'a Value, name: &str) -> Result<&'a str> {
    p[name].as_str().ok_or_else(|| err(format!("missing {name}")))
}
pub fn data(doc: &mut Document) -> &mut AiDocument {
    Arc::make_mut(doc.ai.get_or_insert_with(|| Arc::new(AiDocument::default())))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "ai.text.frames", "AI Text Frames", [], None, "{} → stories with frames and character counts", has_doc, |s,_| {
            Ok(json!(s.doc()?.doc.stories.values().map(|st| json!({"story":st.id.0,"frames":st.frames,"characters":st.text.chars().count()})).collect::<Vec<_>>()))
        }),
        cmd!(query "ai.text.read", "AI Read Text", [], None, "{story?, offset?: Unicode characters, length?: characters (max 16000)}; without story reads selected text", has_doc, read),
        cmd!(query "ai.suggestion.propose", "AI Propose Change", [], None, "{snapshot, original: unique exact quote, replacement, reason, offset?: Unicode characters within snapshot}", has_doc, propose),
        cmd!(query "ai.suggestion.list", "AI Suggestions", [], None, "{} → current review data; revalidates pending suggestions", has_doc, |s,_| {
            refresh(s)?; Ok(json!(s.doc()?.doc.ai))
        }),
        cmd!("ai.suggestion.accept", "Accept AI Suggestion", [], None, "{id}", has_doc, |s, p| accept(s, Some(number(p, "id")?))),
        cmd!("ai.suggestion.acceptAll", "Accept All AI Suggestions", [], None, "{} — atomic, one undo step", has_doc, |s, _| accept(s, None)),
        cmd!("ai.suggestion.reject", "Reject AI Suggestion", [], None, "{id}", has_doc, |s, p| {
            let id = number(p, "id")?;
            s.edit(|d, _| {
                let item = data(d).suggestions.iter_mut().find(|x| x.id == id).ok_or_else(|| err("unknown suggestion"))?;
                if item.status != Status::Pending {
                    return Err(err("suggestion is not pending"));
                }
                item.status = Status::Rejected;
                ok()
            })
        }),
        cmd!(query "ai.suggestion.locate", "Locate AI Original", [], None, "{id} → story, start, end, frame?", has_doc, locate),
        cmd!(query "ai.chat.append", "Append AI Message", [], None, "{role: user|assistant, text}", has_doc, |s,p| {
            let role=string(p,"role")?; if !matches!(role,"user"|"assistant") { return Err(err("invalid role")); }
            let text=string(p,"text")?; if text.len()>1_000_000 { return Err(err("message too large")); }
            s.edit(|d,_| { data(d).messages.push(Message{role:role.into(),text:text.into()}); ok() })
        }),
        cmd!(query "ai.task.status", "AI Task Status", [], None, "{status?: string, document?: session UID}; no status reads state", has_doc, |s,p| {
            if let Some(status)=p["status"].as_str() {
                if let Some(uid)=p["document"].as_u64() {
                    let st=s.docs.iter_mut().find(|st|st.uid==uid).ok_or_else(||err("document is closed"))?;
                    data(Arc::make_mut(&mut st.doc)).task_status=status.into();st.revision+=1;ok()
                } else {s.edit(|d,_| {data(d).task_status=status.into(); ok()})}
            }
            else {Ok(json!(s.doc()?.doc.ai.as_ref().map(|a|a.task_status.as_str()).unwrap_or("")))}
        }),
        cmd!("ai.chat.clear", "New AI Conversation", [], None, "{} — preserves suggestions", has_doc, |s, _| s.edit(|d, _| {
            data(d).messages.clear();
            ok()
        })),
        cmd!("ai.records.clear", "Clear AI Records", [], None, "{} — clears chat, suggestions and snapshots", has_doc, |s, _| s.edit(|d, _| {
            d.ai = None;
            ok()
        })),
        cmd!(query "ai.report.html", "AI Review Report HTML", [], None, "{} → standalone HTML", has_doc, |s,_| {
            refresh(s)?; let d=&s.doc()?.doc; Ok(json!(designcraft_ai::report(&d.title, d.ai.as_deref().unwrap_or(&AiDocument::default()))))
        }),
    ]
}

pub fn read(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let (sid, base, bound) = if let Some(id) = p["story"].as_u64() {
        let story = st.doc.story(StoryId(id)).ok_or_else(|| err("unknown story"))?;
        (story.id, 0, story.len())
    } else {
        let t = st.selection.text.filter(|t| !t.is_caret() && t.cell.is_none()).ok_or_else(|| err("请先用文字工具选择普通正文。"))?;
        (t.story, t.range().start, t.range().end)
    };
    if st.doc.endnote_story == Some(sid) {
        return Err(err("首期仅支持普通正文，暂不支持尾注文章。"));
    }
    let story = st.doc.story(sid).ok_or_else(|| err("unknown story"))?;
    let selected = story.text.get(base..bound).ok_or_else(|| err("invalid text boundaries"))?;
    let offset = p["offset"].as_u64().unwrap_or(0) as usize;
    let start = base + designcraft_ai::byte_offset(selected, offset).ok_or_else(|| err("offset out of range"))?;
    let length = (p["length"].as_u64().unwrap_or(16000) as usize).min(16000);
    let end = start + designcraft_ai::byte_offset(&story.text[start..bound], length).unwrap_or(bound - start);
    let text = Arc::new(story.text.clone());
    let part = text[start..end].to_string();
    let total = selected.chars().count();
    s.edit(|d, _| {
        let a = data(d);
        if a.snapshots.len() >= 10000 {
            return Err(err("快照过多，请清除 AI 记录后重试。"));
        }
        let id = a.alloc();
        a.snapshots.insert(id, Snapshot { story: sid, text, start, end });
        Ok(json!({"snapshot":id,"story":sid.0,"text":part,"offset":offset,"length":part.chars().count(),"total":total,"hasMore":end<bound}))
    })
}

pub fn propose(s: &mut Session, p: &Value) -> Result<Value> {
    let snapshot_id = number(p, "snapshot")?;
    let snap = s.doc()?.doc.ai.as_ref().and_then(|a| a.snapshots.get(&snapshot_id)).cloned().ok_or_else(|| err("先读取正文以创建快照。"))?;
    if s.doc()?.doc.story(snap.story).is_none_or(|t| t.text != *snap.text) {
        return Err(err("正文已变化，请重新读取。"));
    }
    let original = string(p, "original")?;
    let replacement = string(p, "replacement")?.replace("\r\n", "\n").replace('\r', "\n");
    let reason = string(p, "reason")?;
    if !designcraft_ai::same_layout(original, &replacement) {
        return Err(err("AI 仅修改文字内容，不能增删段落、制表符或换行等排版标记。"));
    }
    if original == replacement {
        return Err(err("no change"));
    }
    if replacement.len() > 200000 || reason.len() > 10000 {
        return Err(err("suggestion too large"));
    }
    let part = snap.text.get(snap.start..snap.end).ok_or_else(|| err("invalid snapshot"))?;
    let offset = p.get("offset").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok());
    let relative = designcraft_ai::quote_offset(part, original, offset).map_err(err)?;
    let mut start = snap.start + relative;
    let mut end = start + original.len();
    if designcraft_ai::protected(original) || designcraft_ai::protected(&replacement) {
        return Err(err("不能跨越或插入特殊对象标记。"));
    }
    // Trim unchanged edges before touching formatting.
    let prefix = original.chars().zip(replacement.chars()).take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum::<usize>();
    let suffix = original[prefix..]
        .chars()
        .rev()
        .zip(replacement[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    start += prefix;
    end -= suffix;
    let replacement = replacement[prefix..replacement.len() - suffix].to_owned();
    let original = snap.text[start..end].to_owned();
    let context = snap.text[snap.start..snap.end].chars().take(16000).collect();
    let page = page_at(s, snap.story, start);
    let label = page.map(|n| s.doc().unwrap().doc.page_name(n));
    s.edit(|d, _| {
        let a = data(d);
        if a.suggestions.len() >= 10000 {
            return Err(err("建议过多，请导出并清除记录。"));
        }
        if let Some(old) = a.suggestions.iter().find(|x| {
            x.status == Status::Pending && x.story == snap.story && x.start == start && x.original == original && x.replacement == replacement
        }) {
            return Ok(json!({"id":old.id}));
        }
        let id = a.alloc();
        a.suggestions.push(Suggestion {
            id,
            snapshot: snapshot_id,
            story: snap.story,
            start,
            end,
            original,
            replacement,
            reason: reason.into(),
            status: Status::Pending,
            source_start: start,
            source_end: end,
            context,
            page,
            page_label: label,
        });
        Ok(json!({"id":id}))
    })
}
fn valid(doc: &Document, a: &AiDocument, x: &Suggestion) -> bool {
    designcraft_ai::same_layout(&x.original, &x.replacement)
        && !designcraft_ai::protected(&x.original)
        && !designcraft_ai::protected(&x.replacement)
        && a.snapshots.get(&x.snapshot).is_some_and(|sp| {
            sp.story == x.story
                && x.start >= sp.start
                && x.end <= sp.end
                && doc.story(x.story).is_some_and(|st| st.text == *sp.text && st.text.get(x.start..x.end) == Some(x.original.as_str()))
        })
}

pub fn refresh(s: &mut Session) -> Result<()> {
    let d = &s.doc()?.doc;
    let stale: Vec<_> =
        d.ai.as_ref()
            .map(|a| a.suggestions.iter().filter(|x| x.status == Status::Pending && !valid(d, a, x)).map(|x| x.id).collect())
            .unwrap_or_default();
    if !stale.is_empty() {
        s.edit(|d, _| {
            for x in &mut data(d).suggestions {
                if stale.contains(&x.id) {
                    x.status = Status::Stale;
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}
fn overlaps(a: &Suggestion, b: &Suggestion) -> bool {
    a.story == b.story
        && (a.start < b.end && b.start < a.end || a.start == b.start || a.start == a.end && a.start == b.end || b.start == b.end && b.start == a.end)
}
fn accept(s: &mut Session, id: Option<u64>) -> Result<Value> {
    // No mutation (including status changes) until the whole batch is validated.
    let d = &s.doc()?.doc;
    let a = d.ai.as_ref().ok_or_else(|| err("no suggestions"))?;
    let mut edits: Vec<_> = a.suggestions.iter().filter(|x| x.status == Status::Pending && id.is_none_or(|id| x.id == id)).cloned().collect();
    if edits.is_empty() {
        return Err(err("没有待确认建议。"));
    }
    if edits.iter().any(|x| !valid(d, a, x)) {
        return Err(err("正文已变化，请重新生成建议。"));
    }
    edits.sort_by_key(|x| (x.story.0, x.start, x.end));
    if edits.windows(2).any(|w| overlaps(&w[0], &w[1])) {
        return Err(err("建议范围重叠，请逐条处理。"));
    }
    s.edit(|d, sel| {
        for x in edits.iter().rev() {
            let changes = designcraft_ai::text_edits(&x.original, &x.replacement).map_err(err)?;
            let story = d.story_mut(x.story).ok_or_else(|| err("unknown story"))?;
            for change in changes.iter().rev() {
                let range = x.start + change.start..x.start + change.end;
                let formats: Vec<_> = story.text[range.clone()].char_indices().map(|(i, _)| story.format_after(range.start + i).clone()).collect();
                let inherited = story.char_format_at(range.start).clone();
                story.replace(range.clone(), &change.replacement);
                let mut runs: Vec<(std::ops::Range<usize>, designcraft_doc::story::CharFormat)> = vec![];
                for (i, (byte, c)) in change.replacement.char_indices().enumerate() {
                    let format = formats.get(i.min(formats.len().saturating_sub(1))).unwrap_or(&inherited);
                    if let Some((r, f)) = runs.last_mut()
                        && f == format
                    {
                        r.end += c.len_utf8();
                    } else {
                        runs.push((range.start + byte..range.start + byte + c.len_utf8(), format.clone()));
                    }
                }
                for (range, format) in runs {
                    story.format_chars(range, |f| *f = format.clone());
                }
            }
        }
        let mut a = d.ai.as_deref().unwrap().clone();
        for x in &mut a.suggestions {
            if edits.iter().any(|e| e.id == x.id) {
                x.status = Status::Accepted;
                continue;
            }
            if x.status != Status::Pending {
                continue;
            }
            if edits.iter().any(|e| overlaps(e, x)) {
                x.status = Status::Stale;
                continue;
            }
            let delta: isize = edits
                .iter()
                .filter(|e| e.story == x.story && e.end <= x.start)
                .map(|e| e.replacement.len() as isize - (e.end - e.start) as isize)
                .sum();
            x.start = x.start.saturating_add_signed(delta);
            x.end = x.end.saturating_add_signed(delta);
            if edits.iter().any(|e| e.story == x.story) {
                let sid = a.next_id;
                a.next_id += 1;
                let text = Arc::new(d.story(x.story).unwrap().text.clone());
                a.snapshots.insert(sid, Snapshot { story: x.story, start: 0, end: text.len(), text });
                x.snapshot = sid;
            }
        }
        if let Some(t) = sel.text.as_mut() {
            for e in edits.iter().rev().filter(|e| e.story == t.story) {
                for pos in [&mut t.anchor, &mut t.focus] {
                    if *pos >= e.end {
                        *pos = pos.saturating_add_signed(e.replacement.len() as isize - (e.end - e.start) as isize);
                    } else if *pos > e.start {
                        *pos = e.start + e.replacement.len();
                    }
                }
            }
        }
        d.ai = Some(Arc::new(a));
        Ok(json!({"accepted":edits.len()}))
    })
}
fn page_at(s: &Session, sid: StoryId, pos: usize) -> Option<usize> {
    let d = &s.doc().ok()?.doc;
    let c = s.cache.get(d, sid, None);
    let frame = c.frames.iter().find(|f| f.range.contains(&pos) || pos == f.range.end && pos == d.story(sid).unwrap().len())?;
    let loc = d.find(frame.frame)?;
    let spread = d.spread(loc.spread)?;
    let bounds = d.item(frame.frame)?.bounds();
    let page = spread.pages.iter().find(|p| bounds.center().x >= p.x && bounds.center().x <= p.x + p.width).or(spread.pages.first())?;
    (0..d.page_count()).find(|&n| d.page(n).is_some_and(|p| p.id == page.id))
}
fn locate(s: &mut Session, p: &Value) -> Result<Value> {
    refresh(s)?;
    let id = number(p, "id")?;
    let d = &s.doc()?.doc;
    let a = d.ai.as_ref().ok_or_else(|| err("no suggestions"))?;
    let x = a.suggestions.iter().find(|x| x.id == id).cloned().ok_or_else(|| err("unknown suggestion"))?;
    if x.status != Status::Pending || !valid(d, a, &x) {
        return Err(err("此建议已处理或原文已变化。"));
    }
    let page = page_at(s, x.story, x.start);
    let st = s.doc_mut()?;
    st.selection.text = Some(TextSel { story: x.story, anchor: x.start, focus: x.end, frame: None, cell: None });
    st.selection.items.clear();
    st.revision += 1;
    s.set_tool("type");
    Ok(json!({"story":x.story.0,"start":x.start,"end":x.end,"page":page}))
}

/// Chat and subsequently-created review records survive unrelated document undo.
/// Existing records retain the historical acceptance state, so accepting is reversible.
pub(super) fn preserve_records(restored: &mut Document, current: &Document) {
    let Some(current) = &current.ai else {
        return;
    };
    let a = data(restored);
    a.messages = current.messages.clone();
    a.task_status = current.task_status.clone();
    a.next_id = a.next_id.max(current.next_id);
    for (id, snap) in &current.snapshots {
        a.snapshots.entry(*id).or_insert_with(|| snap.clone());
    }
    for item in &current.suggestions {
        if !a.suggestions.iter().any(|s| s.id == item.id) {
            a.suggestions.push(item.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(text: &str) -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = s.execute("frame.create", &json!({"rect":[36,36,300,200],"content":"text"})).unwrap();
        s.execute("text.insert", &json!({"text":text,"raw":true})).unwrap();
        (s, frame["story"].as_u64().unwrap())
    }
    fn suggestion(s: &mut Session, sid: u64, offset: usize, original: &str, replacement: &str) -> u64 {
        let r = s.execute("ai.text.read", &json!({"story":sid})).unwrap();
        s.execute(
            "ai.suggestion.propose",
            &json!({"snapshot":r["snapshot"],"offset":offset,"original":original,"replacement":replacement,"reason":"校对"}),
        )
        .unwrap()["id"]
            .as_u64()
            .unwrap()
    }
    #[test]
    fn unicode_rebase_atomic_accept_undo_and_save() {
        let (mut s, sid) = fixture("你😀好，错字错字。");
        let first = suggestion(&mut s, sid, 3, "，", "！好");
        let later = suggestion(&mut s, sid, 6, "错", "正");
        s.execute("ai.chat.append", &json!({"role":"user","text":"保留聊天"})).unwrap();
        s.execute("ai.suggestion.accept", &json!({"id":first})).unwrap();
        s.execute("ai.suggestion.locate", &json!({"id":later})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, "你😀好！好错字错字。");
        s.execute("ai.suggestion.accept", &json!({"id":later})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, "你😀好！好错字正字。");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.ai.as_ref().unwrap().suggestions[1].status, Status::Pending);
        assert_eq!(s.doc().unwrap().doc.ai.as_ref().unwrap().messages.len(), 1);
        let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
        let doc = designcraft_format::load(&bytes).unwrap();
        assert_eq!(doc.ai, s.doc().unwrap().doc.ai);
        assert_eq!(doc.story(StoryId(sid)).unwrap().text, "你😀好！好错字错字。");
    }
    #[test]
    fn batch_is_one_undo_and_overlaps_never_partially_apply() {
        let (mut s, sid) = fixture("a bad word and a bad word");
        suggestion(&mut s, sid, 2, "bad", "good");
        suggestion(&mut s, sid, 17, "bad", "fine");
        let before = s.doc().unwrap().history.undo.len();
        s.execute("ai.suggestion.acceptAll", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), before + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, "a bad word and a bad word");
        suggestion(&mut s, sid, 2, "bad word", "phrase");
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("ai.suggestion.acceptAll", &json!({})).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
    #[test]
    fn external_changes_mark_pending_stale_and_keep_objects() {
        let (mut s, sid) = fixture("错字\u{e009}结束");
        let id = suggestion(&mut s, sid, 0, "错", "正");
        let snap = s.execute("ai.text.read", &json!({"story":sid})).unwrap();
        assert!(
            s.execute(
                "ai.suggestion.propose",
                &json!({"snapshot":snap["snapshot"],"offset":1,"original":"字\u{e009}","replacement":"字","reason":"bad"})
            )
            .is_err()
        );
        s.execute("text.insert", &json!({"text":"外部"})).unwrap();
        assert!(s.execute("ai.suggestion.accept", &json!({"id":id})).is_err());
        s.execute("ai.suggestion.list", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.ai.as_ref().unwrap().suggestions[0].status, Status::Stale);
    }
    #[test]
    fn selected_range_pagination_and_wrong_document() {
        let (mut s, sid) = fixture("前😀选区尾");
        s.execute("text.select", &json!({"story":sid,"anchor":3,"focus":13})).unwrap();
        let r = s.execute("ai.text.read", &json!({"offset":1,"length":1})).unwrap();
        assert_eq!(r["text"], "选");
        assert_eq!(r["hasMore"], true);
        s.execute("file.new", &json!({})).unwrap();
        assert!(
            s.execute("ai.suggestion.propose", &json!({"snapshot":r["snapshot"],"offset":0,"original":"选","replacement":"正","reason":"test"}))
                .is_err()
        );
    }
    #[test]
    fn exact_quote_localization_handles_chinese_emoji_and_wrong_offsets() {
        let text = "选区外错字。😀这是一份测式，第二句也是测式。结束";
        let (mut s, sid) = fixture(text);
        let base = "选区外错字。".len();
        s.execute("text.select", &json!({"story":sid,"anchor":base,"focus":text.len()})).unwrap();
        let snap = s.execute("ai.text.read", &json!({})).unwrap();
        // Repeated originals must never silently pick the first/nearest occurrence.
        for original in ["测式", "不在原文", ""] {
            assert!(
                s.execute("ai.suggestion.propose", &json!({"snapshot":snap["snapshot"],"original":original,"replacement":"测试","reason":"校对"}))
                    .is_err()
            );
        }
        // The host, not the model, computes byte boundaries inside the selection.
        for (original, replacement, offset) in [("一份测式", "一份测试", Value::Null), ("第二句也是测式", "第二句也是测试", json!(999))]
        {
            s.execute(
                "ai.suggestion.propose",
                &json!({"snapshot":snap["snapshot"],"original":original,"replacement":replacement,"offset":offset,"reason":"校对"}),
            )
            .unwrap();
        }
        s.execute("ai.suggestion.acceptAll", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, "选区外错字。😀这是一份测试，第二句也是测试。结束");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, text);
        // Overlapping matches also count as ambiguous.
        assert!(designcraft_ai::quote_offset("哈哈哈", "哈哈", None).is_err());
    }
    #[test]
    fn content_only_preserves_mixed_formats_paragraphs_and_frames() {
        let text = "错😀强调错\n另一段错\t尾";
        let (mut s, sid) = fixture(text);
        s.edit(|d, _| {
            let st = d.story_mut(StoryId(sid)).unwrap();
            for (i, (pos, c)) in text.char_indices().enumerate() {
                st.format_chars(pos..pos + c.len_utf8(), |f| f.over.size = Some(10.0 + i as f64));
            }
            st.paras[1].para.space_before = Some(17.0);
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.clone();
        let old = before.story(StoryId(sid)).unwrap();
        let snap = s.execute("ai.text.read", &json!({"story":sid})).unwrap();
        for replacement in ["错😀强调错另一段错\t尾", "错😀强调错\n另一段错 尾"] {
            assert!(
                s.execute("ai.suggestion.propose", &json!({"snapshot":snap["snapshot"],"original":text,"replacement":replacement,"reason":"bad"}))
                    .is_err()
            );
        }
        let changed = "正😀强调对\n另一段正\t尾";
        s.execute("ai.suggestion.propose", &json!({"snapshot":snap["snapshot"],"original":text,"replacement":changed,"reason":"校对"})).unwrap();
        s.execute("ai.suggestion.acceptAll", &json!({})).unwrap();
        let doc = &s.doc().unwrap().doc;
        let st = doc.story(StoryId(sid)).unwrap();
        assert_eq!(st.text, changed);
        assert_eq!(st.chars, old.chars);
        assert_eq!(st.paras, old.paras);
        assert_eq!(st.frames, old.frames);
        assert_eq!(serde_json::to_value(&doc.spreads).unwrap(), serde_json::to_value(&before.spreads).unwrap());
        st.check().unwrap();
        let bytes = designcraft_format::save(doc).unwrap();
        let reopened = designcraft_format::load(&bytes).unwrap();
        assert_eq!(reopened.story(StoryId(sid)).unwrap().chars, old.chars);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap(), old);
    }
    #[test]
    fn inserted_and_replaced_unicode_inherit_local_format() {
        let (mut s, sid) = fixture("甲错乙");
        s.execute("text.select", &json!({"story":sid,"anchor":3,"focus":6})).unwrap();
        s.execute("type.char", &json!({"attrs":{"size":32}})).unwrap();
        let fmt = s.doc().unwrap().doc.story(StoryId(sid)).unwrap().format_after(3).clone();
        let id = suggestion(&mut s, sid, 0, "甲错乙", "甲正确😀乙");
        s.execute("ai.suggestion.accept", &json!({"id":id})).unwrap();
        let st = s.doc().unwrap().doc.story(StoryId(sid)).unwrap();
        assert_eq!(st.text, "甲正确😀乙");
        for pos in [3, 6, 9] {
            assert_eq!(st.format_after(pos), &fmt);
        }
        assert_ne!(st.format_after(13), &fmt);
        st.check().unwrap();
    }
    #[test]
    fn formatting_outside_trimmed_change_survives() {
        let (mut s, sid) = fixture("abc def");
        s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":3})).unwrap();
        s.execute("type.char", &json!({"attrs":{"size":32}})).unwrap();
        let before = s.doc().unwrap().doc.story(StoryId(sid)).unwrap().char_format_at(0).clone();
        let id = suggestion(&mut s, sid, 0, "abc def", "abc xyz");
        s.execute("ai.suggestion.accept", &json!({"id":id})).unwrap();
        assert_eq!(&before, s.doc().unwrap().doc.story(StoryId(sid)).unwrap().char_format_at(0));
    }
}
