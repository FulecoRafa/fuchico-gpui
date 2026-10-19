//! The original's renderer, [pele](https://pele.run) 1.5.2, running in an
//! embedded QuickJS engine (rquickjs).
//!
//! pele is synchronous and dependency-free, so the unmodified IIFE bundle
//! (`vendor/pele/pele.global.js`) runs as-is. Text is measured by
//! [`crate::measure`] through a `measurer` callback, standing in for the
//! browser canvas the original lays out with.
//!
//! One engine lives on a dedicated worker thread (QuickJS runtimes are not
//! `Send`); [`render`] posts a request and waits for the reply. Loading the
//! bundle takes ~50 ms once; a render is typically well under 5 ms.

use crate::measure;
use rquickjs::{Context, Function, Object, Runtime};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const PELE_VERSION: &str = "1.5.2";
const PELE_JS: &str = include_str!("../vendor/pele/pele.global.js");

/// Hard cap for one render; pele has its own input limits, this is only a
/// guard against pathological input spinning the engine.
const RENDER_TIMEOUT: Duration = Duration::from_secs(5);
const MEMORY_LIMIT: usize = 256 << 20;

const SHIM: &str = r#"
globalThis.__peleRender = function (src, sans, mono) {
  try {
    const r = pele.render(src, {
      fontFamily: sans || undefined,
      fontFamilyMono: mono || undefined,
      measurer: { width: (t, s, st) => __peleMeasure(t, s, st | 0) },
    });
    return { ok: true, svg: r.svg, width: r.width, height: r.height, type: r.type };
  } catch (e) {
    return { ok: false, message: (e && e.message !== undefined) ? String(e.message) : String(e) };
  }
};
"#;

/// A successful render.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    /// pele's SVG, still painted from `var(--pele-*)`.
    pub svg: String,
    pub width: f64,
    pub height: f64,
    /// pele's diagram type (`flowchart`, `sequence`, `gitGraph`, ...).
    pub kind: String,
}

struct Request {
    source: String,
    font_sans: String,
    font_mono: String,
    reply: mpsc::Sender<Result<Rendered, String>>,
}

static DEADLINE_MS: AtomicU64 = AtomicU64::new(u64::MAX);

fn epoch() -> Instant {
    static E: OnceLock<Instant> = OnceLock::new();
    *E.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    epoch().elapsed().as_millis() as u64
}

fn worker(rx: mpsc::Receiver<Request>) {
    let init = || -> rquickjs::Result<(Runtime, Context)> {
        let rt = Runtime::new()?;
        rt.set_memory_limit(MEMORY_LIMIT);
        rt.set_max_stack_size(8 << 20);
        rt.set_interrupt_handler(Some(Box::new(|| {
            now_ms() > DEADLINE_MS.load(Ordering::Relaxed)
        })));
        let ctx = Context::full(&rt)?;
        ctx.with(|ctx| -> rquickjs::Result<()> {
            ctx.eval::<(), _>(PELE_JS)?;
            let measure = Function::new(ctx.clone(), |text: String, size: f64, style: u32| {
                measure::width(&text, size, style)
            })?;
            ctx.globals().set("__peleMeasure", measure)?;
            ctx.eval::<(), _>(SHIM)?;
            Ok(())
        })?;
        Ok((rt, ctx))
    };
    let engine = init();
    // Also index the fonts now, off the UI thread.
    let _ = crate::fonts::fontdb();
    for req in rx {
        let result = match &engine {
            Err(e) => Err(format!("Diagram renderer failed to start: {e}")),
            Ok((_rt, ctx)) => {
                measure::set_families(&req.font_sans, &req.font_mono);
                DEADLINE_MS.store(
                    now_ms() + RENDER_TIMEOUT.as_millis() as u64,
                    Ordering::Relaxed,
                );
                let r = ctx.with(|ctx| -> Result<Rendered, String> {
                    let f: Function = ctx
                        .globals()
                        .get("__peleRender")
                        .map_err(|e| e.to_string())?;
                    let out: Object = f
                        .call((
                            req.source.as_str(),
                            req.font_sans.as_str(),
                            req.font_mono.as_str(),
                        ))
                        .map_err(|e| match e {
                            rquickjs::Error::Exception => {
                                let ex = ctx.catch();
                                ex.as_exception()
                                    .and_then(|x| x.message())
                                    .unwrap_or_else(|| "The diagram took too long to render".into())
                            }
                            e => e.to_string(),
                        })?;
                    let get = |k: &str| -> Result<rquickjs::Value, String> {
                        out.get(k).map_err(|e| e.to_string())
                    };
                    if get("ok")?.as_bool() != Some(true) {
                        let m: String = out.get("message").unwrap_or_default();
                        return Err(m);
                    }
                    Ok(Rendered {
                        svg: out.get("svg").map_err(|e| e.to_string())?,
                        width: out.get("width").unwrap_or(0.0),
                        height: out.get("height").unwrap_or(0.0),
                        kind: out.get("type").unwrap_or_default(),
                    })
                });
                DEADLINE_MS.store(u64::MAX, Ordering::Relaxed);
                // Free per-render garbage now rather than at the next alloc.
                if let Ok((rt, _)) = &engine {
                    rt.run_gc();
                }
                r
            }
        };
        let _ = req.reply.send(result);
    }
}

fn sender() -> &'static Mutex<Option<mpsc::Sender<Request>>> {
    static TX: Mutex<Option<mpsc::Sender<Request>>> = Mutex::new(None);
    &TX
}

fn spawn() -> mpsc::Sender<Request> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("pele".into())
        .stack_size(32 << 20)
        .spawn(move || worker(rx))
        .expect("spawn pele worker");
    tx
}

fn get_sender() -> mpsc::Sender<Request> {
    let mut g = sender().lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(spawn).clone()
}

/// Starts the engine in the background so the first diagram does not wait
/// for the bundle to load.
pub fn warm_up() {
    let _ = get_sender();
}

/// Renders `source` with pele, laying text out in the given CSS
/// `font-family` lists. `Err` carries pele's message (a `PeleError`: line
/// and caret-marked snippet included), shown as-is like the original.
pub fn render(source: &str, font_sans: &str, font_mono: &str) -> Result<Rendered, String> {
    for attempt in 0..2 {
        let (reply, rx) = mpsc::channel();
        let req = Request {
            source: source.to_string(),
            font_sans: font_sans.to_string(),
            font_mono: font_mono.to_string(),
            reply,
        };
        if get_sender().send(req).is_ok()
            && let Ok(r) = rx.recv()
        {
            return r;
        }
        // The worker died (a panic in a host callback): start a fresh one.
        *sender().lock().unwrap_or_else(|e| e.into_inner()) = None;
        if attempt == 1 {
            break;
        }
    }
    Err("The diagram renderer failed on this diagram".into())
}
