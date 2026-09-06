//! musicmaker 命令行入口（PRD §4.7）。
//!
//! 退出码约定：0 成功 / 1 解析失败 / 2 渲染或播放失败 / 3 参数或配置错误。

mod play;

use clap::{Parser, Subcommand};
use clap::error::ErrorKind as ClapErrorKind;
use mm_core::config::Config;
use mm_core::diagnostics;
use mm_core::model::{note_name, Score};
use mm_core::parser;
use mm_core::render;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "mm", version, about = "把文本变成音乐（键位编曲）")]
struct Cli {
    /// 配置文件路径（默认查找当前目录 config.toml，找不到用内置默认）
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum KeymapAction {
    /// 显示字符→音高对照表（带谱面文件时仅显示其中使用的字符）
    Show { file: Option<PathBuf> },
}

#[derive(Subcommand)]
enum Command {
    /// 实时播放谱面
    Play { file: PathBuf },

    /// 离线渲染 WAV（可一次渲染多个谱面）
    Render {
        files: Vec<PathBuf>,
        /// 输出文件（仅单个谱面时可用，默认 <输入名>.wav）
        #[arg(short = 'o')]
        out: Option<PathBuf>,
        /// 采样率（8000–192000）
        #[arg(short = 'r', default_value_t = 44100)]
        rate: u32,
        /// 覆盖已存在的输出文件，不做确认
        #[arg(short = 'f', long)]
        force: bool,
    },

    /// 键位映射
    Keymap {
        #[command(subcommand)]
        action: KeymapAction,
    },

    /// 查看当前生效配置
    Config {
        /// 显示配置来源与加载路径
        #[arg(long)]
        verbose: bool,
    },
}

enum AppError {
    Parse { file: String, source: String, error: parser::ParseError },
    Render(String),
    Play(String),
    Config(String),
    Args(String),
}

impl AppError {
    fn exit_code(&self) -> u8 {
        match self {
            AppError::Parse { .. } => 1,
            AppError::Render(_) | AppError::Play(_) => 2,
            AppError::Config(_) | AppError::Args(_) => 3,
        }
    }

    fn print(&self) {
        match self {
            AppError::Parse { file, source, error } => {
                eprintln!("{}", diagnostics::format_error(file, source, error));
            }
            other => eprintln!("mm: 错误: {}", other.message()),
        }
    }

    fn message(&self) -> String {
        match self {
            AppError::Parse { .. } => "解析失败".to_string(),
            AppError::Render(m) => format!("渲染失败：{m}"),
            AppError::Play(m) => format!("播放失败：{m}"),
            AppError::Config(m) => format!("配置错误：{m}"),
            AppError::Args(m) => format!("参数错误：{m}"),
        }
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = match e.kind() {
                ClapErrorKind::DisplayHelp
                | ClapErrorKind::DisplayVersion
                | ClapErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => 0,
                _ => 3,
            };
            return ExitCode::from(code);
        }
    };

    let (cfg, _src) = match load_config(cli.config.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            e.print();
            return ExitCode::from(e.exit_code());
        }
    };

    let result = match cli.command {
        Command::Play { file } => run_play(&cfg, &file),
        Command::Render { files, out, rate, force } => run_render(&cfg, files, out, rate, force),
        Command::Keymap { action } => run_keymap(&cfg, action),
        Command::Config { verbose } => run_config(&cfg, verbose),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            e.print();
            ExitCode::from(e.exit_code())
        }
    }
}

fn load_config(explicit: Option<&std::path::Path>) -> Result<(Config, Option<PathBuf>), AppError> {
    if let Some(p) = explicit {
        if !p.exists() {
            return Err(AppError::Config(format!("配置文件不存在：{}", p.display())));
        }
        let cfg = Config::load(Some(p)).map_err(|e| AppError::Config(e.to_string()))?;
        return Ok((cfg, Some(p.to_path_buf())));
    }
    let auto = PathBuf::from("config.toml");
    if auto.exists() {
        let cfg = Config::load(Some(&auto)).map_err(|e| AppError::Config(e.to_string()))?;
        Ok((cfg, Some(auto)))
    } else {
        Ok((Config::default(), None))
    }
}

fn parse_file(file: &std::path::Path, cfg: &Config) -> Result<(Score, String), AppError> {
    let source = std::fs::read_to_string(file)
        .map_err(|e| AppError::Args(format!("无法读取 {}：{}", file.display(), e)))?;
    let keymap = cfg.keymap().map_err(|e| AppError::Config(e.to_string()))?;
    let score = parser::parse(&source, &keymap, cfg).map_err(|error| AppError::Parse {
        file: file.display().to_string(),
        source: source.clone(),
        error,
    })?;
    Ok((score, source))
}

