use crate::RenderCtx;
use crate::text::render::render_text;

pub fn render_chinese(ctx: &mut RenderCtx) {
    render_text(ctx, "你好世界！这是一段很长的中文文本，用来测试ast-call在处理包含中文字符的函数调用时是否会因为UTF-8边界问题而崩溃。我们需要确保截断发生在字符边界上。");
}

pub fn render_mixed(ctx: &mut RenderCtx) {
    render_text(ctx, "Hello 世界 こんにちは мир 🌍 this is a long mixed-script string to test truncation at multi-byte boundaries safely");
}
