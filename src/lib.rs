use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    CanvasRenderingContext2d, Document, Event, FileReader, HtmlAnchorElement, HtmlCanvasElement,
    HtmlElement, HtmlImageElement, HtmlInputElement, MouseEvent, ProgressEvent, TouchEvent,
};

const PREVIEW_MAX: f64 = 360.0;

struct AppState {
    image: Option<HtmlImageElement>,
    zoom: f64,
    offset_x: f64,
    offset_y: f64,
    base_scale: f64,
    sel_w: u32,
    sel_h: u32,
    dragging: bool,
    drag_start: (f64, f64),
    offset_start: (f64, f64),
    fill_white: bool,
}

impl AppState {
    fn new() -> Self {
        AppState {
            image: None,
            zoom: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            base_scale: 1.0,
            sel_w: 600,
            sel_h: 800,
            dragging: false,
            drag_start: (0.0, 0.0),
            offset_start: (0.0, 0.0),
            fill_white: true,
        }
    }
}

type Shared = Rc<RefCell<AppState>>;

#[wasm_bindgen(start)]
pub fn run() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let window = web_sys::window().ok_or("janela global não encontrada")?;
    let document = window.document().ok_or("documento não encontrado")?;

    inject_style(&document)?;
    inject_body(&document)?;

    let state: Shared = Rc::new(RefCell::new(AppState::new()));

    let canvas: HtmlCanvasElement = document
        .get_element_by_id("preview-canvas")
        .unwrap()
        .dyn_into()?;
    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")?
        .unwrap()
        .dyn_into()?;

    resize_preview_canvas(&canvas, 600, 800);

    wire_file_input(&document, &state, &canvas, &ctx)?;
    wire_size_options(&document, &state, &canvas, &ctx)?;
    wire_fill_toggle(&document, &state)?;
    wire_zoom(&document, &state, &canvas, &ctx)?;
    wire_pointer_events(&document, &state, &canvas, &ctx)?;
    wire_download(&document, &state)?;

    Ok(())
}

fn el<T: JsCast>(document: &Document, id: &str) -> Result<T, JsValue> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("elemento #{id} não encontrado")))?
        .dyn_into::<T>()
        .map_err(|_| JsValue::from_str(&format!("elemento #{id} tem tipo inesperado")))
}

// ---------- layout ----------

fn inject_style(document: &Document) -> Result<(), JsValue> {
    let head = document.head().ok_or("sem <head>")?;
    let style = document.create_element("style")?;
    style.set_text_content(Some(CSS));
    head.append_child(&style)?;
    Ok(())
}

fn inject_body(document: &Document) -> Result<(), JsValue> {
    let body = document.body().ok_or("sem <body>")?;
    body.set_inner_html(BODY_HTML);
    Ok(())
}

// ---------- canvas helpers ----------

fn preview_dims(w: u32, h: u32) -> (u32, u32) {
    let ratio = w as f64 / h as f64;
    if ratio >= 1.0 {
        (PREVIEW_MAX as u32, (PREVIEW_MAX / ratio) as u32)
    } else {
        ((PREVIEW_MAX * ratio) as u32, PREVIEW_MAX as u32)
    }
}

fn resize_preview_canvas(canvas: &HtmlCanvasElement, w: u32, h: u32) {
    let (cw, ch) = preview_dims(w, h);
    canvas.set_width(cw);
    canvas.set_height(ch);
}

fn fit_initial(state: &mut AppState, canvas: &HtmlCanvasElement) {
    if let Some(img) = &state.image {
        let cw = canvas.width() as f64;
        let ch = canvas.height() as f64;
        let iw = img.natural_width().max(1) as f64;
        let ih = img.natural_height().max(1) as f64;
        state.base_scale = (cw / iw).max(ch / ih);
        state.zoom = 1.0;
        state.offset_x = 0.0;
        state.offset_y = 0.0;
    }
}

fn clamp_offset(state: &mut AppState, canvas: &HtmlCanvasElement) {
    if let Some(img) = &state.image {
        let cw = canvas.width() as f64;
        let ch = canvas.height() as f64;
        let scale = state.base_scale * state.zoom;
        let draw_w = img.natural_width() as f64 * scale;
        let draw_h = img.natural_height() as f64 * scale;
        let max_x = ((draw_w - cw) / 2.0).max(0.0);
        let max_y = ((draw_h - ch) / 2.0).max(0.0);
        state.offset_x = state.offset_x.clamp(-max_x, max_x);
        state.offset_y = state.offset_y.clamp(-max_y, max_y);
    }
}

