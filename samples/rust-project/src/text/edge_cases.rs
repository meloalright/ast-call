use crate::RenderCtx;

pub fn commented_out_call(ctx: &mut RenderCtx) {
    // render_text(ctx, "this is a comment, not a real call");
    // crate::text::render::render_text(ctx, "also commented");
    ctx.push_line("only this line runs");
}

pub fn render_text(ctx: &mut RenderCtx, _text: &str) {
    ctx.push_line("this is a DIFFERENT render_text, not the one in render.rs");
}

pub fn calls_local_render_text(ctx: &mut RenderCtx) {
    render_text(ctx, "calls edge_cases::render_text, not render::render_text");
}
