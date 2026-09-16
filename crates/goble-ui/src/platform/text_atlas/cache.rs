//! Memoized text measurement.
//!
//! Both measurement entry points below a run's box are fontdue line-breaking
//! passes: [`super::fonts::measure_text_family`] lays the run out once to size
//! its box, and [`super::fonts::advance_width`] lays it out again to budget the
//! room it takes up, which a paragraph asks for once per word. The tree is
//! rebuilt from scratch every frame, so without a cache a transcript re-ran
//! both for every visible run on every redraw. The answers only depend on the
//! run and its face, so they are cached here under the exact numbers they were
//! measured with.
//!
//! The cache is thread-local: the UI measures on one thread, and the font set
//! behind the measurement is already a process-global `OnceLock`, so this is
//! the same lifetime story with no plumbing through `AppContext`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::geometry::Vector2F;
use crate::theme::FontFamily;

use super::fonts::FontWeight;

/// How many runs each cache keeps before it is emptied.
///
/// A transcript's visible runs are hundreds, so the cap is never reached while
/// the window shows one screen; it exists so a program that measures unbounded
/// distinct strings (a terminal printing new output forever) cannot grow the
/// maps without limit. Overflow drops everything, which costs one frame of
/// re-measurement and needs no eviction bookkeeping.
const MAX_ENTRIES: usize = 4096;

/// Bumped whenever the answers stop being valid for the current render setup
/// (a new render scale, hence a new atlas). It is part of every key, so an
/// entry from before the bump can never be read after it.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Every number that can change the measured size of a run. The font atlas is
/// keyed the same way (see [`super::atlas::text_key`]); the two must agree, or
/// a box would be sized for one face and drawn with another.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct SizeKey {
    text: String,
    /// Sizes and widths as `f32` bits: not rounded, because the line breaks
    /// fontdue picks are a function of the exact number.
    font_size: u32,
    line_height: u32,
    max_width: u32,
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
    generation: u64,
}

impl SizeKey {
    pub(super) fn new(
        text: &str,
        font_size: f32,
        line_height: f32,
        max_width: f32,
        weight: FontWeight,
        family: FontFamily,
        italic: bool,
    ) -> Self {
        Self {
            text: text.to_string(),
            font_size: font_size.to_bits(),
            line_height: line_height.to_bits(),
            max_width: max_width.to_bits(),
            weight,
            family,
            italic,
            generation: GENERATION.load(Ordering::Relaxed),
        }
    }
}

/// Every number that can change a single-line advance. A run laid out on one
/// line has no wrap width and no line height to affect it, so those are not
/// part of this key.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct AdvanceKey {
    text: String,
    font_size: u32,
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
    generation: u64,
}

impl AdvanceKey {
    pub(super) fn new(
        text: &str,
        font_size: f32,
        weight: FontWeight,
        family: FontFamily,
        italic: bool,
    ) -> Self {
        Self {
            text: text.to_string(),
            font_size: font_size.to_bits(),
            weight,
            family,
            italic,
            generation: GENERATION.load(Ordering::Relaxed),
        }
    }
}

/// What the caches have been asked and answered, for measuring the effect of
/// memoizing rather than guessing it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeasureCacheStats {
    /// Measurements served from a cache.
    pub hits: u64,
    /// Measurements that had to run the fontdue pass.
    pub misses: u64,
    /// Runs the size cache is holding.
    pub entries: usize,
    /// Runs the advance cache is holding.
    pub advance_entries: usize,
}

#[derive(Default)]
struct Cache {
    sizes: HashMap<SizeKey, Vector2F>,
    advances: HashMap<AdvanceKey, f32>,
    stats: MeasureCacheStats,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// The cached size of `key`, or `None` when it has not been measured since the
/// last invalidation.
pub(super) fn lookup_size(key: &SizeKey) -> Option<Vector2F> {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        match cache.sizes.get(key).copied() {
            Some(size) => {
                cache.stats.hits += 1;
                Some(size)
            }
            None => {
                cache.stats.misses += 1;
                None
            }
        }
    })
}

/// Remember a size. A full cache is emptied first.
pub(super) fn store_size(key: SizeKey, size: Vector2F) {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.sizes.len() >= MAX_ENTRIES {
            cache.sizes.clear();
        }
        cache.sizes.insert(key, size);
    });
}

/// The cached advance of `key`, or `None` when it has not been measured since
/// the last invalidation.
pub(super) fn lookup_advance(key: &AdvanceKey) -> Option<f32> {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        match cache.advances.get(key).copied() {
            Some(advance) => {
                cache.stats.hits += 1;
                Some(advance)
            }
            None => {
                cache.stats.misses += 1;
                None
            }
        }
    })
}

/// Remember an advance. A full cache is emptied first.
pub(super) fn store_advance(key: AdvanceKey, advance: f32) {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.advances.len() >= MAX_ENTRIES {
            cache.advances.clear();
        }
        cache.advances.insert(key, advance);
    });
}

/// Drop every cached measurement, for this thread and every other.
///
/// Called when the render scale changes: the atlas is refilled at the new
/// scale then, and a size measured against the old one must not be reused.
/// (fontdue's metrics are scale-free, since elements measure in logical points,
/// so this is the conservative side of the question rather than a strict
/// requirement — it costs one frame of re-measurement per zoom/display change.)
pub fn invalidate_text_measure_cache() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.sizes.clear();
        cache.advances.clear();
    });
}

/// What this thread's measurement cache has done so far.
pub fn measure_cache_stats() -> MeasureCacheStats {
    CACHE.with(|cache| {
        let cache = cache.borrow();
        MeasureCacheStats {
            entries: cache.sizes.len(),
            advance_entries: cache.advances.len(),
            ..cache.stats
        }
    })
}
