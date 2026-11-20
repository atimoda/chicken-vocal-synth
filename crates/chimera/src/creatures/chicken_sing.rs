//! The chicken sings: a melody's syllables (the same [`SungNote`]s the human voice sings, from any
//! language front end) turned into a score of call keys for the [`chicken`](super::chicken).
//!
//! A chicken has no lips, tongue or teeth, so each sound is mapped onto what it does have:
//!
//! - **vowels** set the beak gape and the throat (oropharynx) volume: "ee" a narrow gape and a
//!   small throat (bright), "ah" a wide gape, "oo" a nearly closed beak over a big throat (dark).
//!   A diphthong moves to its second vowel late in the note.
//! - **consonants** close one of the two valves it can close: lip sounds (b, p, m, f, v, w) close
//!   the beak; back sounds (k, g, ng, h) close the larynx; the rest (t, d, s, n, l, r…) narrow
//!   both a little. Stops close fully (air-sac pressure builds and bursts out on release: the
//!   "b" of "buk", the "k" of "kaak"); fricatives open the breath noise; voiceless sounds stop the
//!   syrinx; nasals stay voiced behind a closed beak (there is no nose to let them out).
//! - each note starts with a pressure accent that decays: the chicken's swell ("bu-KAAAK"); its
//!   pitch scoops up into the note, since pitch rides on the breath.
//! - words are cut apart (the gate dips) the way a chicken's calls are; phrases breathe.
//!
//! Onset consonants land before the beat, as the human singer's do.

use super::types::{CallKey, Curve};
use crate::math::{js_max, js_min, midi_to_hz};
use crate::phonetics::sing::SungNote;
use crate::phonetics::{Segment, Syllable};
use serde::{Deserialize, Serialize};

/// How the chicken sings (all but `bpm` default when left out of JSON).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChickenSingOptions {
    pub bpm: f64,
    /// semitones added to every note
    pub shift: f64,
    /// chick 0 ↔ hen 0.6 ↔ rooster 1
    pub size: f64,
    /// air-sac pressure between accents (kPa)
    pub pressure: f64,
    /// 0..1, the syrinx's rasp
    pub harsh: f64,
    /// 0..1, jitter and detune
    pub rough: f64,
    /// 0..1: how far apart words are cut (0 legato, 1 clucky staccato)
    pub staccato: f64,
    /// plan time of the first note's beat (its onset consonants come before)
    pub t0: f64,
}

impl Default for ChickenSingOptions {
    fn default() -> Self {
        ChickenSingOptions { bpm: 100.0, shift: 0.0, size: 0.6, pressure: 1.8, harsh: 0.5, rough: 0.3, staccato: 0.6, t0: 0.3 }
    }
}

/// The articulation the chicken holds for a sound.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shape {
    gape: f64,
    throat: f64,
    larynx: f64,
    voice: f64,
    breath: f64,
}

