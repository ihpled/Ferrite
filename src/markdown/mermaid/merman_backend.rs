//! Mermaid.js-compatible rendering through the `merman` crate.
//!
//! `merman` ports Mermaid's parsers, dagre layout and SVG output to Rust. This
//! module renders a diagram to SVG with Mermaid's own `default` / `dark` theme,
//! rasterizes it with `resvg` at the current screen density, and paints the
//! result as an egui texture.
//!
//! Text is measured and painted with the same system-font context, so labels
//! fit their boxes. Loading that context scans the system fonts (a few hundred
//! milliseconds), so [`prewarm`] starts it on a background thread at startup.
//!
//! Any failure (unsupported syntax, font loading, rasterization) returns `Err`
//! and the caller falls back to Ferrite's native renderer.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Color32, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2};
use merman::svg::export::{NativeFontContext, NativeFontOptions};
use merman::svg::{SvgPipeline, SvgPipelinePreset};
use merman::{
    MermaidEngine, OperationControl, RenderError, RenderOutput, RenderRequest, Renderer,
    SourcePresentationConfig, SvgEnvironment, SvgRequest,
};
use resvg::{tiny_skia, usvg};

/// Mermaid's default font size; diagrams are scaled by `font_size / 16`.
const MERMAID_BASE_FONT_SIZE: f32 = 16.0;
/// Parsed diagrams kept in memory (SVG trees are small).
const MAX_DIAGRAMS: usize = 64;
/// Rasterized textures kept alive (one per diagram × theme × density step).
const MAX_TEXTURES: usize = 24;
/// Raster density steps per doubling of zoom (limits re-rasterization while zooming).
const DENSITY_STEPS_PER_OCTAVE: f32 = 4.0;

static USE_NATIVE: AtomicBool = AtomicBool::new(false);
static FONTS: OnceLock<Option<NativeFontContext>> = OnceLock::new();
static DIAGRAMS: Mutex<Option<DiagramCache>> = Mutex::new(None);
static TEXTURES: Mutex<VecDeque<(TextureKey, TextureHandle)>> = Mutex::new(VecDeque::new());

/// Select the renderer: `true` keeps Ferrite's native renderer, `false` uses merman.
pub fn set_use_native_renderer(native: bool) {
    USE_NATIVE.store(native, Ordering::Relaxed);
}

/// Whether diagrams should go through merman first.
pub fn enabled() -> bool {
    !USE_NATIVE.load(Ordering::Relaxed)
}

/// Load the system font context on a background thread.
///
/// Safe to call every frame: only the first call spawns the thread.
pub fn prewarm() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("merman-fonts".into())
            .spawn(|| {
                fonts();
            });
    });
}

fn fonts() -> Option<&'static NativeFontContext> {
    FONTS
        .get_or_init(|| {
            let started = std::time::Instant::now();
            match NativeFontOptions::default()
                .with_system_fonts(true)
                .load(&OperationControl::new())
            {
                Ok(context) => {
                    log::info!(
                        "merman: loaded {} font faces in {:?}",
                        context.face_count(),
                        started.elapsed()
                    );
                    Some(context)
                }
                Err(err) => {
                    log::warn!("merman: system fonts unavailable ({err}); using estimated metrics");
                    None
                }
            }
        })
        .as_ref()
}

/// Why merman could not render a diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermanError {
    /// The source has a Mermaid syntax error (Mermaid.js would reject it too).
    Syntax(String),
    /// Anything else (fonts, SVG, limits, panics): use the native renderer instead.
    Other(String),
}

impl std::fmt::Display for MermanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MermanError::Syntax(msg) | MermanError::Other(msg) => f.write_str(msg),
        }
    }
}

