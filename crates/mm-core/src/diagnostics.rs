//! 错误展示：按 PRD §4.9 输出带源码摘录与定位标记的诊断。

use crate::parser::ParseError;

/// 组装完整诊断文本（文件前缀 + 源码行 + 插入符 + 原因 + 建议）。
pub fn format_error(file: &str, source: &str, err: &ParseError) -> String {
    let line_text: String = source
        .lines()
        .nth(err.line.saturating_sub(1))
        .unwrap_or("")
        .chars()
        .collect();
    let line_no = err.line.to_string();
    let caret = " ".repeat(err.col.saturating_sub(1)) + "^";

    format!(
        "{file}:{line}:{col}  无法解析\n\
         \x20 {line_no} │ {line_text}\n\
         {pad}   {caret}\n\
         原因：{msg}\n\
         建议：{sugg}",
        file = file,
        line = err.line,
        col = err.col,
        pad = " ".repeat(line_no.chars().count()),
        caret = caret,
        msg = err.message(),
        sugg = err.suggestion(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::parser::{parse, ParseErrorKind};

    #[test]
    fn diagnostic_shows_position_and_caret() {
        let src = "abc[def\n";
        let e = parse(src, &Config::default().keymap().unwrap(), &Config::default())
            .expect_err("应当失败");
        assert_eq!(e.kind, ParseErrorKind::UnknownCharacter('['));
        let out = format_error("demo.txt", src, &e);
        assert!(out.contains("demo.txt:1:4"), "应含文件与行列：{}", out);
        assert!(out.contains("^"), "应含定位标记");
        assert!(out.contains("原因："), "应含原因");
        assert!(out.contains("建议："), "应含建议");
    }
}