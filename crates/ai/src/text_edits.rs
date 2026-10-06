//! Bounded Unicode diff, split at layout controls so their positions/styles survive edits.
#[derive(Debug)]
pub struct TextEdit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}
pub fn layout_control(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\t' | '\u{2028}' | '\u{2029}' | '\u{00a0}' | '\u{202f}' | '\u{00ad}' | '\u{200b}')
}
pub fn same_layout(a: &str, b: &str) -> bool {
    a.chars().filter(|c| layout_control(*c)).eq(b.chars().filter(|c| layout_control(*c)))
}
pub fn text_edits(a: &str, b: &str) -> Result<Vec<TextEdit>, String> {
    if !same_layout(a, b) {
        return Err("AI 仅修改文字内容，不能增删段落、制表符或换行等排版标记。".into());
    }
    let controls: Vec<_> = a.chars().filter(|c| layout_control(*c)).collect();
    let mut base = 0;
    let mut result = vec![];
    for (i, (a, b)) in a.split(layout_control).zip(b.split(layout_control)).enumerate() {
        diff(a, b, base, &mut result)?;
        base += a.len() + controls.get(i).map_or(0, |c| c.len_utf8());
    }
    Ok(result)
}
fn diff(a: &str, b: &str, base: usize, result: &mut Vec<TextEdit>) -> Result<(), String> {
    let prefix: usize = a.chars().zip(b.chars()).take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum();
    let a = &a[prefix..];
    let b = &b[prefix..];
    let suffix: usize = a.chars().rev().zip(b.chars().rev()).take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum();
    let a = &a[..a.len() - suffix];
    let b = &b[..b.len() - suffix];
    let base = base + prefix;
    if a.is_empty() && b.is_empty() {
        return Ok(());
    }
    if a.is_empty() || b.is_empty() {
        result.push(TextEdit { start: base, end: base + a.len(), replacement: b.into() });
        return Ok(());
    }
    let ac: Vec<_> = a.chars().collect();
    let bc: Vec<_> = b.chars().collect();
    let n = ac.len() as isize;
    let m = bc.len() as isize;
    // Equal-length corrections keep each character's existing position and format,
    // avoiding ambiguous delete/insert alignments in repeated Chinese text.
    if n == m {
        let mut pending = None;
        for ((ai, ac), (bi, bc)) in a.char_indices().zip(b.char_indices()).chain(std::iter::once(((a.len(), '\0'), (b.len(), '\0')))) {
            if ac == bc {
                if let Some((sa, sb)) = pending.take() {
                    result.push(TextEdit { start: base + sa, end: base + ai, replacement: b[sb..bi].into() });
                }
            } else {
                pending.get_or_insert((ai, bi));
            }
        }
        return Ok(());
    }
    let max = (n + m) as usize;
    let origin = max as isize + 1;
    let at = |k: isize| (origin + k) as usize;
    let mut v = vec![0isize; 2 * max + 3];
    let mut trace = Vec::new();
    let mut distance = None;
    for d in 0..=max {
        // Bound worst-case rewrite cost instead of flattening existing formatting.
        if (d + 1).saturating_mul(v.len()) > 4_000_000 {
            return Err("改写跨度过大，无法安全保留格式；请缩小选区或分开提出修改。".into());
        }
        for k in (-(d as isize)..=d as isize).step_by(2) {
            let mut x = if k == -(d as isize) || (k != d as isize && v[at(k - 1)] < v[at(k + 1)]) { v[at(k + 1)] } else { v[at(k - 1)] + 1 };
            let mut y = x - k;
            while x < n && y < m && ac[x as usize] == bc[y as usize] {
                x += 1;
                y += 1;
            }
            v[at(k)] = x;
            if x == n && y == m {
                distance = Some(d);
                break;
            }
        }
        trace.push(v.clone());
        if distance.is_some() {
            break;
        }
    }
    let mut x = n;
    let mut y = m;
    let mut steps = vec![]; // 0 equal, 1 delete, 2 insert
    for d in (1..=distance.ok_or("无法对齐修改文字")?).rev() {
        let prev = &trace[d - 1];
        let k = x - y;
        let pk = if k == -(d as isize) || (k != d as isize && prev[at(k - 1)] < prev[at(k + 1)]) { k + 1 } else { k - 1 };
        let px = prev[at(pk)];
        let py = px - pk;
        while x > px && y > py {
            steps.push(0);
            x -= 1;
            y -= 1;
        }
        if x == px {
            steps.push(2);
            y -= 1;
        } else {
            steps.push(1);
            x -= 1;
        }
    }
    while x > 0 && y > 0 {
        steps.push(0);
        x -= 1;
        y -= 1;
    }
    steps.reverse();
    let ab: Vec<_> = a.char_indices().map(|(i, _)| i).chain(std::iter::once(a.len())).collect();
    let bb: Vec<_> = b.char_indices().map(|(i, _)| i).chain(std::iter::once(b.len())).collect();
    let (mut ai, mut bi) = (0, 0);
    let mut pending = None;
    for step in steps.into_iter().chain(std::iter::once(0)) {
        if step == 0 {
            if let Some((sa, sb)) = pending.take() {
                result.push(TextEdit { start: base + ab[sa], end: base + ab[ai], replacement: b[bb[sb]..bb[bi]].into() });
            }
            ai += 1;
            bi += 1;
        } else {
            pending.get_or_insert((ai, bi));
            if step == 1 {
                ai += 1;
            } else {
                bi += 1;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diff_reconstructs_exhaustive_small_unicode_strings() {
        let mut words = vec![String::new()];
        let mut level = vec![String::new()];
        for _ in 0..4 {
            level = level.iter().flat_map(|s| ['甲', '😀', '乙'].map(|c| format!("{s}{c}"))).collect();
            words.extend(level.clone());
        }
        for a in &words {
            for b in &words {
                let mut actual = a.clone();
                for e in text_edits(a, b).unwrap().iter().rev() {
                    actual.replace_range(e.start..e.end, &e.replacement);
                }
                assert_eq!(&actual, b, "source: {a}");
            }
        }
    }
    #[test]
    fn separates_distant_edits_and_keeps_layout_controls() {
        let a = format!("错{}错\n甲\t乙", "正文".repeat(7000));
        let b = format!("正{}正\n乙\t甲", "正文".repeat(7000));
        let edits = text_edits(&a, &b).unwrap();
        assert_eq!(edits.len(), 4);
        assert!(edits.iter().all(|e| !a[e.start..e.end].contains(['\n', '\t'])));
        assert!(text_edits("甲\n乙", "甲乙").is_err());
        assert!(text_edits("甲\t乙", "甲 乙").is_err());
        assert!(text_edits("甲\u{2028}乙", "甲乙").is_err());
    }
}