/// A rendered diagram: the parsed SVG tree and its natural size in CSS pixels.
struct Diagram {
    tree: usvg::Tree,
    size: Vec2,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct DiagramKey {
    source_hash: u64,
    dark: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TextureKey {
    diagram: DiagramKey,
    /// Raster pixels per CSS pixel, quantized (bits of the f32).
    density_bits: u32,
}

#[derive(Default)]
struct DiagramCache {
    entries: HashMap<DiagramKey, Result<Arc<Diagram>, MermanError>>,
    order: VecDeque<DiagramKey>,
}

impl DiagramCache {
    fn get(&self, key: &DiagramKey) -> Option<Result<Arc<Diagram>, MermanError>> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: DiagramKey, value: Result<Arc<Diagram>, MermanError>) {
        if self.entries.insert(key, value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > MAX_DIAGRAMS {
            if let Some(old) = self.order.pop_front() {
                self.entries.remove(&old);
            }
        }
    }
}

fn hash_source(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// Render (or fetch from cache) the diagram for `source`.
fn diagram(source: &str, dark: bool) -> Result<Arc<Diagram>, MermanError> {
    let key = DiagramKey {
        source_hash: hash_source(source),
        dark,
    };
    let cached = DIAGRAMS
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().and_then(|cache| cache.get(&key)));
    if let Some(result) = cached {
        return result;
    }

    // merman is designed to be panic-free, but a diagram must never take the
    // editor down: treat a panic like any other render failure.
    let result = std::panic::catch_unwind(|| render_diagram(source, dark))
        .unwrap_or_else(|_| Err(MermanError::Other("merman panicked".to_string())))
        .map(Arc::new);
    if let Err(err) = &result {
        log::debug!("merman: cannot render diagram: {err:?}");
    }
    if let Ok(mut guard) = DIAGRAMS.lock() {
        guard
            .get_or_insert_with(DiagramCache::default)
            .insert(key, result.clone());
    }
    result
}

fn render_diagram(source: &str, dark: bool) -> Result<Diagram, MermanError> {
    let other = |e: &dyn std::fmt::Display| MermanError::Other(e.to_string());
    let fonts = fonts();
    let mut environment = SvgEnvironment::deterministic();
    if let Some(fonts) = fonts {
        environment = environment
            .with_native_font_context(fonts.clone())
            .map_err(|e| other(&e))?;
    }
    let site_config = SourcePresentationConfig::from_value(serde_json::json!({
        "theme": if dark { "dark" } else { "default" },
    }));
    let renderer = Renderer::new()
        .with_presentation_engine(MermaidEngine::new().with_site_config(site_config));
    let request = SvgRequest {
        environment,
        pipeline: Some(SvgPipeline::from_preset(SvgPipelinePreset::ResvgSafe)),
        ..SvgRequest::default()
    };
    let output = renderer
        .render(RenderRequest::svg(source, OperationControl::new(), request))
        .map_err(|e| match e {
            RenderError::Parse(diagnostic) => MermanError::Syntax(diagnostic.to_string()),
            other_error => other(&other_error),
        })?;
    let RenderOutput::Svg(Some(svg)) = output else {
        return Err(MermanError::Other(
            "no Mermaid diagram detected".to_string(),
        ));
    };

    let mut options = usvg::Options::default();
    if let Some(fonts) = fonts {
        fonts.apply_to_usvg_options(&mut options);
    }
    let tree = usvg::Tree::from_str(svg.svg(), &options).map_err(|e| other(&e))?;
    let size = Vec2::new(tree.size().width(), tree.size().height());
    Ok(Diagram { tree, size })
}

/// Whether merman renders `source` (cached; also used for validation).
pub fn check(source: &str) -> Result<(), MermanError> {
    let source_hash = hash_source(source);
    let cached = DIAGRAMS.lock().ok().and_then(|guard| {
        let cache = guard.as_ref()?;
        [false, true]
            .into_iter()
            .find_map(|dark| cache.get(&DiagramKey { source_hash, dark }))
    });
    match cached {
        Some(result) => result.map(|_| ()),
        None => diagram(source, false).map(|_| ()),
    }
}

/// Natural on-screen size (in points) of the diagram, or `None` if merman can't render it.
///
/// The size doesn't depend on the theme, so an already rendered theme is reused.
pub fn natural_size(source: &str, font_size: f32) -> Option<Vec2> {
    let source_hash = hash_source(source);
    let cached = DIAGRAMS.lock().ok().and_then(|guard| {
        let cache = guard.as_ref()?;
        [false, true]
            .into_iter()
            .find_map(|dark| cache.get(&DiagramKey { source_hash, dark }))
    });
    let diagram = match cached {
        Some(result) => result.ok()?,
        None => diagram(source, false).ok()?,
    };
    Some(diagram.size * (font_size / MERMAID_BASE_FONT_SIZE))
}

/// Paint the diagram at the cursor. Returns `Err` (painting nothing) if merman can't render it.
pub fn paint(ui: &mut Ui, source: &str, dark: bool, font_size: f32) -> Result<(), MermanError> {
    let diagram = diagram(source, dark)?;
    let points_per_css_px = font_size / MERMAID_BASE_FONT_SIZE;
    let display_size = diagram.size * points_per_css_px;
    let (rect, _) = ui.allocate_exact_size(display_size, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return Ok(());
    }

    // Physical pixels per point, including inline fit-to-pane and popup zoom
    // layer transforms, so the texture stays crisp at any scale.
    let layer_scale = ui
        .ctx()
        .layer_transform_to_global(ui.layer_id())
        .map(|t| t.scaling)
        .unwrap_or(1.0);
    let px_per_point = ui.ctx().pixels_per_point() * layer_scale;
    let max_side = ui.ctx().input(|i| i.max_texture_side) as f32;
    let density = quantize_density(px_per_point * points_per_css_px)
        .min(max_side / diagram.size.x.max(diagram.size.y).max(1.0));

    let key = TextureKey {
        diagram: DiagramKey {
            source_hash: hash_source(source),
            dark,
        },
        density_bits: density.to_bits(),
    };
    let texture = texture(ui.ctx(), key, &diagram, density).map_err(MermanError::Other)?;
    ui.painter().image(
        texture.id(),
        rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    Ok(())
}

/// Round a density up to the next of [`DENSITY_STEPS_PER_OCTAVE`] steps per doubling.
fn quantize_density(density: f32) -> f32 {
    let density = density.clamp(0.25, 16.0);
    let steps = (density.log2() * DENSITY_STEPS_PER_OCTAVE).ceil();
    2f32.powf(steps / DENSITY_STEPS_PER_OCTAVE)
}

fn texture(
    ctx: &egui::Context,
    key: TextureKey,
    diagram: &Diagram,
    density: f32,
) -> Result<TextureHandle, String> {
    let mut textures = TEXTURES.lock().map_err(|e| e.to_string())?;
    if let Some(pos) = textures.iter().position(|(k, _)| *k == key) {
        let entry = textures.remove(pos).expect("position is in bounds");
        let handle = entry.1.clone();
        textures.push_back(entry);
        return Ok(handle);
    }

    let width = (diagram.size.x * density).ceil().max(1.0) as u32;
    let height = (diagram.size.y * density).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| format!("cannot allocate {width}x{height} pixmap"))?;
    resvg::render(
        &diagram.tree,
        tiny_skia::Transform::from_scale(density, density),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied RGBA, which is what egui expects.
    let image =
        egui::ColorImage::from_rgba_premultiplied([width as usize, height as usize], pixmap.data());
    let handle = ctx.load_texture("mermaid-merman", image, TextureOptions::LINEAR);
    textures.push_back((key, handle.clone()));
    while textures.len() > MAX_TEXTURES {
        textures.pop_front();
    }
    Ok(handle)
}

/// Drop cached diagrams and textures (theme or font changes).
pub fn clear_cache() {
    if let Ok(mut guard) = DIAGRAMS.lock() {
        *guard = None;
    }
    if let Ok(mut textures) = TEXTURES.lock() {
        textures.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantized_density_rounds_up_in_quarter_octaves() {
        assert_eq!(quantize_density(1.0), 1.0);
        assert_eq!(quantize_density(2.0), 2.0);
        let q = quantize_density(1.1);
        assert!(q >= 1.1 && q < 1.2, "{q}");
    }

    #[test]
    fn renders_flowchart_with_subgraph_edge() {
        let source = "flowchart LR\n  subgraph CH[Host]\n    A\n  end\n  X --> CH\n";
        let light = diagram(source, false).expect("merman renders flowchart");
        assert!(light.size.x > 0.0 && light.size.y > 0.0);
        assert!(diagram(source, true).is_ok(), "dark theme renders too");
    }

    #[test]
    fn renders_diagram_types_missing_natively() {
        let source = "quadrantChart\n  title Reach\n  x-axis Low --> High\n  y-axis Low --> High\n  A: [0.3, 0.6]\n";
        assert!(diagram(source, false).is_ok());
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(diagram("this is not mermaid", false).is_err());
    }

    #[test]
    fn incomplete_edge_is_a_syntax_error() {
        let result = check("flowchart LR\n  A --> B\n  D --> A -->|\n");
        assert!(
            matches!(result, Err(MermanError::Syntax(_))),
            "expected syntax error, got {result:?}"
        );
    }
}
