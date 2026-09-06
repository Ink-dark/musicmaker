//! 用户配置：键位映射与默认参数。
//!
//! 优先级：内置默认 < 配置文件 < 谱面指令（`@BPM/@VOL/@WAVE`）< 命令行参数。

use crate::keymap::{Keymap, KeymapError};
use crate::model::Waveform;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod defaults {
    pub const DEFAULT_CHAR_ORDER: &str =
        "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    /// 12 个半音（全音阶含黑键），自最低音起。
    pub const DEFAULT_PITCH_TABLE: [i8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
    /// C3（科学音高标记）。
    pub const DEFAULT_BASE_MIDI: u8 = 48;
    pub const DEFAULT_BPM: f64 = 120.0;
    pub const DEFAULT_VOLUME: f64 = 0.8;
    pub const DEFAULT_WAVE: &str = "square";
    pub const DEFAULT_SAMPLE_RATE: u32 = 44100;
}

/// 完整配置。`serde(default)` 保证缺省字段取内置默认值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub char_order: String,
    pub pitch_table: Vec<i8>,
    pub base_midi: u8,
    pub bpm: f64,
    pub volume: f64,
    pub wave: String,
    pub sample_rate: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            char_order: defaults::DEFAULT_CHAR_ORDER.to_string(),
            pitch_table: defaults::DEFAULT_PITCH_TABLE.to_vec(),
            base_midi: defaults::DEFAULT_BASE_MIDI,
            bpm: defaults::DEFAULT_BPM,
            volume: defaults::DEFAULT_VOLUME,
            wave: defaults::DEFAULT_WAVE.to_string(),
            sample_rate: defaults::DEFAULT_SAMPLE_RATE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ConfigError {
    #[error("配置文件不可读：{0}")]
    Io(String),
    #[error("配置文件格式错误：{0}")]
    Parse(String),
    #[error("配置内容非法：{0}")]
    Invalid(String),
}

impl Config {
    /// 加载配置；`path` 为 None 时返回内置默认配置。
    pub fn load(path: Option<&Path>) -> Result<Config, ConfigError> {
        match path {
            Some(p) => {
                let text = std::fs::read_to_string(p)
                    .map_err(|e| ConfigError::Io(e.to_string()))?;
                let cfg: Config =
                    toml::from_str(&text).map_err(|e| ConfigError::Parse(e.to_string()))?;
                cfg.validate()?;
                Ok(cfg)
            }
            None => Ok(Config::default()),
        }
    }

    /// 校验全部字段取值是否合法。
    pub fn validate(&self) -> Result<(), ConfigError> {
        Keymap::new(self.char_order.chars().collect(), self.pitch_table.clone(), self.base_midi)
            .map_err(|e: KeymapError| ConfigError::Invalid(e.to_string()))?;
        if !(parser_ranges::BPM_MIN..=parser_ranges::BPM_MAX).contains(&self.bpm) {
            return Err(ConfigError::Invalid(format!(
                "bpm 越界，合法范围 {}-{}",
                parser_ranges::BPM_MIN, parser_ranges::BPM_MAX
            )));
        }
        if !(parser_ranges::VOL_MIN..=parser_ranges::VOL_MAX).contains(&self.volume) {
            return Err(ConfigError::Invalid(format!(
                "volume 越界，合法范围 {}-{}",
                parser_ranges::VOL_MIN, parser_ranges::VOL_MAX
            )));
        }
        if !(8000..=192000).contains(&self.sample_rate) {
            return Err(ConfigError::Invalid("sample_rate 越界，合法范围 8000–192000".to_string()));
        }
        if Waveform::from_str(&self.wave).is_none() {
            return Err(ConfigError::Invalid(
                "wave 必须是 sine / square / saw 之一".to_string(),
            ));
        }
        Ok(())
    }

    /// 由配置构建键位映射。
    pub fn keymap(&self) -> Result<Keymap, ConfigError> {
        Keymap::new(self.char_order.chars().collect(), self.pitch_table.clone(), self.base_midi)
            .map_err(|e: KeymapError| ConfigError::Invalid(e.to_string()))
    }
}

/// 与 parser 共享的取值域，避免魔法数散落。
pub(crate) mod parser_ranges {
    pub const BPM_MIN: f64 = 20.0;
    pub const BPM_MAX: f64 = 300.0;
    pub const VOL_MIN: f64 = 0.0;
    pub const VOL_MAX: f64 = 1.0;
}