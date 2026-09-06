//! 谱面解析：文本 → 音符事件时间线。
//!
//! 文法见 PRD §4.2。错误一律带行:列定位，全量校验通过后才产出 `Score`。

use crate::config::{parser_ranges, Config};
use crate::keymap::Keymap;
use crate::model::{NoteEvent, Score, ScoreParams, Waveform};
use std::collections::HashSet;

/// 八度移位限定区间：C2–C7（MIDI 36–96），仅对 `^`/`_` 移位结果生效。
pub const PITCH_MIN: i32 = 36;
pub const PITCH_MAX: i32 = 96;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// 谱面为空（无音符），或仅含注释/休止。
    EmptyScore,
    /// 无法识别的字符。
    UnknownCharacter(char),
    /// `.` 未紧跟音符。
    LoneDot,
    /// 指令出现在音符之后。
    DirectiveAfterNotes(String),
    /// 指令重复出现。
    DuplicateDirective(String),
    /// 未知指令。
    UnknownDirective(String),
    /// 指令取值无法解析为数字。
    DirectiveValueInvalid(String, String),
    /// 指令取值越界。
    DirectiveValueOutOfRange { name: String, value: String, range: String },
    /// 音符经八度移位后越出 C2–C7。
    PitchOutOfRange { c: char, midi: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based 行号。
    pub line: usize,
    /// 1-based 列号（按字符计）。
    pub col: usize,
    pub kind: ParseErrorKind,
}

impl ParseError {
    /// 一句话原因（供诊断模块完整呈现）。
    pub fn message(&self) -> String {
        match &self.kind {
            ParseErrorKind::EmptyScore => "谱面为空，没有可发声的音符".to_string(),
            ParseErrorKind::UnknownCharacter(c) => format!(
                "字符 '{}' 不是音符字符，且不属于保留语法（^ _ = . # @）",
                c
            ),
            ParseErrorKind::LoneDot => "字符 '.' 用于延长音符时长，必须紧跟音符字符".to_string(),
            ParseErrorKind::DirectiveAfterNotes(name) => {
                format!("指令 @{} 出现在音符之后，指令只允许位于文件头（首个音符之前）", name)
            }
            ParseErrorKind::DuplicateDirective(name) => format!("指令 @{} 重复出现", name),
            ParseErrorKind::UnknownDirective(name) => format!("未知指令 @{}", name),
            ParseErrorKind::DirectiveValueInvalid(name, value) => {
                format!("指令 @{} 的值 '{}' 无法解析为合法数字", name, value)
            }
            ParseErrorKind::DirectiveValueOutOfRange { name, value, range } => {
                format!("指令 @{} 的值 {} 越界，合法范围 {}", name, value, range)
            }
            ParseErrorKind::PitchOutOfRange { c, midi } => format!(
                "字符 '{}' 经八度移位后音高 {} 越出 C2–C7 范围",
                c, midi
            ),
        }
    }

    /// 针对原因的修改建议。
    pub fn suggestion(&self) -> String {
        match &self.kind {
            ParseErrorKind::EmptyScore => {
                "在谱面中写入音符字符，例如 'aaa 123'；仅注释不算有效乐谱".to_string()
            }
            ParseErrorKind::UnknownCharacter(_) => {
                "将其改为音符字符，或删除；可运行 mm keymap show 查看可选字符".to_string()
            }
            ParseErrorKind::LoneDot => "将 '.' 紧跟上一个音符字符写入，例如 'a.'".to_string(),
            ParseErrorKind::DirectiveAfterNotes(name) => {
                format!("将 @{} 移动到文件顶部（首个音符字符之前）", name)
            }
            ParseErrorKind::DuplicateDirective(_) => {
                "删除重复指令，每类指令最多出现一次".to_string()
            }
            ParseErrorKind::UnknownDirective(_) => {
                "改用受支持的指令 @BPM / @VOL / @WAVE".to_string()
            }
            ParseErrorKind::DirectiveValueInvalid(_, _) => {
                "检查取值是否为合法数字（如 120、0.8）".to_string()
            }
            ParseErrorKind::DirectiveValueOutOfRange { .. } => {
                "将取值调整到合法范围内".to_string()
            }
            ParseErrorKind::PitchOutOfRange { .. } => {
                "减少 ^/_ 的使用次数，或调整起始音高".to_string()
            }
        }
    }
}

