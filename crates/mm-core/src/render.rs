//! 离线渲染：采样 → 16-bit PCM 单声道 WAV（临时文件 + 原子改名）。

use crate::model::Score;
use crate::synth::{render_timeline, SynthParams};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RenderError {
    #[error("{0}")]
    Message(String),
}

/// 将谱面渲染为单声道 f32 采样（默认包络参数）。
pub fn render_score(score: &Score, sample_rate: u32) -> Vec<f32> {
    render_timeline(score, sample_rate, &SynthParams::default())
}

/// 将采样写为 16-bit PCM WAV。采用「临时文件 + 原子改名」，中途失败不残留半成品。
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), RenderError> {
    use hound::{SampleFormat, WavSpec, WavWriter};

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let tmp = tmp_path(path);
    let mut writer =
        WavWriter::create(&tmp, spec).map_err(|e| RenderError::Message(e.to_string()))?;
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        writer
            .write_sample(v)
            .map_err(|e| RenderError::Message(e.to_string()))?;
    }
    writer.finalize().map_err(|e| RenderError::Message(e.to_string()))?;

    // Windows 上 rename 无法覆盖已存在文件，先移除旧目标
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| RenderError::Message(e.to_string()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| RenderError::Message(e.to_string()))?;
    Ok(())
}

/// 同目录下的临时文件名：`a.wav` → `a.wav.mm-tmp-NNN`。
fn tmp_path(path: &Path) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(format!(".mm-tmp-{}", std::process::id()));
    std::path::PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NoteEvent, Score, ScoreParams};
    use hound::WavReader;
    use std::io::Cursor;

    #[test]
    fn render_then_read_back_wav() {
        let s = Score {
            notes: vec![NoteEvent::new(0.0, 1.0, 60)],
            params: ScoreParams::default(),
        };
        let samples = render_score(&s, 8000);
        let mut bytes = Vec::new();
        {
            use hound::{SampleFormat, WavSpec, WavWriter};
            let spec = WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            };
            let mut w = WavWriter::new(std::io::Cursor::new(&mut bytes), spec).unwrap();
            for &x in &samples {
                w.write_sample((x * i16::MAX as f32) as i16).unwrap();
            }
            w.finalize().unwrap();
        }
        let mut reader = WavReader::new(Cursor::new(&bytes)).unwrap();
        let read: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(read.len(), samples.len());
        let peak = read.iter().map(|x| x.abs()).max().unwrap();
        assert!(peak > 100, "输出不应静音");
    }
}