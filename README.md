# musicmaker

把纯文本变成音乐：用数字字符和英文字母按音高排序书写乐谱，程序解析并用合成器实时生成音乐（实时播放 / 离线渲染 WAV）。

零音乐理论门槛、零采样素材、纯 Rust 合成器发声。本阶段为单一默认音色（音色切换与分轨列入远期）。

## 快速上手

```bash
# 渲染成 WAV
mm render examples/twinkle.txt -o out.wav

# 实时播放
mm play examples/twinkle.txt

# 查看字符→音高对照表
mm keymap show
```

最小谱面（小星星）：

```text
@BPM=120
@WAVE=square

ccjjllj hhggeec
```

## 键位编曲模型

字符按排序表 → 音高表循环映射，位置即音高：`'0'`→C3、`'c'`→C4、`'j'`→G4 … 默认映射含 62 个字符（`0-9 a-z A-Z`）与 12 半音全音阶，支持配置自定义。

## 语法速查

| 元素 | 写法 | 规则 |
| --- | --- | --- |
| 音符 | `0-9 a-z A-Z` | 每字符一拍 |
| 休止 | 空格 / `=` / `-` | 一拍静音 |
| 连奏 | `aaa` | 合并为 3 拍持续音 |
| 半拍延长 | `a.` | +0.5 拍，可连续 |
| 八度 | `^` 升 / `_` 降 | 作用到行尾，可叠加，限 C2–C7 |
| 注释 | `# 注释` | 到行尾 |
| 谱面指令 | `@BPM=120` `@VOL=0.8` `@WAVE=square` | 仅限文件头，每类一次 |

## CLI

```
mm play    <谱面>                实时播放
mm render  <谱面> [-o 文件.wav] [-r 采样率] [-f]   离线渲染（可多谱面）
mm keymap show [谱面]            键位对照表
mm config                        查看生效配置
```

退出码：0 成功 / 1 解析失败 / 2 渲染或播放失败 / 3 参数或配置错误

## 构建

需要 Rust ≥ 1.85：

```bash
cargo build --release
# 或直接安装
cargo install --path crates/mm-cli
```

## 配置

配置文件优先级：内置默认 < `config.toml`（当前目录或 `--config` 指定）< 谱面指令 < 命令行参数。模板见 `configs/default.toml`。

## 架构

```
crates/
├── mm-core/   解析 · 键位映射 · 合成 · 渲染（纯逻辑，无 UI 依赖）
└── mm-cli/    CLI 入口（clap）；TUI（ratatui）规划中
```

## 路线图

- [x] M0：CLI 解析/合成/渲染/播放
- [ ] M1：TUI 界面（编辑即预览、钢琴卷帘、播放控制）
- [ ] M2：三角波/低通滤波、播放增强、示例库与 golden 回归
- [ ] 远期：音色切换与预设库、效果器、MIDI 导出

## 许可

MIT