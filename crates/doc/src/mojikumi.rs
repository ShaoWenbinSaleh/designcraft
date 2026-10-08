//! Unicode Mojikumi rules shared by composition and preflight.
//!
//! IDML class numbers follow the documented Mojikumi override interchange format;
//! they are not the similarly numbered JLReq classes. Spacing is measured in em.
use crate::{Styles, cjk::MojikumiAki};

#[derive(Clone, Copy, Debug, Default)]
pub struct Aki {
    pub min: f64,
    pub desired: f64,
    pub max: f64,
    pub priority: u8,
    /// Discrete endpoints rather than a continuously floating amount.
    pub discrete: bool,
}
impl Aki {
    fn fixed(value: f64) -> Self {
        Self { min: value, desired: value, max: value, ..Self::default() }
    }
}

/// Adobe's public 16-preset enumeration. Japanese preset geometry is described
/// in the Adobe Mojikumi guide, sections 3.1–3.14 (see docs/mojikumi.md).
#[derive(Clone, Copy, Debug)]
struct Preset(u8);
impl Preset {
    fn parse(name: &str) -> Option<Self> {
        let name = name.trim_start_matches("$ID/");
        let names = [
            "LineEndAllOneHalfEmEnum",
            "OneEmIndentLineEndUkeOneHalfEmEnum",
            "OneOrOneHalfEmIndentLineEndUkeOneHalfEmEnum",
            "OneOrOneHalfEmIndentLineEndAllOneEmEnum",
            "OneEmIndentLineEndAllOneEmEnum",
            "OneEmIndentLineEndAllNoFloatEnum",
            "OneEmIndentLineEndUkeNoFloatEnum",
            "OneOrOneHalfEmIndentLineEndUkeNoFloatEnum",
            "OneEmIndentLineEndAllOneHalfEmEnum",
            "LineEndAllOneEmEnum",
            "LineEndUkeNoFloatEnum",
            "OneOrOneHalfEmIndentLineEndPeriodOneEmEnum",
            "OneEmIndentLineEndPeriodOneEmEnum",
            "LineEndPeriodOneEmEnum",
            "TradChineseDefault",
            "SimpChineseDefault",
        ];
        names.iter().position(|n| *n == name).map(|i| Self(i as u8 + 1)).or_else(|| {
            let n = name.strip_prefix("kMojikumiDefaultName")?.parse::<u8>().ok()?;
            (1..=16).contains(&n).then_some(Self(n))
        })
    }
    fn leading(self, c: i16) -> f64 {
        if self.0 == 15 && matches!(c, 6 | 21 | 30 | 31) {
            0.25
        } else if self.0 == 16 && c == 32 {
            0.0
        } else {
            leading(c)
        }
    }
    fn trailing(self, c: i16) -> f64 {
        if self.0 == 15 && matches!(c, 6 | 21 | 30 | 31) {
            0.25
        } else if self.0 == 16 && c == 32 {
            0.5
        } else {
            trailing(c)
        }
    }
    fn boundary(self, left: i16, right: i16) -> Option<Aki> {
        let n = self.0;
        let open = |c| matches!(c, 1 | 26 | 27);
        if left == 23 {
            if n >= 15 {
                return Some(Aki::fixed(self.leading(right)));
            }
            let indent = if matches!(n, 1 | 10 | 11 | 14) { 0.0 } else { 1.0 };
            let opening = match n {
                2 | 5 | 7 | 13 => 1.5,
                3 | 8 | 10 => 0.5,
                4 | 6 | 9 | 12 => 1.0,
                _ => 0.0,
            };
            return Some(Aki::fixed(if open(right) { opening } else { indent + self.leading(right) }));
        }
        if left == 22 {
            return Some(Aki::fixed(if n >= 15 || matches!(n, 4 | 5 | 10) { self.leading(right) } else { 0.0 }));
        }
        if right == 22 {
            let width = self.trailing(left);
            let full = n >= 15 || matches!(n, 4 | 5 | 10) || (matches!(n, 12..=14) && matches!(left, 6 | 31));
            let discrete = n == 6 || (matches!(n, 7 | 8 | 11) && matches!(left, 2 | 6 | 21 | 28..=31));
            return Some(if full {
                Aki::fixed(width)
            } else if discrete {
                Aki { min: 0.0, desired: width, max: width, priority: 1, discrete: true }
            } else {
                Aki::fixed(0.0)
            });
        }
        None
    }
}

/// Resolved table; custom rows override the base preset in document order.
pub struct Rules<'a> {
    preset: Preset,
    rows: &'a [MojikumiAki],
}
impl<'a> Rules<'a> {
    pub fn resolve(styles: &'a Styles, name: &str) -> Result<Option<Self>, String> {
        if matches!(name, "" | "Nothing" | "None") {
            return Ok(None);
        }
        let key = name.strip_prefix("MojikumiTable/").unwrap_or(name);
        let table = styles.mojikumi_tables.iter().find(|t| t.name == key);
        let base = table.map_or(key, |t| if t.based_on.is_empty() { t.name.as_str() } else { t.based_on.as_str() });
        let preset = Preset::parse(base).ok_or_else(|| format!("unknown base preset `{base}`"))?;
        let rows = table.map_or(&[][..], |t| t.overrides.as_slice());
        for r in rows {
            if !valid_class(r.target_class) || !valid_class(r.side_class) {
                return Err(format!("unsupported character classes {}/{}", r.target_class, r.side_class));
            }
            if !valid_range(r) {
                return Err("invalid spacing range or priority".into());
            }
        }
        Ok(Some(Self { preset, rows }))
    }

