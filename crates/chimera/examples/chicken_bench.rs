//! What one chicken voice costs, for running it live inside a game:
//! `cargo run --release -p chimera --example chicken_bench`.
//! Prints the real-time CPU share of one core (singing a song, and holding a note at each internal
//! rate), the per-voice memory, and whether the airway still holds at lower internal rates.
use chimera::creatures::chicken_sing::{ChickenSingOptions, sing_keys};
use chimera::creatures::{CreatureVoice, creature_by_id};
use chimera::phonetics::sing::SungNote;
use chimera::phonetics::songs::DEMO_SONGS;
use chimera::phonetics::{Segment, Syllable};
use chimera::sources::syrinx::SyrinxLabia;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// counts live heap bytes, to measure what a voice holds
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

const FS: f64 = 48000.0;

fn song() -> Vec<SungNote> {
    // Daisy Bell's melody on "buk bah gawk" syllables (no dictionary needed)
    let d = DEMO_SONGS.iter().find(|s| s.name == "Daisy Bell").unwrap();
    let seg = |s: &str| Segment { ipa: s.into(), ..Default::default() };
    let sy = [("b", "ʌ", "k"), ("b", "ɑ", ""), ("g", "ɔ", "k")];
    d.notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let (o, v, c) = sy[i % 3];
            let coda = if c.is_empty() { vec![] } else { vec![seg(c)] };
            SungNote { midi: n[0], beats: n[1], syl: Syllable { text: v.into(), onset: vec![seg(o)], nucleus: seg(v), coda, word_end: true, phrase_end: i % 6 == 5, ..Default::default() } }
        })
        .collect()
}

fn main() {
    chimera::creatures::init();
    let def = creature_by_id("chicken").unwrap();
    let before = LIVE.load(Ordering::Relaxed);
    let mut v = CreatureVoice::new(def, FS, &[]);
    let voice_bytes = LIVE.load(Ordering::Relaxed) - before;
    let keys = sing_keys(&song(), &ChickenSingOptions::default());
    let before = LIVE.load(Ordering::Relaxed);
    v.play_score(&keys, 1.0);
    let score_bytes = LIVE.load(Ordering::Relaxed) - before;
    let secs = keys.iter().map(|k| k.t).fold(0.0, f64::max);
    let n = (secs * FS) as usize / 128;
    let mut out = [0f32; 128];
    let t0 = Instant::now();
    for _ in 0..n {
        v.render(&mut out);
    }
    let dt = t0.elapsed().as_secs_f64();
    let audio = n as f64 * 128.0 / FS;
    println!("singing Daisy Bell ({audio:.1} s of audio, internal {} Hz): {:.1}% of one core, {:.3} ms per 128-sample block (budget 2.667 ms)", v.fs, 100.0 * dt / audio, 1000.0 * dt / n as f64);
    println!("memory: voice {:.1} KB, its sung score {:.1} KB ({} keys)", voice_bytes as f64 / 1024.0, score_bytes as f64 / 1024.0, keys.len());

    // the airway alone, at each internal rate: cost, and whether it holds (finite, not silent)
    for os in [4usize, 3, 2, 1] {
        let fs = FS * os as f64;
        let mut sim = def.create(fs);
        let p: Vec<f64> = def.params.iter().map(|p| p.default).collect();
        let ctrl = 64 * os;
        let mut sum2 = 0.0;
        let mut bad = false;
        let secs = 3.0;
        let t0 = Instant::now();
        let mut k = 0usize;
        while (k as f64) < secs * fs {
            sim.control(&p, 1.0, ctrl as f64 / fs);
            if let Some(net) = sim.net_mut() {
                net.begin_block(ctrl);
            }
            for _ in 0..ctrl {
                let y = sim.sample() * sim.gain();
                if !y.is_finite() || y.abs() > 1e3 {
                    bad = true;
                }
                sum2 += y * y;
            }
            k += ctrl;
        }
        let dt = t0.elapsed().as_secs_f64();
        let rms = (sum2 / k as f64).sqrt();
        let nodes = sim.tubes().iter().map(|(_, t)| t.n).sum::<usize>();
        println!(
            "airway at {:>6} Hz (x{os}): {:5.1}% of one core, {nodes:>3} tube sections, {}",
            fs,
            100.0 * dt / secs,
            if bad { "UNSTABLE (blows up)".to_string() } else { format!("holds, rms {rms:.3}") }
        );
    }

    // the two syrinx oscillators alone at x4 (their RK4 sub-steps)
    let fs = FS * 4.0;
    let mut l = SyrinxLabia::new(fs, 0.0);
    l.f0 = 380.0;
    let t0 = Instant::now();
    let mut s = 0.0;
    for _ in 0..(fs * 3.0) as usize {
        s += l.next();
    }
    let dt = t0.elapsed().as_secs_f64();
    println!("one syrinx side at x4: {:.1}% of one core (x2 for both sides) [{s:.0}]", 100.0 * dt / 3.0);
}
