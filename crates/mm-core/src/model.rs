//! 时间线数据模型：谱面解析后的音符事件与谱面级参数。

/// 音符事件：时间线上的单个发声单元。
#[derive(Debug, Clone, PartialEq)]
pub struct NoteEvent {
    /// 起点（单位：拍，四分音符）。
    pub start_beats: f64,
    /// 时长（单位：拍）。
    pub dur_beats: f64,
    /// MIDI 音高（0–127）。
    pub midi: u8,
    /// 力度（0.0–1.0），本阶段恒定 1.0。
    pub velocity: f64,
}

impl NoteEvent {
    pub fn new(start_beats: f64, dur_beats: f64, midi: u8) -> Self {
        Self { start_beats, dur_beats, midi, velocity: 1.0 }
    }
}

/// 振荡器波形（本阶段默认音色的可选项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Waveform {
    Sine,
    #[default]
    Square,
    Saw,
}

impl Waveform {
    /// 从谱面指令 / 配置字符串解析波形。
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sine" | "sin" => Some(Self::Sine),
            "square" | "sq" => Some(Self::Square),
            "saw" | "sawtooth" => Some(Self::Saw),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Sine => "sine",
            Self::Square => "square",
            Self::Saw => "sawtooth",
        }
    }
}

/// 谱面级参数（默认值来自配置，可被谱面指令覆盖）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreParams {
    pub bpm: f64,
    pub volume: f64,
    pub waveform: Waveform,
}

impl Default for ScoreParams {
    fn default() -> Self {
        Self { bpm: 120.0, volume: 0.8, waveform: Waveform::Square }
    }
}

/// 一次完整解析的结果：音符时间线 + 谱面级参数。
#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub notes: Vec<NoteEvent>,
    pub params: ScoreParams,
}

impl Score {
    /// 最后一个音符结束的拍数（起点 + 时长），用于估算渲染长度。
    pub fn total_beats(&self) -> f64 {
        self.notes
            .iter()
            .map(|n| n.start_beats + n.dur_beats)
            .fold(0.0, f64::max)
    }
}

/// 将 MIDI 音高转换为可读记法（如 48 → `C3`，61 → `C#4`）。
pub fn note_name(midi: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let oct = (midi as i32 / 12) - 1;
    format!("{}{}", NAMES[(midi % 12) as usize], oct)
}