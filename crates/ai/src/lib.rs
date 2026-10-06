//! AI protocol and review utilities, independent of the engine and UI.
#![forbid(unsafe_code)]
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
use designcraft_doc::ai::AiDocument;

/// All model offsets count Unicode scalar values, never UTF-16 units or UTF-8 bytes.
pub fn byte_offset(text: &str, character: usize) -> Option<usize> {
    text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).nth(character)
}
pub fn protected(text: &str) -> bool {
    text.chars().any(|c| ('\u{e000}'..='\u{f8ff}').contains(&c) || c == '\u{fffc}')
}
/// Paragraph-first batching with bounded long paragraphs. Ranges are byte offsets.
pub fn batches(text: &str, limit: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = vec![];
    let mut start = 0;
    while start < text.len() {
        let rest = &text[start..];
        let hard = byte_offset(rest, limit.max(1)).unwrap_or(rest.len());
        let end = if hard < rest.len() { rest[..hard].rfind('\n').map_or(hard, |n| n + 1) } else { hard };
        out.push(start..start + end);
        start += end;
    }
    out
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}
pub fn report(title: &str, data: &AiDocument) -> String {
    let mut html = format!(
        "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><title>{}</title><style>body{{font:16px system-ui;max-width:1100px;margin:40px auto;padding:20px}}article{{border-top:1px solid #aaa;padding:20px 0}}pre{{white-space:pre-wrap;overflow-wrap:anywhere}}del{{background:#ffe8e8}}ins{{background:#e3f6e5}}small{{color:#555}}</style><h1>{} · 校对报告</h1>",
        escape(title),
        escape(title)
    );
    for s in &data.suggestions {
        html.push_str(&format!("<article><h2>#{0} · {1}</h2><small>文章 {2} · 页码 {3} · 原始 UTF-8 位置 {4}–{5}</small><h3>原文上下文</h3><pre>{6}</pre><h3>修改</h3><pre><del>{7}</del> → <ins>{8}</ins></pre><p>{9}</p></article>", s.id, s.status.label(), s.story.0, escape(s.page_label.as_deref().unwrap_or("未知/溢出")), s.source_start, s.source_end, escape(&s.context), escape(&s.original), escape(&s.replacement), escape(&s.reason)));
    }
    html.push_str("</html>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_long_paragraph_batches() {
        let text = "😀甲乙\n丙丁戊己庚";
        let parts = batches(text, 3);
        assert_eq!(parts.iter().map(|r| &text[r.clone()]).collect::<String>(), text);
        assert!(parts.iter().all(|r| text[r.clone()].chars().count() <= 3));
        assert_eq!(byte_offset("😀中e\u{301}", 2), Some(7));
        assert_eq!(byte_offset("😀", 2), None);
    }
    #[test]
    fn report_escapes_document_text() {
        let html = report("<script>evil</script>", &AiDocument::default());
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}