    /// Blank body portions for the preset's regional punctuation convention.
    pub fn leading(&self, class: i16) -> f64 {
        self.preset.leading(class)
    }
    pub fn trailing(&self, class: i16) -> f64 {
        self.preset.trailing(class)
    }

    pub fn pair(&self, left: i16, right: i16) -> Aki {
        if let Some(row) = self
            .rows
            .iter()
            .rev()
            .find(|r| if r.after { r.target_class == left && r.side_class == right } else { r.target_class == right && r.side_class == left })
        {
            return Aki {
                min: row.minimum,
                desired: row.desired,
                max: row.maximum,
                priority: if row.priority == 0 { 10 } else { row.priority as u8 },
                discrete: row.does_not_float,
            };
        }
        if let Some(aki) = self.preset.boundary(left, right) {
            return aki;
        }
        // Adjacent opening brackets and adjacent closing punctuation share a
        // half-body, rather than restoring both glyphs to their full widths.
        if (matches!(left, 1 | 26 | 27) && matches!(right, 1 | 26 | 27))
            || (matches!(left, 2 | 6 | 21 | 28..=31) && matches!(right, 2 | 6 | 21 | 28..=31))
        {
            return Aki::fixed(0.0);
        }
        let punctuation = self.trailing(left).max(self.leading(right));
        if punctuation > 0.0 {
            return Aki { min: 0.0, desired: punctuation, max: punctuation, priority: 1, discrete: false };
        }
        let roman = |c| matches!(c, 18 | 25);
        let ideograph = |c| matches!(c, 3 | 7..=9 | 11 | 12 | 24 | 33);
        if (roman(left) && ideograph(right)) || (ideograph(left) && roman(right)) {
            return Aki { min: 0.0, desired: 0.25, max: 0.5, priority: 3, discrete: false };
        }
        if ideograph(left) && ideograph(right) {
            return Aki { min: 0.0, desired: 0.0, max: 0.25, priority: 10, discrete: false };
        }
        Aki::default()
    }
}

/// Structural validation shared by native document loading and interchange.
/// Unknown integer classes remain preservable and are diagnosed by the resolver.
pub fn valid_range(r: &MojikumiAki) -> bool {
    [r.minimum, r.desired, r.maximum].iter().all(|v| v.is_finite() && (-1.0..=100.0).contains(v))
        && r.minimum <= r.desired
        && r.desired <= r.maximum
        && (0..=9).contains(&r.priority)
}

pub fn valid_class(c: i16) -> bool {
    matches!(c, 1..=12 | 18 | 21..=33)
}
/// Natural half-body blank before a full-width punctuation character.
pub fn leading(c: i16) -> f64 {
    match c {
        1 | 26 | 27 => 0.5,
        4 | 5 | 32 => 0.25,
        _ => 0.0,
    }
}
pub fn trailing(c: i16) -> f64 {
    match c {
        2 | 6 | 21 | 28..=31 => 0.5,
        4 | 5 | 32 => 0.25,
        _ => 0.0,
    }
}