fn run_render(
    cfg: &Config,
    files: Vec<PathBuf>,
    out: Option<PathBuf>,
    rate: u32,
    force: bool,
) -> Result<(), AppError> {
    if files.is_empty() {
        return Err(AppError::Args("render 至少需要一个谱面文件".to_string()));
    }
    if !(8000..=192000).contains(&rate) {
        return Err(AppError::Args("采样率越界，合法范围 8000–192000".to_string()));
    }
    if out.is_some() && files.len() > 1 {
        return Err(AppError::Args("-o 仅适用于单个谱面；多谱面输出文件名为各自输入名".to_string()));
    }

    for file in &files {
        let (score, _src) = parse_file(file, cfg)?;
        let target = match &out {
            Some(o) => o.clone(),
            None => file.with_extension("wav"),
        };

        if target.exists() && !force {
            if !confirm_overwrite(&target) {
                eprintln!("跳过 {}：输出文件已存在（用 -f 强制覆盖）", target.display());
                continue;
            }
        }

        let samples = render::render_score(&score, rate);
        render::write_wav(&target, &samples, rate)
            .map_err(|e| AppError::Render(e.to_string()))?;

        let secs = samples.len() as f64 / rate as f64;
        eprintln!(
            "成功: {}：{} 个音符，时长 {:.2}s（{} Hz, {:.1} x 实时）",
            target.display(),
            score.notes.len(),
            secs,
            rate,
            secs / score_total_seconds(&score),
        );
    }
    Ok(())
}

fn score_total_seconds(score: &Score) -> f64 {
    60.0 / score.params.bpm * score.total_beats()
}

fn confirm_overwrite(path: &std::path::Path) -> bool {
    use std::io::{BufRead, Write};
    eprint!("{} 已存在，覆盖？[y/N] ", path.display());
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn run_keymap(cfg: &Config, action: KeymapAction) -> Result<(), AppError> {
    let km = cfg.keymap().map_err(|e| AppError::Config(e.to_string()))?;
    let base = note_name(km.base_midi());
    let KeymapAction::Show { file } = action;

    match file {
        None => {
            println!("默认键位映射（字符序 → 音高，起始 {}", base);
            println!("{}", "-".repeat(48));
            let mut line = String::new();
            for (i, &c) in km.char_order().iter().enumerate() {
                let midi = km.resolve(c, 0).unwrap();
                line.push_str(&format!("  {}→{:<4}", c, note_name(midi as u8)));
                if (i + 1) % 6 == 0 {
                    println!("{line}");
                    line.clear();
                }
            }
            if !line.is_empty() {
                println!("{line}");
            }
            let table: Vec<String> = km.pitch_table().iter().map(|s| s.to_string()).collect();
            println!("音高表: [{}]", table.join(", "));
            Ok(())
        }
        Some(f) => {
            let (_score, src) = parse_file(&f, cfg)?;
            // 只统计乐谱正文（去指令行、去注释）中的音符字符，按字符序排序列出实际音高
            use std::collections::HashMap;
            let mut counts: HashMap<char, usize> = HashMap::new();
            for raw in src.lines() {
                let line = raw.split('#').next().unwrap_or("").trim();
                if line.is_empty() || line.starts_with('@') {
                    continue;
                }
                for c in line.chars() {
                    if km.is_note_char(c) {
                        *counts.entry(c).or_insert(0) += 1;
                    }
                }
            }
            println!("谱面 {} 中的音符字符对照", f.display());
            println!("{}", "-".repeat(48));
            let mut list: Vec<(&char, &usize)> = counts.iter().collect();
            list.sort_by_key(|(c, _)| km.index_of(**c).unwrap_or(usize::MAX));
            for (c, n) in list {
                let midi = km.resolve(*c, 0).unwrap();
                println!("  '{}' x{:<4} → {} (MIDI {})", c, n, note_name(midi as u8), midi);
            }
            Ok(())
        }
    }
}

fn run_config(cfg: &Config, verbose: bool) -> Result<(), AppError> {
    println!("当前生效配置");
    println!("{}", "-".repeat(48));
    println!("  char_order   : {} 个字符", cfg.char_order.chars().count());
    println!("  pitch_table  : {:?}", cfg.pitch_table);
    println!("  base_midi    : {} ({})", cfg.base_midi, note_name(cfg.base_midi));
    println!("  bpm          : {}", cfg.bpm);
    println!("  volume       : {}", cfg.volume);
    println!("  wave         : {}", cfg.wave);
    println!("  sample_rate  : {}", cfg.sample_rate);
    if verbose {
        println!("{}", "-".repeat(48));
        println!("来源：配置文件（当前目录 config.toml 或 --config）优先，缺省用内置默认");
    }
    Ok(())
}

fn run_play(cfg: &Config, file: &std::path::Path) -> Result<(), AppError> {
    crate::play::play_file(cfg, file)
}