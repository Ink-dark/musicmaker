//! 合成引擎：振荡器 + ADSR 包络 + 防削波混音。
//!
//! 单声部时序发声，本阶段无和弦/分轨；叠加仅发生在同音连奏（解析已合并）等
//! 罕见相位重叠场景，输出统一做防削波限幅。

use crate::model::{Score, Waveform};

/// ADSR 包络参数（秒/比例）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynthParams {
    pub attack_s: f64,
    pub decay_s: f64,
    /// 延音水平（0.0–1.0，相对包络最大值）。
    pub sustain: f64,
    pub release_s: f64,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self { attack_s: 0.005, decay_s: 0.02, sustain: 0.8, release_s: 0.05 }
    }
}

/// 将时间线渲染为单声道 f32 采样（含防削波）。
pub fn render_timeline(score: &Score, sample_rate: u32, params: &SynthParams) -> Vec<f32> {
    let sr = sample_rate as f64;
    let beat_s = 60.0 / score.params.bpm;
    let total_beats = score.total_beats();
    // 总长度 = 乐谱结束时刻 + 末音释音 + 安全余量
    let total_samples =
        ((total_beats * beat_s) * sr).ceil() as usize + (params.release_s * sr).ceil() as usize + 4;
    let mut out = vec![0f32; total_samples];

    let attack = (params.attack_s * sr).max(1.0) as usize;
    let decay = (params.decay_s * sr).max(1.0) as usize;
    let release = (params.release_s * sr).ceil().max(1.0) as usize;

    for n in &score.notes {
        let freq = midi_to_freq(n.midi);
        let start = (n.start_beats * beat_s * sr).round() as usize;
        let dur = (n.dur_beats * beat_s * sr).round().max(1.0) as usize;

        // 短音规整：若 起音+衰减 长于音符本身，按比例压缩，保证延音段可达
        let (a, d) = if attack + decay > dur {
            let f = dur as f64 / (attack + decay) as f64;
            (((attack as f64) * f).max(1.0) as usize, ((decay as f64) * f).max(1.0) as usize)
        } else {
            (attack, decay)
        };

        let sustain_level = params.sustain;
        let amp = n.velocity * score.params.volume;
        let local = dur + release;

        for k in 0..local {
            let idx = start + k;
            if idx >= out.len() {
                break;
            }
            let env = if k < a {
                k as f64 / a as f64
            } else if k < a + d {
                1.0 - (1.0 - sustain_level) * ((k - a) as f64 / d as f64)
            } else if k < dur {
                sustain_level
            } else {
                let r = k - dur;
                sustain_level * (1.0 - r as f64 / release as f64).max(0.0)
            };
            out[idx] += (osc_value(score.params.waveform, freq, k as f64 / sr) * env * amp)
                as f32;
        }
    }

    // 防削波
    for s in &mut out {
        *s = s.clamp(-1.0, 1.0);
    }
    out
}

/// 波形采样：`t` 为相对音符起点的秒。
fn osc_value(wave: Waveform, freq: f64, t: f64) -> f64 {
    let phase = (t * freq).fract(); // 0..1
    match wave {
        Waveform::Sine => (2.0 * std::f64::consts::PI * phase).sin(),
        Waveform::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Waveform::Saw => 2.0 * phase - 1.0,
    }
}

/// MIDI 音高 → 频率（A4 = 440Hz 十二平均律）。
pub fn midi_to_freq(midi: u8) -> f64 {
    440.0 * 2f64.powf((midi as f64 - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NoteEvent, Score, ScoreParams};

    fn score_of(notes: Vec<NoteEvent>) -> Score {
        Score { notes, params: ScoreParams::default() }
    }

    #[test]
    fn single_note_is_not_silent_and_bounded() {
        let s = score_of(vec![NoteEvent::new(0.0, 1.0, 48)]);
        let out = render_timeline(&s, 44100, &SynthParams::default());
        assert!(!out.is_empty());
        let peak = out.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.01, "音符不应静音");
        assert!(peak <= 1.0);
        // 一拍 @120BPM = 0.5s = 22050 采样，加上释音
        assert!(out.len() >= 22050);
    }

    #[test]
    fn length_matches_total_beats() {
        let s = score_of(vec![NoteEvent::new(0.0, 2.0, 60), NoteEvent::new(3.0, 1.0, 65)]);
        let out = render_timeline(&s, 44100, &SynthParams::default());
        // 结束拍 4 → 2s = 88200 采样 + 释音
        assert!(out.len() >= 88200 && out.len() <= 88200 + 4096);
    }

    #[test]
    fn waveforms_differ() {
        let base = ScoreParams::default();
        let make = |w: Waveform| {
            let s = Score {
                notes: vec![NoteEvent::new(0.0, 0.5, 60)],
                params: ScoreParams { waveform: w, ..base },
            };
            render_timeline(&s, 8000, &SynthParams::default())
        };
        assert_ne!(make(Waveform::Sine), make(Waveform::Square));
        assert_ne!(make(Waveform::Sine), make(Waveform::Saw));
    }
}