fn err(line: usize, col: usize, kind: ParseErrorKind) -> ParseError {
    ParseError { line, col, kind }
}

/// 解析谱面文本。`keymap` 决定字符→音高，`cfg` 提供 BPM/音量/波形默认值（可被指令覆盖）。
pub fn parse(text: &str, keymap: &Keymap, cfg: &Config) -> Result<Score, ParseError> {
    let mut bpm = cfg.bpm;
    let mut volume = cfg.volume;
    let mut waveform = Waveform::from_str(&cfg.wave).unwrap_or_default();
    let mut notes: Vec<NoteEvent> = Vec::new();
    let mut cursor: f64 = 0.0;
    let mut saw_note = false;
    let mut seen_directives: HashSet<String> = HashSet::new();
    let mut last_pos = (1usize, 1usize); // 记录文件末尾位置，供空谱面报错定位

    for (li, raw_line) in text.lines().enumerate() {
        let line = li + 1;
        // 八度记号按行作用：行首重置
        let mut octave_shift: i64 = 0;

        let chars: Vec<(usize, char)> = raw_line.chars().enumerate().collect();
        let mut i = 0usize;
        while i < chars.len() {
            let (ci, c) = chars[i];
            let col = ci + 1;
            last_pos = (line, col);
            match c {
                '#' => break, // 注释到行尾
                '@' => {
                    let rest: String = chars[i + 1..].iter().map(|&(_, x)| x).collect();
                    let end = rest.find([' ', '\t']).unwrap_or(rest.len());
                    let token = &rest[..end];
                    // 音符之后的指令：直接报错（指令名去掉 =value 部分，统一小写）
                    if saw_note {
                        let name = token
                            .split('=')
                            .next()
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        return Err(err(line, col, ParseErrorKind::DirectiveAfterNotes(name)));
                    }
                    let Some((name, value)) = token.split_once('=') else {
                        return Err(err(line, col, ParseErrorKind::UnknownDirective(token.to_string())));
                    };
                    apply_directive(
                        &mut seen_directives,
                        name.trim(),
                        value.trim(),
                        &mut bpm,
                        &mut volume,
                        &mut waveform,
                        line,
                        col,
                    )?;
                    i = chars.len(); // 指令行剩余内容不再参与时值
                }
                c if c.is_whitespace() => {
                    cursor += 1.0; // 空格/制表符 = 一拍休止
                    i += 1;
                }
                '=' | '-' => {
                    cursor += 1.0; // 休止，时长一拍
                    i += 1;
                }
                '^' => {
                    octave_shift = octave_shift.saturating_add(1);
                    i += 1;
                }
                '_' => {
                    octave_shift = octave_shift.saturating_sub(1);
                    i += 1;
                }
                '.' => return Err(err(line, col, ParseErrorKind::LoneDot)),
                c if keymap.is_note_char(c) => {
                    saw_note = true;
                    // 连奏：连续相同字符合并为一个音符，小时值一拍
                    let mut k = 1usize;
                    while i + k < chars.len() && chars[i + k].1 == c {
                        k += 1;
                    }
                    // 半拍延长：紧跟的 '.'
                    let mut j = i + k;
                    let mut dots = 0usize;
                    while j < chars.len() && chars[j].1 == '.' {
                        dots += 1;
                        j += 1;
                    }
                    let dur = k as f64 + 0.5 * dots as f64;
                    let midi = keymap
                        .resolve(c, octave_shift)
                        .expect("已判定为音符字符");
                    if octave_shift != 0 && !(PITCH_MIN..=PITCH_MAX).contains(&midi) {
                        return Err(err(line, col, ParseErrorKind::PitchOutOfRange { c, midi }));
                    }
                    notes.push(NoteEvent::new(cursor, dur, midi as u8));
                    cursor += dur;
                    i = j;
                }
                c => return Err(err(line, col, ParseErrorKind::UnknownCharacter(c))),
            }
        }
    }

    if notes.is_empty() {
        return Err(err(last_pos.0, last_pos.1, ParseErrorKind::EmptyScore));
    }

    Ok(Score {
        notes,
        params: ScoreParams { bpm, volume, waveform },
    })
}