impl Shape {
    const fn vowel(gape: f64, throat: f64) -> Shape {
        Shape { gape, throat, larynx: 1.0, voice: 1.0, breath: 0.0 }
    }
    fn keys(&self) -> [(&'static str, f64); 5] {
        [("gape", self.gape), ("throat", self.throat), ("larynx", self.larynx), ("voice", self.voice), ("breath", self.breath)]
    }
}

/// a neutral, half-open "uh"
const SCHWA: Shape = Shape::vowel(0.5, 1.8);

/// The beak and throat for one vowel symbol (the first vowel letter of an IPA string).
fn vowel_char(c: char) -> Option<Shape> {
    Some(match c {
        'i' | 'ɪ' | 'y' | 'ʏ' | 'ɨ' | 'ʉ' => Shape::vowel(0.32, 0.55),
        'e' | 'ɛ' | 'ø' | 'œ' => Shape::vowel(0.55, 0.85),
        'æ' | 'a' | 'ɐ' | 'ɶ' => Shape::vowel(0.95, 1.0),
        'ɑ' | 'ʌ' | 'ɒ' => Shape::vowel(1.0, 1.7),
        'ɔ' | 'o' | 'ɤ' => Shape::vowel(0.6, 2.8),
        'u' | 'ʊ' | 'ɯ' => Shape::vowel(0.25, 3.8),
        'ə' | 'ɜ' | 'ɝ' | 'ɚ' | 'ɘ' | 'ɵ' => SCHWA,
        _ => return None,
    })
}

/// A vowel's main shape and, for a diphthong ("aɪ", "oʊ"), the shape it glides to.
fn vowel(seg: &Segment) -> (Shape, Option<Shape>) {
    let shapes: Vec<Shape> = seg.ipa.chars().filter_map(vowel_char).collect();
    match shapes.as_slice() {
        [] => (SCHWA, None),
        [v] => (*v, None),
        [v, .., w] => (*v, if w != v { Some(*w) } else { None }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Manner {
    Stop,
    Fricative,
    Nasal,
    Approximant,
}

/// A consonant: what the chicken closes, and for how long (s).
fn consonant(seg: &Segment) -> (Shape, Manner, f64) {
    let ipa = seg.ipa.as_str();
    let c = ipa.chars().next().unwrap_or('ə');
    let voiceless = matches!(c, 'p' | 't' | 'k' | 'c' | 'q' | 'ʔ' | 'f' | 's' | 'ʃ' | 'θ' | 'x' | 'h' | 'χ' | 'ç' | 'ɸ' | 'ʍ' | 'ɕ' | 'ʂ');
    let manner = match c {
        'p' | 'b' | 't' | 'd' | 'k' | 'g' | 'ɡ' | 'c' | 'ɟ' | 'q' | 'ɢ' | 'ʔ' | 'ʈ' | 'ɖ' => Manner::Stop,
        'm' | 'n' | 'ŋ' | 'ɲ' | 'ɳ' | 'ɴ' => Manner::Nasal,
        'f' | 'v' | 's' | 'z' | 'ʃ' | 'ʒ' | 'θ' | 'ð' | 'x' | 'ɣ' | 'h' | 'ɦ' | 'χ' | 'ʁ' | 'ç' | 'ɸ' | 'β' | 'ɕ' | 'ʑ' | 'ʂ' | 'ʐ' => Manner::Fricative,
        _ => Manner::Approximant,
    };
    let labial = matches!(c, 'p' | 'b' | 'm' | 'f' | 'v' | 'w' | 'ɸ' | 'β' | 'ʍ' | 'ʋ');
    let back = matches!(c, 'k' | 'g' | 'ɡ' | 'ŋ' | 'x' | 'ɣ' | 'h' | 'ɦ' | 'ʔ' | 'q' | 'ɢ' | 'χ' | 'ʁ' | 'ɴ');
    // how far the closing valve closes
    let close = match manner {
        Manner::Stop | Manner::Nasal => 0.0,
        Manner::Fricative => 0.18,
        Manner::Approximant => 0.5,
    };
    let mut s = Shape { gape: 0.35, throat: 1.6, larynx: 1.0, voice: if voiceless { 0.0 } else { 1.0 }, breath: 0.0 };
    if labial {
        s.gape = close * 0.4;
    } else if back {
        s.larynx = close;
    } else {
        // tongue sounds: a chicken's tongue can't seal, so narrow both valves part-way
        s.larynx = js_max(close, 0.15);
        s.gape = js_max(0.12, close * 0.5);
    }
    match manner {
        Manner::Stop => {
            // a voiced stop keeps a little voicing (the pressure behind the closure still rises)
            if !voiceless {
                s.voice = 0.35;
            }
        }
        Manner::Fricative => {
            s.breath = 1.0;
            if !voiceless {
                s.voice = 0.5;
            }
        }
        Manner::Nasal => s.voice = 0.8,
        Manner::Approximant => match c {
            'w' | 'ʍ' => s = Shape { gape: 0.15, throat: 3.6, larynx: 1.0, voice: 1.0, breath: 0.0 },
            'j' => s = Shape { gape: 0.28, throat: 0.5, larynx: 1.0, voice: 1.0, breath: 0.0 },
            _ => {}
        },
    }
    let len = match manner {
        Manner::Stop => 0.055,
        Manner::Fricative => 0.085,
        Manner::Nasal => 0.065,
        Manner::Approximant => 0.05,
    };
    (s, manner, len)
}

/// Total length of a consonant cluster (s), squeezed to fit `room`.
fn cluster(segs: &[Segment], room: f64) -> (Vec<(Shape, Manner, f64)>, f64) {
    let mut c: Vec<_> = segs.iter().map(consonant).collect();
    let total: f64 = c.iter().map(|x| x.2).sum();
    let k = if total > room && total > 0.0 { room / total } else { 1.0 };
    for x in &mut c {
        x.2 *= k;
    }
    (c, total * k)
}

fn push(keys: &mut Vec<CallKey>, t: f64, shape: &Shape, extra: &[(&'static str, f64)], curve: Option<Curve>) {
    let mut v: Vec<(&'static str, f64)> = shape.keys().to_vec();
    v.extend_from_slice(extra);
    let mut k = CallKey::new(t, &v);
    if let Some(c) = curve {
        k = k.curve(c);
    }
    keys.push(k);
}

/// Consonants from `a`, each held then released toward `then` (written by the next key).
fn consonants(keys: &mut Vec<CallKey>, mut t: f64, c: &[(Shape, Manner, f64)]) {
    for (s, manner, len) in c {
        // close quickly, hold, and leave the last moment for the release
        let hold = if *manner == Manner::Stop { 0.8 } else { 0.7 };
        push(keys, t + js_min(0.012, len * 0.25), s, &[], None);
        push(keys, t + len * hold, s, &[], None);
        t += len;
    }
}

/// The score for a melody: keys for the chicken's params and its gate, from `t = 0`.
pub fn sing_keys(notes: &[SungNote], o: &ChickenSingOptions) -> Vec<CallKey> {
    let beat = 60.0 / js_max(20.0, o.bpm);
    let p = o.pressure;
    let mut keys = vec![CallKey::new(
        0.0,
        &[("gate", 0.0), ("size", o.size), ("harsh", o.harsh), ("rough", o.rough), ("tension", 0.45), ("pressure", p), ("larynx", 1.0), ("voice", 1.0), ("breath", 0.0)],
    )];
    let melisma = |s: &Syllable| s.melisma == Some(true);
    let mut t = o.t0;
    // the gate is open (breath flowing) from the previous note
    let mut flowing = false;
    // the syllable whose vowel is being held through melisma notes, and its shapes
    let mut held: Option<(&Syllable, Shape, Option<Shape>)> = None;
    // where the previous note's keys end (onsets must not reach back past it), and its pitch
    let mut free = 0.0;
    let mut prev_hz = 0.0;
    for (i, n) in notes.iter().enumerate() {
        let dur = n.beats * beat;
        let (ts, te) = (t, t + dur);
        t = te;
        if n.midi <= 0.0 {
            // a rest (midi <= 0, as in `render notes`): the note before it ended its phrase
            continue;
        }
        let hz = midi_to_hz(n.midi + o.shift);
        let s = &n.syl;
        let next_melisma = notes.get(i + 1).is_some_and(|m| m.midi > 0.0 && melisma(&m.syl));
        // a breath follows: the phrase ends, a rest comes, or the song does
        let breath = s.phrase_end || notes.get(i + 1).is_none_or(|m| m.midi <= 0.0);
        let (main, glide) = if melisma(s) {
            match held {
                Some((_, m, g)) => (g.unwrap_or(m), None),
                None => vowel(&s.nucleus),
            }
        } else {
            vowel(&s.nucleus)
        };
        // the syllable that owns this note's coda and word/phrase end: the first note of a melisma
        let owner = if melisma(s) { held.map(|h| h.0).unwrap_or(s) } else { s };
        if !melisma(s) {
            held = Some((s, main, glide));
            // onset consonants, before the beat (at most 0.3 of the note before, 0.2 s)
            let (c, len) = cluster(&s.onset, js_max(0.0, js_min(0.2, ts - free - 0.035)));
            let a = ts - len;
            if !flowing {
                // breathe in, then open the air sacs at the first closure: pressure builds behind it
                keys.push(CallKey::new(a - 0.03, &[("gate", 0.0)]));
                keys.push(CallKey::new(a, &[("gate", 1.0)]));
            } else {
                keys.push(CallKey::new(a, &[("gate", 1.0)]));
            }
            consonants(&mut keys, a, &c);
            // the vowel: pitch scoops up, pressure accent decays
            let from = if c.is_empty() && !flowing { hz * 0.88 } else { hz * 0.93 };
            push(&mut keys, ts, &main, &[("gate", 1.0), ("pitch", from), ("pressure", p * 1.45)], Some(Curve::Step));
            keys.push(CallKey::new(ts + js_min(0.06, dur * 0.3), &[("pitch", hz)]).curve(Curve::Exp));
            keys.push(CallKey::new(ts + js_min(0.18, dur * 0.6), &[("pressure", p)]));
        } else {
            // a held vowel moves to the new pitch (a slur)
            keys.push(CallKey::new(ts, &[("pitch", prev_hz)]));
            keys.push(CallKey::new(ts + js_min(0.05, dur * 0.3), &[("pitch", hz)]).curve(Curve::Exp));
            keys.push(CallKey::new(ts, &[("pressure", p * 1.15)]));
            keys.push(CallKey::new(ts + js_min(0.15, dur * 0.5), &[("pressure", p)]));
        }
        flowing = true;
        prev_hz = hz;
        free = ts;
        if next_melisma {
            continue;
        }
        // the end of the syllable: coda, then a cut between words or a breath at a phrase end
        let breath = breath || owner.phrase_end;
        let gap = if breath {
            js_min(0.08, dur * 0.25)
        } else if owner.word_end {
            js_min(0.05, dur * 0.2) * o.staccato
        } else {
            0.0
        };
        let (c, len) = cluster(&owner.coda, js_max(0.0, dur * 0.45));
        let ve = te - gap - len;
        // the vowel holds (a diphthong glides in its last third)
        if let Some(g) = glide {
            let at = js_max(ts + 0.05, ve - js_min(0.15, (ve - ts) * 0.4));
            push(&mut keys, at, &main, &[], None);
            push(&mut keys, ve, &g, &[], None);
        } else {
            push(&mut keys, js_max(ts + 0.02, ve - 0.01), &main, &[], None);
        }
        consonants(&mut keys, ve, &c);
        let end = te - gap;
        if breath {
            keys.push(CallKey::new(end, &[("gate", 1.0), ("pitch", hz)]));
            keys.push(CallKey::new(end + 0.035, &[("gate", 0.0), ("pitch", hz * 0.85)]).curve(Curve::Exp));
            flowing = false;
        } else if gap > 0.0 {
            keys.push(CallKey::new(end, &[("gate", 1.0)]));
            keys.push(CallKey::new(end + gap * 0.6, &[("gate", 1.0 - 0.8 * o.staccato)]));
        }
        // after the last consonant, back to a neutral shape for the next onset to move from
        if !c.is_empty() {
            push(&mut keys, end + js_min(0.02, gap), &SCHWA, &[], None);
        }
        free = if flowing { end + gap * 0.6 } else { end + 0.035 };
    }
    if flowing {
        keys.push(CallKey::new(t, &[("gate", 1.0)]));
        keys.push(CallKey::new(t + 0.04, &[("gate", 0.0)]));
    }
    keys.push(CallKey::new(t + 0.1, &[("gate", 0.0)]));
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(ipa: &str) -> Segment {
        Segment { ipa: ipa.into(), ..Default::default() }
    }

    fn syl(onset: &[&str], v: &str, coda: &[&str], word_end: bool, phrase_end: bool) -> Syllable {
        Syllable { text: v.into(), onset: onset.iter().map(|s| seg(s)).collect(), nucleus: seg(v), coda: coda.iter().map(|s| seg(s)).collect(), word_end, phrase_end, ..Default::default() }
    }

    #[test]
    fn buk_closes_the_beak_then_the_larynx() {
        let notes = vec![SungNote { midi: 60.0, beats: 1.0, syl: syl(&["b"], "ʌ", &["k"], true, true) }];
        let keys = sing_keys(&notes, &ChickenSingOptions::default());
        let at = |name: &str, x: f64| keys.iter().any(|k| k.v.0.iter().any(|(n, v)| *n == name && (*v - x).abs() < 1e-9));
        assert!(at("gape", 0.0), "b closes the beak");
        assert!(at("larynx", 0.0), "k closes the larynx");
        assert!(at("voice", 0.0), "k is voiceless");
        // the score is in time order once sorted, starts silent and ends silent
        let last = keys.iter().max_by(|a, b| a.t.total_cmp(&b.t)).unwrap();
        assert_eq!(last.v.0, vec![("gate", 0.0)]);
    }

    #[test]
    fn vowels_set_gape_and_throat() {
        let (ee, _) = vowel(&seg("i"));
        let (ah, _) = vowel(&seg("ɑ"));
        let (oo, _) = vowel(&seg("u"));
        assert!(ee.gape < ah.gape && oo.gape < ah.gape);
        assert!(ee.throat < ah.throat && ah.throat < oo.throat);
        let (a, g) = vowel(&seg("aɪ"));
        assert_eq!(a, vowel_char('a').unwrap());
        assert_eq!(g, vowel_char('ɪ'));
    }
}
