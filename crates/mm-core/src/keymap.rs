//! 键位映射：字符排序表 × 音高表。

use crate::config::defaults::{DEFAULT_BASE_MIDI, DEFAULT_CHAR_ORDER, DEFAULT_PITCH_TABLE};
use std::collections::HashSet;

/// 键位映射错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    #[error("字符排序表为空")]
    EmptyCharOrder,
    #[error("字符排序表中存在重复字符 '{0}'")]
    DuplicateChar(char),
    #[error("字符 '{0}' 与保留语法字符冲突")]
    ReservedChar(char),
    #[error("字符排序表中包含空白字符")]
    WhitespaceInOrder,
    #[error("音高表为空")]
    EmptyPitchTable,
    #[error("起始音高 base_midi 越界（0–127）")]
    BaseMidiOutOfRange,
}

/// 保留给语法的控制字符，不参与音乐映射。注意：降八度记号为 `_`（小写 v 是音符字符）。
pub fn is_control_char(c: char) -> bool {
    matches!(c, '^' | '_' | '=' | '.' | '#' | '@') || c.is_whitespace()
}

/// 键位映射：字符序 → 音高。
///
/// 模型：`字符序[i] → base_midi + (i / 音高表长) * 12 + 音高表[i mod 音高表长]`，
/// 每越过一个音高表周期升高一个八度。`resolution` 的 `octaves` 参数代表 `^`/`v` 的累计升降。
#[derive(Debug, Clone)]
pub struct Keymap {
    order: Vec<char>,
    table: Vec<i8>,
    base_midi: u8,
}

impl Keymap {
    pub fn new(order: Vec<char>, table: Vec<i8>, base_midi: u8) -> Result<Self, KeymapError> {
        if order.is_empty() {
            return Err(KeymapError::EmptyCharOrder);
        }
        if table.is_empty() {
            return Err(KeymapError::EmptyPitchTable);
        }
        if base_midi > 127 {
            return Err(KeymapError::BaseMidiOutOfRange);
        }
        let mut seen = HashSet::new();
        for &c in &order {
            if c.is_whitespace() {
                return Err(KeymapError::WhitespaceInOrder);
            }
            if is_control_char(c) {
                return Err(KeymapError::ReservedChar(c));
            }
            if !seen.insert(c) {
                return Err(KeymapError::DuplicateChar(c));
            }
        }
        Ok(Self { order, table, base_midi })
    }

    /// 内置默认映射：62 个字符，12 半音全量音高表自 C3 起。
    pub fn default() -> Self {
        Self::new(
            DEFAULT_CHAR_ORDER.chars().collect(),
            DEFAULT_PITCH_TABLE.to_vec(),
            DEFAULT_BASE_MIDI,
        )
        .expect("默认映射必然合法")
    }

    /// 该字符是否为音符字符。
    pub fn is_note_char(&self, c: char) -> bool {
        self.order.contains(&c)
    }

    /// 字符在字符序中的索引；不存在返回 None。
    pub fn index_of(&self, c: char) -> Option<usize> {
        self.order.iter().position(|&x| x == c)
    }

    /// 解析字符 → MIDI 音高；`octaves` 为 `^`/`v` 的累计升降（每单位一个八度）。
    /// 返回 None 表示字符不在映射之内。
    pub fn resolve(&self, c: char, octaves: i64) -> Option<i32> {
        let idx = self.index_of(c)? as i64;
        let len = self.table.len() as i64;
        let oct = idx / len + octaves;
        let semi = idx % len;
        Some(self.base_midi as i32 + (oct * 12) as i32 + self.table[semi as usize] as i32)
    }

    pub fn base_midi(&self) -> u8 {
        self.base_midi
    }

    pub fn char_order(&self) -> &[char] {
        &self.order
    }

    pub fn pitch_table(&self) -> &[i8] {
        &self.table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resolution_matches_prd() {
        let km = Keymap::default();
        // PRD §4.3 示例：'0'→C3, 'c'→C4, 'h'→F4, 'j'→G4, 'l'→A4
        assert_eq!(km.resolve('0', 0), Some(48)); // C3
        assert_eq!(km.resolve('c', 0), Some(60)); // C4
        assert_eq!(km.resolve('h', 0), Some(65)); // F4
        assert_eq!(km.resolve('j', 0), Some(67)); // G4
        assert_eq!(km.resolve('l', 0), Some(69)); // A4
    }

    #[test]
    fn octave_shift_moves_by_octave() {
        let km = Keymap::default();
        assert_eq!(km.resolve('c', 1), Some(72)); // 60 + 12
        assert_eq!(km.resolve('c', -1), Some(48)); // 60 - 12
        assert_eq!(km.resolve('c', 2), Some(84));
    }

    #[test]
    fn non_note_char_is_none() {
        let km = Keymap::default();
        assert_eq!(km.resolve('@', 0), None);
        assert_eq!(km.is_note_char('['), false);
    }

    #[test]
    fn rejects_duplicate_and_control_chars() {
        let e = Keymap::new(vec!['a', 'b', 'a'], vec![0], 48);
        assert!(matches!(e, Err(KeymapError::DuplicateChar('a'))));
        let e = Keymap::new(vec!['a', '^'], vec![0], 48);
        assert!(matches!(e, Err(KeymapError::ReservedChar('^'))));
    }
}