fn draw(state: &AppState, canvas: &HtmlCanvasElement, ctx: &CanvasRenderingContext2d) {
    let cw = canvas.width() as f64;
    let ch = canvas.height() as f64;

    ctx.clear_rect(0.0, 0.0, cw, ch);
    ctx.set_fill_style(&JsValue::from_str("#ffffff"));
    ctx.fill_rect(0.0, 0.0, cw, ch);

    if let Some(img) = &state.image {
        let scale = state.base_scale * state.zoom;
        let draw_w = img.natural_width() as f64 * scale;
        let draw_h = img.natural_height() as f64 * scale;
        let cx = cw / 2.0 + state.offset_x;
        let cy = ch / 2.0 + state.offset_y;
        let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
            img,
            cx - draw_w / 2.0,
            cy - draw_h / 2.0,
            draw_w,
            draw_h,
        );
    }

    ctx.set_stroke_style(&JsValue::from_str("rgba(47,93,58,0.5)"));
    ctx.set_line_width(1.0);
    ctx.stroke_rect(0.5, 0.5, cw - 1.0, ch - 1.0);
}

// ---------- wiring: file input ----------

fn wire_file_input(
    document: &Document,
    state: &Shared,
    canvas: &HtmlCanvasElement,
    ctx: &CanvasRenderingContext2d,
) -> Result<(), JsValue> {
    let pick_btn: HtmlElement = el(document, "pick-btn")?;
    let file_input: HtmlInputElement = el(document, "file-input")?;

    {
        let file_input = file_input.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |_e: MouseEvent| {
            file_input.click();
        });
        pick_btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    let document = document.clone();
    let state = state.clone();
    let canvas = canvas.clone();
    let ctx = ctx.clone();
    let closure = Closure::<dyn FnMut(_)>::new(move |e: Event| {
        let input: HtmlInputElement = e.target().unwrap().dyn_into().unwrap();
        if let Some(files) = input.files() {
            if let Some(file) = files.get(0) {
                load_file(&document, &state, &canvas, &ctx, file);
            }
        }
    });
    file_input.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
    closure.forget();

    Ok(())
}

fn load_file(
    document: &Document,
    state: &Shared,
    canvas: &HtmlCanvasElement,
    ctx: &CanvasRenderingContext2d,
    file: web_sys::File,
) {
    let reader = FileReader::new().unwrap();
    let file_name = file.name();

    let document = document.clone();
    let state = state.clone();
    let canvas = canvas.clone();
    let ctx = ctx.clone();
    let reader_clone = reader.clone();

    let onload = Closure::<dyn FnMut(_)>::new(move |_e: ProgressEvent| {
        let data_url = reader_clone.result().unwrap().as_string().unwrap();

        let img = HtmlImageElement::new().unwrap();
        let document2 = document.clone();
        let state2 = state.clone();
        let canvas2 = canvas.clone();
        let ctx2 = ctx.clone();
        let file_name = file_name.clone();

        let img_for_closure = img.clone();
        let onimg = Closure::<dyn FnMut(_)>::new(move |_e: Event| {
            state2.borrow_mut().image = Some(img_for_closure.clone());
            fit_initial(&mut state2.borrow_mut(), &canvas2);
            clamp_offset(&mut state2.borrow_mut(), &canvas2);
            draw(&state2.borrow(), &canvas2, &ctx2);

            if let Some(empty) = document2.get_element_by_id("empty-state") {
                let _ = empty
                    .dyn_into::<HtmlElement>()
                    .unwrap()
                    .style()
                    .set_property("display", "none");
            }
            if let Ok(status) = el::<HtmlElement>(&document2, "stage-status") {
                status.set_text_content(Some(&format!("✓ {file_name}")));
            }
            if let Ok(zoom) = el::<HtmlInputElement>(&document2, "zoom-range") {
                zoom.set_disabled(false);
                zoom.set_value("100");
            }
            if let Ok(btn) = el::<HtmlElement>(&document2, "download-btn") {
                let _ = btn.remove_attribute("disabled");
            }
        });
        img.set_onload(Some(onimg.as_ref().unchecked_ref()));
        onimg.forget();

        img.set_src(&data_url);
    });

    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();
    reader.read_as_data_url(&file).unwrap();
}

