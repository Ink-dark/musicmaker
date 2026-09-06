//! musicmaker 核心库：谱面解析、键位映射、合成、渲染。
//!
//! 本阶段（M0）范围：`文件 → 音乐`，单一默认音色，无音色切换、无分轨。

pub mod config;
pub mod diagnostics;
pub mod keymap;
pub mod model;
pub mod parser;
pub mod render;
pub mod synth;