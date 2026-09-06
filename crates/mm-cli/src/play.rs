//! 实时播放：cpal 输出。渲染与实时播放共用同一合成内核。

use crate::AppError;
use mm_core::config::Config;
use mm_core::render;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub fn play_file(cfg: &Config, file: &Path) -> Result<(), AppError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let (score, _src) = crate::parse_file(file, cfg)?;

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| AppError::Play("未找到音频输出设备，可改用 mm render 渲染 WAV 文件".to_string()))?;
    let supported = device
        .default_output_config()
        .map_err(|e| AppError::Play(format!("无法获取音频输出配置：{e}")))?;

    let sr = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let samples = render::render_score(&score, sr);

    let state = Arc::new(Mutex::new(PlayState { samples, pos: 0, channels, done: false }));
    let error_flag = Arc::new(AtomicBool::new(false));

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &supported, &state, &error_flag),
        cpal::SampleFormat::I16 => build::<i16>(&device, &supported, &state, &error_flag),
        other => {
            return Err(AppError::Play(format!("不支持的音频输出格式：{other:?}")));
        }
    }
    .map_err(|e| AppError::Play(e.to_string()))?;

    stream
        .play()
        .map_err(|e| AppError::Play(format!("无法开始播放：{e}")))?;

    loop {
        if error_flag.load(Ordering::SeqCst) {
            return Err(AppError::Play("音频输出流错误（设备中断），已停止播放".to_string()));
        }
        let done = state.lock().map(|s| s.done).unwrap_or(true);
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Ok(())
}

struct PlayState {
    samples: Vec<f32>,
    pos: usize,
    channels: usize,
    done: bool,
}

/// 为某采样格式构建输出流。回调把单声道样本复制到各声道帧；耗尽后补零并置 done。
fn build<T>(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    state: &Arc<Mutex<PlayState>>,
    error_flag: &Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;

    let config = supported.config();
    let state = Arc::clone(state);
    let error_flag = Arc::clone(error_flag);

    let data_cb = move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
        let mut st = match state.lock() {
            Ok(s) => s,
            Err(_) => {
                for x in out.iter_mut() {
                    *x = T::EQUILIBRIUM;
                }
                return;
            }
        };
        let channels = st.channels.max(1);
        let len = st.samples.len();
        let mut pos = st.pos;
        for frame in out.chunks_mut(channels) {
            let value = st.samples.get(pos).copied().unwrap_or(0.0);
            for x in frame.iter_mut() {
                *x = T::from_sample(value);
            }
            pos += 1;
        }
        st.pos = pos;
        if pos >= len {
            st.done = true;
        }
    };
    let err_cb = move |e: cpal::StreamError| {
        eprintln!("音频流错误：{e}");
        error_flag.store(true, Ordering::SeqCst);
    };

    device.build_output_stream(&config, data_cb, err_cb, None)
}