// ---------- wiring: size options ----------

fn wire_size_options(
    document: &Document,
    state: &Shared,
    canvas: &HtmlCanvasElement,
    ctx: &CanvasRenderingContext2d,
) -> Result<(), JsValue> {
    let nodes = document.query_selector_all(".size-opt")?;
    for i in 0..nodes.length() {
        let node = nodes.get(i).unwrap();
        let element: HtmlElement = node.dyn_into()?;

        let document = document.clone();
        let state = state.clone();
        let canvas = canvas.clone();
        let ctx = ctx.clone();
        let target_el = element.clone();

        let closure = Closure::<dyn FnMut(_)>::new(move |_e: MouseEvent| {
            let w: u32 = target_el.get_attribute("data-w").unwrap().parse().unwrap();
            let h: u32 = target_el.get_attribute("data-h").unwrap().parse().unwrap();

            {
                let mut s = state.borrow_mut();
                s.sel_w = w;
                s.sel_h = h;
            }

            if let Ok(all) = document.query_selector_all(".size-opt") {
                for j in 0..all.length() {
                    if let Some(n) = all.get(j) {
                        if let Ok(e) = n.dyn_into::<HtmlElement>() {
                            let _ = e.class_list().remove_1("active");
                        }
                    }
                }
            }
            let _ = target_el.class_list().add_1("active");

            if let Ok(ratio_el) = el::<HtmlElement>(&document, "stage-ratio") {
                ratio_el.set_text_content(Some(&simplify_ratio(w, h)));
            }

            resize_preview_canvas(&canvas, w, h);
            {
                let mut s = state.borrow_mut();
                fit_initial(&mut s, &canvas);
                clamp_offset(&mut s, &canvas);
            }
            draw(&state.borrow(), &canvas, &ctx);
            if let Ok(zoom) = el::<HtmlInputElement>(&document, "zoom-range") {
                zoom.set_value("100");
            }
        });

        element.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }
    Ok(())
}

fn simplify_ratio(w: u32, h: u32) -> String {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    let g = gcd(w, h).max(1);
    format!("{} : {}", w / g, h / g)
}

// ---------- wiring: fill toggle ----------

