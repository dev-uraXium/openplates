// OpenPlates — Copyright (C) 2026 uraXium
// SPDX-License-Identifier: AGPL-3.0-only

//! Background renderer: decrypts + parses + rasterises charts off the UI thread.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use resvg::tiny_skia;
use resvg::usvg;

use crate::bundle::Bundle;
use eframe::egui;

static FONT_R: &[u8] = include_bytes!("../assets/LiberationSans-Regular.ttf");
static FONT_B: &[u8] = include_bytes!("../assets/LiberationSans-Bold.ttf");
static FONT_I: &[u8] = include_bytes!("../assets/LiberationSans-Italic.ttf");
static FONT_BI: &[u8] = include_bytes!("../assets/LiberationSans-BoldItalic.ttf");

pub enum Job {
    /// Render chart `id` at `scale` × its natural size for display.
    View { id: String, scale: f32, seq: u64 },
    /// Render chart `id` at `scale` and save it as PNG to `path`.
    Export { id: String, scale: f32, path: std::path::PathBuf },
}

pub enum Done {
    View {
        id: String,
        scale: f32,
        seq: u64,
        size: [usize; 2],
        rgba: Vec<u8>,
        natural: [f32; 2],
    },
    Exported(Result<std::path::PathBuf, String>),
    Failed { id: String, seq: u64, err: String },
}

pub struct Renderer {
    tx: Sender<Job>,
    pub rx: Receiver<Done>,
}

fn options() -> usvg::Options<'static> {
    let mut db = usvg::fontdb::Database::new();
    for f in [FONT_R, FONT_B, FONT_I, FONT_BI] {
        db.load_font_data(f.to_vec());
    }
    db.set_sans_serif_family("Liberation Sans");
    db.set_serif_family("Liberation Sans");
    db.set_monospace_family("Liberation Sans");
    let mut opt = usvg::Options::default();
    opt.font_family = "Liberation Sans".into();
    opt.fontdb = Arc::new(db);
    opt
}

fn raster(tree: &usvg::Tree, scale: f32) -> Option<tiny_skia::Pixmap> {
    let s = tree.size();
    let w = (s.width() * scale).ceil().max(1.0) as u32;
    let h = (s.height() * scale).ceil().max(1.0) as u32;
    let mut pm = tiny_skia::Pixmap::new(w, h)?;
    pm.fill(tiny_skia::Color::WHITE);
    resvg::render(tree, tiny_skia::Transform::from_scale(scale, scale), &mut pm.as_mut());
    Some(pm)
}

impl Renderer {
    pub fn new(bundle: Arc<Bundle>, ctx: egui::Context) -> Self {
        let (tx, jobs) = channel::<Job>();
        let (done, rx) = channel::<Done>();
        thread::spawn(move || {
            let opt = options();
            // Small cache of parsed trees (parsing is the slow part for big charts).
            let mut trees: HashMap<String, Arc<usvg::Tree>> = HashMap::new();
            let mut order: Vec<String> = Vec::new();
            let mut get_tree = |id: &str| -> Result<Arc<usvg::Tree>, String> {
                if let Some(t) = trees.get(id) {
                    return Ok(t.clone());
                }
                let svg = bundle.svg(id)?;
                let t = Arc::new(usvg::Tree::from_data(&svg, &opt).map_err(|e| e.to_string())?);
                drop(svg);
                trees.insert(id.to_string(), t.clone());
                order.push(id.to_string());
                if order.len() > 12 {
                    let old = order.remove(0);
                    trees.remove(&old);
                }
                Ok(t)
            };
            while let Ok(mut job) = jobs.recv() {
                // Skip stale view requests: only the newest one matters.
                while let Ok(next) = jobs.try_recv() {
                    match (&job, &next) {
                        (Job::View { .. }, Job::View { .. }) => job = next,
                        _ => {
                            run(&mut get_tree, job, &done);
                            job = next;
                        }
                    }
                }
                run(&mut get_tree, job, &done);
                ctx.request_repaint();
            }
        });
        Self { tx, rx }
    }

    pub fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }
}

fn run(
    get_tree: &mut impl FnMut(&str) -> Result<Arc<usvg::Tree>, String>,
    job: Job,
    done: &Sender<Done>,
) {
    match job {
        Job::View { id, scale, seq } => match get_tree(&id) {
            Ok(tree) => match raster(&tree, scale) {
                Some(pm) => {
                    let size = [pm.width() as usize, pm.height() as usize];
                    let natural = [tree.size().width(), tree.size().height()];
                    let _ = done.send(Done::View { id, scale, seq, size, rgba: pm.take(), natural });
                }
                None => {
                    let _ = done.send(Done::Failed { id, seq, err: "chart too large to draw".into() });
                }
            },
            Err(err) => {
                let _ = done.send(Done::Failed { id, seq, err });
            }
        },
        Job::Export { id, scale, path } => {
            let r = get_tree(&id).and_then(|tree| {
                let pm = raster(&tree, scale).ok_or("chart too large to export")?;
                pm.save_png(&path).map_err(|e| e.to_string())?;
                Ok(path)
            });
            let _ = done.send(Done::Exported(r));
        }
    }
}