/// Unicode classification, independent of font availability and localized names.
pub fn class(c: char) -> i16 {
    match c {
        '「' | '『' | '｢' => 26,
        '（' => 27,
        '〈' | '《' | '【' | '〔' | '〖' | '〘' | '［' | '｛' | '‘' | '“' => 1,
        '」' | '』' | '｣' => 28,
        '）' => 29,
        '〉' | '》' | '】' | '〕' | '〗' | '〙' | '］' | '｝' | '’' | '”' => 2,
        '。' | '｡' => 6,
        '、' | '､' => 21,
        '，' => 30,
        '．' => 31,
        '：' | '；' => 32,
        '・' | '·' => 5,
        '！' | '？' => 4,
        '…' | '‥' | '—' | '―' => 7,
        '￥' | '＄' | '£' | '¥' => 8,
        '％' | '‰' | '℃' | '°' => 9,
        '\u{3000}' => 10,
        'ゕ'
        | 'ゖ'
        | 'ヵ'
        | 'ヶ'
        | '\u{31f0}'..='\u{31ff}'
        | 'ぁ'
        | 'ぃ'
        | 'ぅ'
        | 'ぇ'
        | 'ぉ'
        | 'っ'
        | 'ゃ'
        | 'ゅ'
        | 'ょ'
        | 'ゎ'
        | 'ァ'
        | 'ィ'
        | 'ゥ'
        | 'ェ'
        | 'ォ'
        | 'ッ'
        | 'ャ'
        | 'ュ'
        | 'ョ'
        | 'ヮ'
        | 'ー'
        | '々'
        | 'ゝ'
        | 'ゞ'
        | 'ヽ'
        | 'ヾ' => 3,
        '\u{3040}'..='\u{309f}' => 11,
        '\u{30a0}'..='\u{30ff}' | '\u{ff66}'..='\u{ff9d}' => 33,
        '０'..='９' => 24,
        '0'..='9' => 25,
        '\u{2e80}'..='\u{a4cf}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}' | '\u{ff01}'..='\u{ff60}' | '\u{20000}'..='\u{3347f}' => 12,
        _ => 18,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cjk::MojikumiTable;

    #[test]
    fn regional_punctuation_retains_the_correct_side_bearings() {
        let styles = Styles::default();
        let traditional = Rules::resolve(&styles, "TradChineseDefault").unwrap().unwrap();
        let simplified = Rules::resolve(&styles, "SimpChineseDefault").unwrap().unwrap();
        assert_eq!(traditional.leading(class('。')), 0.25);
        assert_eq!(traditional.trailing(class('。')), 0.25);
        assert_eq!(simplified.leading(class('。')), 0.0);
        assert_eq!(simplified.trailing(class('。')), 0.5);
        assert_eq!(traditional.pair(12, class('。')).desired, 0.25);
        assert_eq!(simplified.pair(12, class('。')).desired, 0.0);
        assert_eq!(simplified.leading(class('：')), 0.0);
        assert_eq!(simplified.trailing(class('：')), 0.5);
    }

    #[test]
    fn all_public_presets_have_distinct_documented_edges() {
        let styles = Styles::default();
        let openings = [0.0, 1.5, 0.5, 1.0, 1.5, 1.0, 1.5, 0.5, 1.0, 0.5, 0.0, 1.0, 1.5, 0.0];
        for (i, expected) in openings.into_iter().enumerate() {
            let rules = Rules::resolve(&styles, &format!("$ID/kMojikumiDefaultName{}", i + 1)).unwrap().unwrap();
            assert_eq!(rules.pair(23, 26).desired, expected, "preset {}", i + 1);
            assert_eq!(rules.pair(23, 12).desired, if matches!(i + 1, 1 | 10 | 11 | 14) { 0.0 } else { 1.0 });
        }
        let discrete = Rules::resolve(&styles, "OneEmIndentLineEndUkeNoFloatEnum").unwrap().unwrap();
        assert!(discrete.pair(6, 22).discrete);
        assert!(!discrete.pair(5, 22).discrete);
        let period = Rules::resolve(&styles, "LineEndPeriodOneEmEnum").unwrap().unwrap();
        assert_eq!(period.pair(6, 22).desired, 0.5);
        assert_eq!(period.pair(21, 22).desired, 0.0);
        for name in ["$ID/kMojikumiDefaultName15", "$ID/kMojikumiDefaultName16"] {
            assert!(Rules::resolve(&styles, name).unwrap().is_some());
        }
        assert_eq!(period.pair(26, 26).desired, 0.0);
        assert_eq!(period.pair(28, 6).desired, 0.0);
        assert_eq!(class('ㇰ'), 3);
        assert_eq!(class('ｶ'), 33);
    }

    #[test]
    fn overrides_are_directional_and_unknown_tables_are_not_silently_applied() {
        let mut styles = Styles::default();
        styles.mojikumi_tables.push(MojikumiTable {
            name: "Pair".into(),
            based_on: "SimpChineseDefault".into(),
            overrides: vec![MojikumiAki {
                target_class: 12,
                side_class: 18,
                after: false,
                minimum: 0.1,
                desired: 0.2,
                maximum: 0.3,
                priority: 2,
                does_not_float: false,
            }],
        });
        let rule = Rules::resolve(&styles, "MojikumiTable/Pair").unwrap().unwrap();
        assert_eq!(rule.pair(18, 12).desired, 0.2);
        assert_eq!(rule.pair(12, 18).desired, 0.25);
        assert!(Rules::resolve(&styles, "Unknown").is_err());
        assert!(Rules::resolve(&styles, "Nothing").unwrap().is_none());
        styles.mojikumi_tables[0].overrides[0].minimum = 0.4;
        assert!(Rules::resolve(&styles, "MojikumiTable/Pair").is_err());
    }

    #[test]
    fn chinese_and_half_em_boundaries_differ_and_classes_cover_punctuation() {
        let styles = Styles::default();
        let cn = Rules::resolve(&styles, "SimpChineseDefault").unwrap().unwrap();
        let jp = Rules::resolve(&styles, "LineEndAllOneHalfEmEnum").unwrap().unwrap();
        assert_eq!(cn.pair(22, class('（')).desired, 0.5);
        assert_eq!(jp.pair(22, class('（')).desired, 0.0);
        assert_eq!(cn.pair(class('。'), 22).desired, 0.5);
        assert_eq!(jp.pair(class('。'), 22).desired, 0.0);
        assert_ne!(class('１'), class('1'));
        assert_ne!(class('，'), class('、'));
        assert_ne!(class('「'), class('（'));
    }
}
