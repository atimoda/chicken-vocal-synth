//! The chicken: its pitch, its calls, its body sizes, and singing a melody as a score.
//! (It came after the TypeScript port, so the golden fixtures don't cover it.)

use chimera::analysis::pitch::{YinOpts, yin_pitch};
use chimera::creatures::chicken_sing::{ChickenSingOptions, sing_keys};
use chimera::creatures::{CreatureVoice, creature_by_id};
use chimera::phonetics::sing::SungNote;
use chimera::phonetics::{Segment, Syllable};

const FS: f64 = 48000.0;

fn voice(params: &[(&str, f64)]) -> CreatureVoice {
    chimera::creatures::init();
    let def = creature_by_id("chicken").expect("chicken");
    let init: Vec<(usize, f64)> = params.iter().map(|(k, v)| (def.param_index(k).unwrap(), *v)).collect();
    CreatureVoice::new(def, FS, &init)
}

fn render(v: &mut CreatureVoice, sec: f64, block: usize) -> Vec<f32> {
    let mut out = vec![0f32; (sec * FS) as usize];
    for b in out.chunks_mut(block) {
        v.render(b);
    }
    out
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

fn pitch_of(x: &[f32]) -> f64 {
    let frame: Vec<f64> = x.iter().map(|v| *v as f64).collect();
    yin_pitch(&frame, FS, YinOpts { min_f0: Some(80.0), max_f0: Some(6000.0), ..Default::default() }).f0
}

#[test]
fn the_breath_sounds_at_the_syrinx_pitch_for_every_body() {
    for (size, f0) in [(0.0, 3200.0), (0.6, 380.0), (1.0, 550.0)] {
        let mut v = voice(&[("size", size), ("pitch", f0)]);
        v.set_gate(1.0);
        let out = render(&mut v, 0.8, 128);
        let tail = &out[out.len() - 4096..];
        let got = pitch_of(tail);
        assert!(out.iter().all(|x| x.is_finite()), "size {size}: non-finite output");
        assert!(rms(tail) > 0.005, "size {size}: silent ({})", rms(tail));
        // pitch rides on the breath: a fully open gate sounds at the set pitch, give or take the
        // pressure's few percent and the jitter
        assert!((got / f0 - 1.0).abs() < 0.06, "size {size}: wanted ~{f0} Hz, measured {got}");
    }
}

#[test]
fn every_call_sounds_and_ends() {
    let mut v = voice(&[]);
    for c in &v.def.calls {
        let id = c.id;
        let len = c.keys.last().unwrap().t + 0.3;
        v.play_call_id(id, None, 1.0);
        let out = render(&mut v, len, 128);
        let peak = out.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(out.iter().all(|x| x.is_finite()), "{id}: non-finite");
        assert!(peak > 0.02 && peak < 1.0, "{id}: peak {peak}");
        assert_eq!(v.active_call(), None, "{id} still playing");
    }
}

fn seg(ipa: &str) -> Segment {
    Segment { ipa: ipa.into(), ..Default::default() }
}

/// "buk buk ba-gawk": b ʌ k | b ʌ k | b ə | g ɔ k
fn bagawk() -> Vec<SungNote> {
    let s = |on: &str, v: &str, coda: &[&str], word_end: bool, phrase_end: bool| Syllable {
        text: format!("{on}{v}"),
        onset: vec![seg(on)],
        nucleus: seg(v),
        coda: coda.iter().map(|c| seg(c)).collect(),
        word_end,
        phrase_end,
        ..Default::default()
    };
    vec![
        SungNote { midi: 67.0, beats: 0.5, syl: s("b", "ʌ", &["k"], true, false) },
        SungNote { midi: 67.0, beats: 0.5, syl: s("b", "ʌ", &["k"], true, false) },
        SungNote { midi: 64.0, beats: 0.5, syl: s("b", "ə", &[], false, false) },
        SungNote { midi: 72.0, beats: 1.5, syl: s("g", "ɔ", &["k"], true, true) },
    ]
}

#[test]
fn a_sung_score_plays_each_note_at_its_pitch_then_ends() {
    let o = ChickenSingOptions { bpm: 120.0, ..Default::default() };
    let keys = sing_keys(&bagawk(), &o);
    let mut v = voice(&[]);
    v.play_score(&keys, 1.0);
    assert_eq!(v.active_call(), Some("sing"));
    let end = keys.iter().map(|k| k.t).fold(0.0, f64::max) + 0.3;
    let out = render(&mut v, end, 128);
    assert!(out.iter().all(|x| x.is_finite()));
    assert_eq!(v.active_call(), None, "the score ends");
    // the long last note ("gawk", G5 = 784 Hz) in the middle of its vowel
    let beat = 0.5;
    let t = o.t0 + 1.5 * beat + 0.75 * beat;
    let i = (t * FS) as usize;
    let got = pitch_of(&out[i..i + 2048]);
    assert!((got / 523.25 - 1.0).abs() < 0.06, "gawk at {got} Hz, wanted C5 523 Hz");
    // the beak closes for the "b" between "buk" and "buk": quieter than the vowels around it
    let around = |t: f64| rms(&out[(t * FS) as usize - 480..(t * FS) as usize + 480]);
    let gap = around(o.t0 + beat * 0.5 - 0.03);
    let vowel = around(o.t0 + beat * 0.5 + 0.1);
    assert!(gap < vowel * 0.7, "b closure {gap} vs vowel {vowel}");
}

#[test]
fn a_sung_score_does_not_depend_on_block_size() {
    let keys = sing_keys(&bagawk(), &ChickenSingOptions::default());
    let mut a = voice(&[]);
    let mut b = voice(&[]);
    a.play_score(&keys, 1.0);
    b.play_score(&keys, 1.0);
    let x = render(&mut a, 2.0, 128);
    let y = render(&mut b, 2.0, 37);
    assert_eq!(x, y);
}

#[test]
fn a_bigger_bird_has_a_longer_trachea_and_lower_resonances() {
    // the same low note through each body: its strongest harmonics follow the trachea's first
    // resonance (≈ 2 kHz in a chick, ≈ 550 Hz in a rooster), so the rooster's waveform crosses
    // zero less often
    let centroid = |size: f64| {
        let mut v = voice(&[("size", size), ("pitch", 250.0), ("harsh", 0.8), ("rough", 0.0)]);
        v.set_gate(1.0);
        let out = render(&mut v, 0.6, 128);
        let x: Vec<f64> = out[out.len() - 8192..].iter().map(|v| *v as f64).collect();
        // zero-crossing rate as a cheap spectral centre
        x.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count() as f64
    };
    let (chick, rooster) = (centroid(0.0), centroid(1.0));
    assert!(rooster < chick * 0.8, "rooster {rooster} vs chick {chick} crossings");
}
