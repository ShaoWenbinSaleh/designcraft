//! Type › Find/Replace Font: the fonts a document uses (missing ones flagged) and replacing one
//! font with another everywhere — local formatting and style definitions.

use std::collections::BTreeMap;
use std::sync::Arc;

use designcraft_doc::{CharAttrs, Document, Story};
use designcraft_fonts::FontSource;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "font.list", "Fonts in Document", [], None, "{} → [{family, style, characters, missing, styleMissing, source: bundled|installed|document|added (null when missing)}] (missing first)", has_doc, |s, _| {
            Ok(Value::Array(list(&s.doc()?.doc)))
        }),
        cmd!(
            "font.replace",
            "Find/Replace Font…",
            ["Type"],
            None,
            "{family, style?, toFamily, toStyle?, redefineStyles?: true} — replaces the font in text and (by default) in paragraph and character styles",
            has_doc,
            replace
        ),
    ]
}

/// Every story's text, including table cells and footnotes.
fn for_each_story(d: &Document, f: &mut dyn FnMut(&Story)) {
    let mut pending: Vec<&Story> = d.stories.values().map(AsRef::as_ref).collect();
    while let Some(st) = pending.pop() {
        f(st);
        pending.extend(st.tables.values().flat_map(|t| t.cells.iter().map(|c| &c.text)));
        pending.extend(st.notes.iter().chain(&st.endnotes).map(|n| &n.text));
    }
}

/// Effective document font use shared by the command layer and preflight.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UsedFont {
    pub family: String,
    pub style: String,
    pub characters: usize,
    pub missing: bool,
    pub style_missing: bool,
    pub source: Option<&'static str>,
}

fn list(d: &Document) -> Vec<Value> {
    used_fonts(d).into_iter().map(|font| json!(font)).collect()
}

pub(super) fn used_fonts(d: &Document) -> Vec<UsedFont> {
    let db = designcraft_fonts::FontDb::global().scoped(d.font_scope);
    let mut used: BTreeMap<(String, String), usize> = BTreeMap::new();
    for_each_story(d, &mut |st| {
        let ranges = st.para_ranges();
        for (pi, r) in ranges.iter().enumerate() {
            let (_, base) = d.styles.resolve_para(&st.paras[pi]);
            for (rr, fmt) in st.runs() {
                let n = st.text[rr.start.max(r.start)..rr.end.min(r.end).max(rr.start.max(r.start))].chars().count();
                if n == 0 && !(r.is_empty() && rr.contains(&r.start)) {
                    continue;
                }
                let p = d.styles.resolve_char(&base, fmt);
                if let Some(f) = d.styles.composite_font(&p.font_family) {
                    let text = st.text.get(rr.start.max(r.start)..rr.end.min(r.end).max(rr.start.max(r.start))).unwrap_or("");
                    for c in text.chars() {
                        if let Some(e) = f.entry(c) {
                            *used.entry((e.family.clone(), e.style.clone())).or_default() += 1;
                        }
                    }
                } else {
                    *used.entry((p.font_family, p.font_style)).or_default() += n;
                }
            }
        }
    });
    let mut out: Vec<UsedFont> = used
        .into_iter()
        .map(|((family, style), n)| {
            let missing = !db.has_family(&family);
            let style_missing = !missing && !db.styles(&family).iter().any(|s| s.eq_ignore_ascii_case(&style));
            let source = (!missing).then(|| match db.face(&family, &style).source {
                FontSource::Bundled => "bundled",
                FontSource::Installed(_) => "installed",
                FontSource::Document(_) => "document",
                FontSource::Memory => "added",
            });
            UsedFont { family, style, characters: n, missing, style_missing, source }
        })
        .collect();
    out.sort_by_key(|font| !(font.missing || font.style_missing));
    out
}

/// Replace the font in a sparse attribute set (only where the family is set); true when it changed.
fn swap(a: &mut CharAttrs, family: &str, style: Option<&str>, to_family: &str, to_style: Option<&str>) -> bool {
    let fam_ok = a.font_family.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(family));
    let style_ok = style.is_none_or(|s| a.font_style.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(s)));
    if !fam_ok || !style_ok {
        return false;
    }
    a.font_family = Some(to_family.to_string());
    if let Some(ts) = to_style {
        a.font_style = Some(ts.to_string());
    }
    true
}

fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let family = str_param(p, "family").ok_or_else(|| bad("font.replace", "missing family"))?.to_string();
    let style = str_param(p, "style").map(str::to_string);
    let to_family = str_param(p, "toFamily").ok_or_else(|| bad("font.replace", "missing toFamily"))?.to_string();
    let to_style = str_param(p, "toStyle").map(str::to_string);
    let styles_too = p.get("redefineStyles").and_then(Value::as_bool).unwrap_or(true);
    s.edit(|d, _| {
        let mut changed = 0usize;
        if styles_too {
            let st = d.styles_mut();
            for ps in &mut st.paragraph {
                changed += swap(&mut ps.chars, &family, style.as_deref(), &to_family, to_style.as_deref()) as usize;
            }
            for cs in &mut st.character {
                changed += swap(&mut cs.chars, &family, style.as_deref(), &to_family, to_style.as_deref()) as usize;
            }
            for f in &mut st.composite_fonts {
                for e in &mut f.entries {
                    if e.family.eq_ignore_ascii_case(&family) && style.as_ref().is_none_or(|s| e.style.eq_ignore_ascii_case(s)) {
                        e.family.clone_from(&to_family);
                        if let Some(s) = &to_style {
                            e.style.clone_from(s);
                        }
                        changed += 1;
                    }
                }
            }
        }
        // Local formatting in every story (cells and footnotes too).
        let fix = |st: &mut Story, changed: &mut usize| {
            let mut any = false;
            for r in &mut st.chars {
                if swap(&mut r.format.over, &family, style.as_deref(), &to_family, to_style.as_deref()) {
                    any = true;
                    *changed += 1;
                }
            }
            if any {
                st.rev += 1;
            }
        };
        let ids: Vec<_> = d.stories.keys().copied().collect();
        for sid in ids {
            let Some(st) = d.story_mut(sid) else { continue };
            fix(st, &mut changed);
            for t in st.tables.values_mut() {
                for c in &mut Arc::make_mut(t).cells {
                    fix(&mut c.text, &mut changed);
                }
            }
            for n in &mut st.notes {
                fix(&mut Arc::make_mut(n).text, &mut changed);
            }
        }
        Ok(json!({"changed": changed}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_and_replace_font() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Some text here."})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 4})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "Nonexistent Sans", "fontStyle": "Bold"})).unwrap();
        let l = s.execute("font.list", &json!({})).unwrap();
        assert_eq!(l[0]["family"], "Nonexistent Sans", "missing fonts first: {l}");
        assert_eq!(l[0]["missing"], true);
        assert_eq!(l[0]["characters"], 4);
        // Replace it with an available family.
        let avail = l.as_array().unwrap().iter().find(|f| f["missing"] == false).unwrap()["family"].as_str().unwrap().to_string();
        let r = s.execute("font.replace", &json!({"family": "Nonexistent Sans", "toFamily": avail, "toStyle": "Regular"})).unwrap();
        assert_eq!(r["changed"], 1);
        let l = s.execute("font.list", &json!({})).unwrap();
        assert!(l.as_array().unwrap().iter().all(|f| f["missing"] == false), "{l}");
        // Style definitions are redefined too.
        s.execute("style.paragraph.create", &json!({"name": "Odd", "chars": {"fontFamily": "Nonexistent Sans"}})).unwrap();
        let r = s.execute("font.replace", &json!({"family": "Nonexistent Sans", "toFamily": avail})).unwrap();
        assert_eq!(r["changed"], 1);
        assert_eq!(s.doc().unwrap().doc.styles.para("Odd").unwrap().chars.font_family.as_deref(), Some(avail.as_str()));
    }
}

#[cfg(test)]
mod composite_tests {
    use super::*;
    use designcraft_doc::{
        ParaFormat,
        cjk::{CompositeFont, CompositeFontEntry},
    };

    #[test]
    fn inventory_resolves_composites_in_nested_cell_notes() {
        use designcraft_doc::{StoryId, Table, build::NewDocument};
        let mut d = Document::new(&NewDocument::default());
        d.styles_mut().composite_fonts.push(CompositeFont {
            name: "Mixed".into(),
            entries: vec![
                CompositeFontEntry { family: "Source Serif 4".into(), ..Default::default() },
                CompositeFontEntry { characters: "1".into(), family: "Source Sans 3".into(), ..Default::default() },
            ],
        });
        let mut story = Story::new(StoryId(d.alloc()));
        let mut table = Table::new(1, 1, 1, 0, 0, 100.0);
        let cell = &mut table.cells[0].text;
        cell.insert_note(0, "1", ParaFormat::default());
        Arc::make_mut(&mut cell.notes[0]).text.format_chars(0..1, |f| f.over.font_family = Some("Mixed".into()));
        story.insert_table(0, table);
        d.stories.insert(story.id, Arc::new(story));
        let fonts = list(&d);
        assert!(fonts.iter().any(|f| f["family"] == "Source Sans 3" && f["characters"] == 1));
        assert!(!fonts.iter().any(|f| f["family"] == "Mixed"));
    }

    #[test]
    fn preflight_and_font_list_follow_used_composite_entries_in_notes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [20,20,400,600], "content":"text", "text":"Body"})).unwrap();
        let d = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        d.styles_mut().composite_fonts.push(CompositeFont {
            name: "Mixed".into(),
            entries: vec![
                CompositeFontEntry { family: "Source Serif 4".into(), ..Default::default() },
                CompositeFontEntry { characters: "1".into(), family: "Missing Note Digits".into(), ..Default::default() },
                CompositeFontEntry { characters: "2".into(), family: "Unused Missing Digits".into(), ..Default::default() },
            ],
        });
        let sid = *d.stories.keys().next().unwrap();
        let st = d.story_mut(sid).unwrap();
        st.insert_note(4, "A1", ParaFormat::default());
        Arc::make_mut(&mut st.notes[0]).text.format_chars(0..2, |f| f.over.font_family = Some("CompositeFont/Mixed".into()));
        let list = s.execute("font.list", &json!({})).unwrap();
        assert!(list.as_array().unwrap().iter().any(|f| f["family"] == "Missing Note Digits" && f["characters"] == 1));
        assert!(!list.to_string().contains("Unused Missing Digits"));
        let issues = s.execute("preflight.run", &json!({})).unwrap();
        let fonts: Vec<_> = issues["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "missingFont").collect();
        assert_eq!(fonts.len(), 1, "{issues}");
        assert!(fonts[0]["message"].as_str().unwrap().contains("Missing Note Digits"));
        s.execute("font.replace", &json!({"family":"Missing Note Digits", "toFamily":"Source Sans 3", "toStyle":"Regular"})).unwrap();
        let issues = s.execute("preflight.run", &json!({})).unwrap();
        assert!(issues["issues"].as_array().unwrap().iter().all(|i| i["kind"] != "missingFont"));
        assert_eq!(s.doc().unwrap().doc.styles.composite_fonts[0].entries[1].characters, "1");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.composite_fonts[0].entries[1].family, "Missing Note Digits");
    }
}