fn wire_fill_toggle(document: &Document, state: &Shared) -> Result<(), JsValue> {
    let checkbox: HtmlInputElement = el(document, "fill-white")?;
    let state = state.clone();
    let checkbox_clone = checkbox.clone();
    let closure = Closure::<dyn FnMut(_)>::new(move |_e: Event| {
        state.borrow_mut().fill_white = checkbox_clone.checked();
    });
    checkbox.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

// ---------- wiring: zoom ----------

fn wire_zoom(
    document: &Document,
    state: &Shared,
    canvas: &HtmlCanvasElement,
    ctx: &CanvasRenderingContext2d,
) -> Result<(), JsValue> {
    let zoom_range: HtmlInputElement = el(document, "zoom-range")?;
    let state = state.clone();
    let canvas = canvas.clone();
    let ctx = ctx.clone();
    let zoom_clone = zoom_range.clone();

    let closure = Closure::<dyn FnMut(_)>::new(move |_e: Event| {
        let value: f64 = zoom_clone.value().parse().unwrap_or(100.0);
        {
            let mut s = state.borrow_mut();
            s.zoom = value / 100.0;
            clamp_offset(&mut s, &canvas);
        }
        draw(&state.borrow(), &canvas, &ctx);
    });
    zoom_range.add_event_listener_with_callback("input", closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

// ---------- wiring: drag / pan (mouse + touch) ----------

fn wire_pointer_events(
    document: &Document,
    state: &Shared,
    canvas: &HtmlCanvasElement,
    ctx: &CanvasRenderingContext2d,
) -> Result<(), JsValue> {
    let frame: HtmlElement = el(document, "canvas-frame")?;

    // mouse down
    {
        let state = state.clone();
        let frame_clone = frame.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |e: MouseEvent| {
            let mut s = state.borrow_mut();
            if s.image.is_none() {
                return;
            }
            s.dragging = true;
            s.drag_start = (e.client_x() as f64, e.client_y() as f64);
            s.offset_start = (s.offset_x, s.offset_y);
            let _ = frame_clone.class_list().add_1("dragging");
        });
        canvas.add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // mouse move (on window so drag continues outside canvas)
    {
        let window = web_sys::window().unwrap();
        let state = state.clone();
        let canvas = canvas.clone();
        let ctx = ctx.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |e: MouseEvent| {
            let mut s = state.borrow_mut();
            if !s.dragging {
                return;
            }
            let (sx, sy) = s.drag_start;
            let (ox, oy) = s.offset_start;
            s.offset_x = ox + (e.client_x() as f64 - sx);
            s.offset_y = oy + (e.client_y() as f64 - sy);
            clamp_offset(&mut s, &canvas);
            drop(s);
            draw(&state.borrow(), &canvas, &ctx);
        });
        window.add_event_listener_with_callback("mousemove", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // mouse up (on window)
    {
        let window = web_sys::window().unwrap();
        let state = state.clone();
        let frame_clone = frame.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |_e: MouseEvent| {
            state.borrow_mut().dragging = false;
            let _ = frame_clone.class_list().remove_1("dragging");
        });
        window.add_event_listener_with_callback("mouseup", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // touch start
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |e: TouchEvent| {
            if let Some(touch) = e.touches().get(0) {
                let mut s = state.borrow_mut();
                if s.image.is_none() {
                    return;
                }
                s.dragging = true;
                s.drag_start = (touch.client_x() as f64, touch.client_y() as f64);
                s.offset_start = (s.offset_x, s.offset_y);
            }
        });
        canvas.add_event_listener_with_callback("touchstart", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // touch move
    {
        let state = state.clone();
        let canvas_target = canvas.clone();
        let canvas = canvas.clone();
        let ctx = ctx.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |e: TouchEvent| {
            if let Some(touch) = e.touches().get(0) {
                let mut s = state.borrow_mut();
                if !s.dragging {
                    return;
                }
                let (sx, sy) = s.drag_start;
                let (ox, oy) = s.offset_start;
                s.offset_x = ox + (touch.client_x() as f64 - sx);
                s.offset_y = oy + (touch.client_y() as f64 - sy);
                clamp_offset(&mut s, &canvas);
                drop(s);
                draw(&state.borrow(), &canvas, &ctx);
            }
        });
        canvas_target.add_event_listener_with_callback("touchmove", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // touch end
    {
        let state = state.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |_e: TouchEvent| {
            state.borrow_mut().dragging = false;
        });
        canvas.add_event_listener_with_callback("touchend", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    Ok(())
}

// ---------- wiring: download ----------

fn wire_download(document: &Document, state: &Shared) -> Result<(), JsValue> {
    let btn: HtmlElement = el(document, "download-btn")?;
    let document = document.clone();
    let state = state.clone();

    let closure = Closure::<dyn FnMut(_)>::new(move |_e: MouseEvent| {
        let s = state.borrow();
        let img = match &s.image {
            Some(i) => i.clone(),
            None => return,
        };

        let out_w = s.sel_w;
        let out_h = s.sel_h;

        let out_canvas: HtmlCanvasElement = document
            .create_element("canvas")
            .unwrap()
            .dyn_into()
            .unwrap();
        out_canvas.set_width(out_w);
        out_canvas.set_height(out_h);
        let out_ctx: CanvasRenderingContext2d = out_canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into()
            .unwrap();

        if s.fill_white {
            out_ctx.set_fill_style(&JsValue::from_str("#ffffff"));
            out_ctx.fill_rect(0.0, 0.0, out_w as f64, out_h as f64);
        }

        // canvas de pré-visualização atual define a escala relativa
        let preview_canvas: HtmlCanvasElement =
            el(&document, "preview-canvas").unwrap();
        let scale_ratio = out_w as f64 / preview_canvas.width() as f64;

        let scale = s.base_scale * s.zoom * scale_ratio;
        let draw_w = img.natural_width() as f64 * scale;
        let draw_h = img.natural_height() as f64 * scale;
        let cx = out_w as f64 / 2.0 + s.offset_x * scale_ratio;
        let cy = out_h as f64 / 2.0 + s.offset_y * scale_ratio;

        let _ = out_ctx.draw_image_with_html_image_element_and_dw_and_dh(
            &img,
            cx - draw_w / 2.0,
            cy - draw_h / 2.0,
            draw_w,
            draw_h,
        );

        let filename = format!("foto-3x4-{out_w}x{out_h}.jpg");
        let document2 = document.clone();
        let callback = Closure::<dyn FnMut(_)>::new(move |blob: web_sys::Blob| {
            let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
            let a: HtmlAnchorElement = document2
                .create_element("a")
                .unwrap()
                .dyn_into()
                .unwrap();
            a.set_href(&url);
            a.set_download(&filename);
            a.click();
            web_sys::Url::revoke_object_url(&url).ok();
        });
        let _ = out_canvas
            .to_blob_with_type(callback.as_ref().unchecked_ref(), "image/jpeg");
        callback.forget();
    });

    btn.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

// ---------- markup ----------

const BODY_HTML: &str = r##"
<nav>
  <a href="#topo" class="logo">Foto3x4.</a>
  <div class="nav-links">
    <a href="#topo">Início</a>
    <a href="#conversor">Conversor</a>
    <a href="https://github.com/" target="_blank" rel="noopener">GitHub</a>
  </div>
</nav>

<div class="wrap" id="topo">
  <header>
    <span class="eyebrow">Rust · WebAssembly · sem servidor</span>
    <h1>Foto 3x4</h1>
    <p class="lede">Recorte, ajuste o zoom e exporte sua foto no formato de documento. Todo o processamento roda em WebAssembly, direto no navegador.</p>
  </header>
  <div id="conversor"></div>

  <div class="layout">
    <div class="card stage">
      <div class="stage-head">
        <span id="stage-status">Nenhuma imagem carregada</span>
        <span id="stage-ratio">3 : 4</span>
      </div>
      <div class="canvas-frame" id="canvas-frame">
        <div class="empty-state" id="empty-state">
          <strong>Selecione uma foto</strong>
          use o botão ao lado
        </div>
        <canvas id="preview-canvas"></canvas>
      </div>
      <div class="zoom-row">
        <label for="zoom-range">ZOOM</label>
        <input type="range" id="zoom-range" min="100" max="300" value="100" disabled>
      </div>
    </div>

    <div class="card panel">
      <div>
        <span class="field-label">01 — Imagem</span>
        <button class="file-btn" id="pick-btn" type="button">Selecionar foto</button>
        <input type="file" id="file-input" accept="image/*" style="display:none">
      </div>

      <div>
        <span class="field-label">02 — Tamanho final</span>
        <div class="sizes">
          <div class="size-opt active" data-w="300" data-h="400">
            <span class="dim">300×400</span><span class="desc">Pequeno</span>
          </div>
          <div class="size-opt" data-w="600" data-h="800">
            <span class="dim">600×800</span><span class="desc">Médio (recomendado)</span>
          </div>
          <div class="size-opt" data-w="900" data-h="1200">
            <span class="dim">900×1200</span><span class="desc">Grande</span>
          </div>
          <div class="size-opt" data-w="1200" data-h="1600">
            <span class="dim">1200×1600</span><span class="desc">Muito grande</span>
          </div>
        </div>
      </div>

      <div>
        <span class="field-label">03 — Fundo</span>
        <label class="toggle-row" for="fill-white">
          <span>
            <span class="t-label">Preencher com branco</span>
          </span>
          <input type="checkbox" id="fill-white" checked style="width:18px;height:18px;">
        </label>
      </div>

      <button class="convert-btn" id="download-btn" disabled>Baixar foto 3x4</button>
      <span class="note">Arraste dentro da moldura para reposicionar o enquadramento.</span>
    </div>
  </div>
</div>
<footer>conversor-3x4 · Rust + WebAssembly, sem backend</footer>
"##;

const CSS: &str = r#"
:root{
  --paper:#eef0ea; --card:#ffffff; --ink:#1c2b1f; --ink-soft:#5b6b5e;
  --line:#c9d0c3; --accent:#2f5d3a; --accent-dark:#1f3f27;
}
*{box-sizing:border-box;}
html{scroll-behavior:smooth;}
body{margin:0;background:var(--paper);color:var(--ink);
  font-family:-apple-system,"Segoe UI",Roboto,Arial,sans-serif;min-height:100vh;padding:0 0 80px;}
nav{position:sticky;top:0;left:0;width:100%;z-index:1000;
  display:flex;justify-content:space-between;align-items:center;
  padding:14px 20px;background:rgba(238,240,234,0.9);backdrop-filter:blur(8px);
  border-bottom:1px solid var(--line);}
.logo{font-family:monospace;font-weight:700;font-size:1.1rem;color:var(--accent);
  text-decoration:none;letter-spacing:.02em;}
.nav-links{display:flex;gap:1.4rem;align-items:center;}
.nav-links a{text-decoration:none;color:var(--ink);font-weight:600;font-size:.9rem;transition:color .2s;}
.nav-links a:hover{color:var(--accent);}
@media (max-width:640px){.nav-links a:not(:last-child){display:none;}}
.wrap{max-width:960px;margin:0 auto;padding:40px 20px 0;}
.eyebrow{font-family:monospace;font-size:11px;letter-spacing:.1em;text-transform:uppercase;
  color:var(--accent);border:1px solid var(--accent);padding:3px 8px;border-radius:2px;display:inline-block;margin-bottom:14px;}
h1{font-size:clamp(32px,5vw,46px);margin:0 0 8px;}
.lede{color:var(--ink-soft);font-size:15px;max-width:520px;line-height:1.5;}
.layout{display:grid;grid-template-columns:1.2fr 1fr;gap:24px;}
@media (max-width:800px){.layout{grid-template-columns:1fr;}}
.card{background:var(--card);border:1px solid var(--line);border-radius:6px;}
.stage{padding:20px;display:flex;flex-direction:column;gap:14px;}
.stage-head{display:flex;justify-content:space-between;font-family:monospace;font-size:11px;
  text-transform:uppercase;color:var(--ink-soft);}
.canvas-frame{position:relative;border:1px dashed var(--line);border-radius:4px;background:#fbfbfa;
  display:flex;align-items:center;justify-content:center;min-height:380px;overflow:hidden;cursor:grab;}
.canvas-frame.dragging{cursor:grabbing;}
.empty-state{position:absolute;text-align:center;color:var(--ink-soft);font-size:13px;}
.empty-state strong{display:block;color:var(--ink);font-size:17px;margin-bottom:4px;}
.zoom-row{display:flex;align-items:center;gap:10px;}
.zoom-row label{font-family:monospace;font-size:11px;color:var(--ink-soft);}
input[type=range]{flex:1;}
.panel{padding:24px;display:flex;flex-direction:column;gap:22px;}
.field-label{font-family:monospace;font-size:11px;text-transform:uppercase;letter-spacing:.08em;
  color:var(--ink-soft);display:block;margin-bottom:10px;}
.file-btn{width:100%;background:var(--ink);color:#fff;border:none;border-radius:4px;padding:13px 16px;
  font-size:14px;font-weight:600;cursor:pointer;}
.file-btn:hover{background:var(--accent-dark);}
.sizes{display:grid;grid-template-columns:1fr 1fr;gap:8px;}
.size-opt{border:1px solid var(--line);border-radius:4px;padding:10px 12px;cursor:pointer;font-size:13px;}
.size-opt:hover{border-color:var(--accent);}
.size-opt.active{border-color:var(--accent);background:#f2f7f3;}
.size-opt .dim{font-family:monospace;font-weight:600;display:block;}
.size-opt .desc{color:var(--ink-soft);font-size:11px;}
.toggle-row{display:flex;align-items:center;justify-content:space-between;border:1px solid var(--line);
  border-radius:4px;padding:12px 14px;cursor:pointer;}
.convert-btn{background:var(--accent);color:#fff;border:none;border-radius:4px;padding:15px;
  font-size:15px;font-weight:700;cursor:pointer;}
.convert-btn:hover:not(:disabled){background:var(--accent-dark);}
.convert-btn:disabled{background:#c9d0c3;cursor:not-allowed;}
.note{font-size:11px;color:var(--ink-soft);font-family:monospace;}
footer{max-width:960px;margin:40px auto 0;text-align:center;font-family:monospace;font-size:11px;color:var(--ink-soft);}
"#;