#[allow(clippy::too_many_arguments)]
fn apply_directive(
    seen: &mut HashSet<String>,
    name: &str,
    value: &str,
    bpm: &mut f64,
    volume: &mut f64,
    waveform: &mut Waveform,
    line: usize,
    col: usize,
) -> Result<(), ParseError> {
    let lname = name.to_ascii_lowercase();
    if !seen.insert(lname.clone()) {
        return Err(err(line, col, ParseErrorKind::DuplicateDirective(lname.clone())));
    }
    match lname.as_str() {
        "bpm" => {
            let v: f64 = value.parse().map_err(|_| {
                err(line, col, ParseErrorKind::DirectiveValueInvalid(lname.clone(), value.to_string()))
            })?;
            if !(parser_ranges::BPM_MIN..=parser_ranges::BPM_MAX).contains(&v) {
                return Err(err(
                    line,
                    col,
                    ParseErrorKind::DirectiveValueOutOfRange {
                        name: lname.clone(),
                        value: value.to_string(),
                        range: format!("{}-{}", parser_ranges::BPM_MIN, parser_ranges::BPM_MAX),
                    },
                ));
            }
            *bpm = v;
        }
        "vol" => {
            let v: f64 = value.parse().map_err(|_| {
                err(line, col, ParseErrorKind::DirectiveValueInvalid(lname.clone(), value.to_string()))
            })?;
            if !(parser_ranges::VOL_MIN..=parser_ranges::VOL_MAX).contains(&v) {
                return Err(err(
                    line,
                    col,
                    ParseErrorKind::DirectiveValueOutOfRange {
                        name: lname.clone(),
                        value: value.to_string(),
                        range: format!("{}-{}", parser_ranges::VOL_MIN, parser_ranges::VOL_MAX),
                    },
                ));
            }
            *volume = v;
        }
        "wave" => {
            let Some(w) = Waveform::from_str(value) else {
                return Err(err(
                    line,
                    col,
                    ParseErrorKind::DirectiveValueOutOfRange {
                        name: lname.clone(),
                        value: value.to_string(),
                        range: "sine | square | saw".to_string(),
                    },
                ));
            };
            *waveform = w;
        }
        other => return Err(err(line, col, ParseErrorKind::UnknownDirective(other.to_string()))),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_default(text: &str) -> Result<Score, ParseError> {
        let cfg = Config::default();
        let km = cfg.keymap().expect("默认配置合法");
        parse(text, &km, &cfg)
    }

    fn assert_kind(text: &str, kind: ParseErrorKind) {
        let e = parse_default(text).expect_err("应当解析失败");
        assert_eq!(e.kind, kind);
    }

    #[test]
    fn twinkle_parses_to_expected_notes() {
        // 连奏：连续相同字符合并为持续音（cc→C×2拍, jj→G×2拍 …）
        let score = parse_default("ccjjllj hhggeec\n").expect("解析成功");
        assert_eq!(score.notes.len(), 8);
        // C(2) G(2) A(2) G(1) | F(2) E(2) D(2) C(1)
        let mids: Vec<u8> = score.notes.iter().map(|n| n.midi).collect();
        assert_eq!(mids, vec![60, 67, 69, 67, 65, 64, 62, 60]);
        assert_eq!(score.notes[0].dur_beats, 2.0);
        // 前 6 拍（2+2+2），第 4 音起点 6；休止一拍后第 5 音起点 8
        assert_eq!(score.notes[3].start_beats, 6.0);
        assert_eq!(score.notes[4].start_beats, 8.0);
        assert_eq!(score.params.waveform, Waveform::Square);
    }

    #[test]
    fn directive_controls_params() {
        let score = parse_default("@BPM=150\n@VOL=0.5\n@WAVE=saw\nccc\n").expect("解析成功");
        assert_eq!(score.params.bpm, 150.0);
        assert_eq!(score.params.volume, 0.5);
        assert_eq!(score.params.waveform, Waveform::Saw);
    }

    #[test]
    fn bpm_out_of_range() {
        assert_kind("@BPM=500\ncc\n", ParseErrorKind::DirectiveValueOutOfRange {
            name: "bpm".to_string(),
            value: "500".to_string(),
            range: "20-300".to_string(),
        });
    }

    #[test]
    fn duplicate_directive_rejected() {
        assert_kind("@BPM=120\n@BPM=130\ncc\n", ParseErrorKind::DuplicateDirective("bpm".to_string()));
    }

    #[test]
    fn directive_after_note_rejected() {
        assert_kind("cc\n@BPM=120\n", ParseErrorKind::DirectiveAfterNotes("bpm".to_string()));
    }

    #[test]
    fn unknown_directive_rejected() {
        assert_kind("@FOO=1\ncc\n", ParseErrorKind::UnknownDirective("foo".to_string()));
    }

    #[test]
    fn empty_and_comment_only_are_errors() {
        assert_kind("", ParseErrorKind::EmptyScore);
        assert_kind("# 只有注释\n# 还是注释\n", ParseErrorKind::EmptyScore);
        assert_kind("   = - \n", ParseErrorKind::EmptyScore);
    }

    #[test]
    fn unknown_char_reports_positions() {
        let e = parse_default("abc[def").expect_err("应当解析失败");
        assert_eq!(e.line, 1);
        assert_eq!(e.col, 4);
        assert_eq!(e.kind, ParseErrorKind::UnknownCharacter('['));
    }

    #[test]
    fn lone_dot_rejected() {
        assert_kind(".abc\n", ParseErrorKind::LoneDot);
    }

    #[test]
    fn tie_extends_duration() {
        let score = parse_default("aaa bb\n").expect("解析成功");
        assert_eq!(score.notes[0].dur_beats, 3.0);
        assert_eq!(score.notes[1].dur_beats, 2.0);
        // 'aaa' 之后 ' ' 休止一拍，'bb' 起点 4
        assert_eq!(score.notes[1].start_beats, 4.0);
    }

    #[test]
    fn dot_extends_half_beat() {
        let score = parse_default("a. a..\n").expect("解析成功");
        assert_eq!(score.notes[0].dur_beats, 1.5);
        assert_eq!(score.notes[1].dur_beats, 2.0); // 2 拍 + 2×0.5
        // a(1.5) + 休止(1) = 2.5
        assert_eq!(score.notes[1].start_beats, 2.5);
    }

    #[test]
    fn octave_marker_scopes_to_line_end() {
        // 用不重复字符 c/d 避免连奏合并，便于核对每个音符
        let score = parse_default("^cd\n__cd\n").expect("解析成功");
        // 第 1 行 ^ 升一倍频程：C4→C5(72)，C#4→C#5(73)
        assert_eq!(score.notes[0].midi, 72);
        assert_eq!(score.notes[1].midi, 73);
        // 第 2 行重置后 __ 降两个八度：C4 60 - 24 = 36（C2 合法）
        assert_eq!(score.notes[2].midi, 36);
        assert_eq!(score.notes[3].midi, 37);
    }

    #[test]
    fn octave_shift_out_of_range() {
        // '___' 三次降八度：C4(60) - 36 = 24，低于 C2(36)
        assert_kind("___cc\n", ParseErrorKind::PitchOutOfRange { c: 'c', midi: 24 });
    }
}