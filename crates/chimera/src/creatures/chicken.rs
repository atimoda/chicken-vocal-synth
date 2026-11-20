//! Domestic chicken (*Gallus gallus domesticus*): chick, hen and rooster on one body-size control.
//!
//! The airway, from the lungs up:
//!
//! - **Air sacs** drive everything: a chicken has no intrinsic syringeal muscles, so pitch and
//!   loudness both rise with air-sac pressure (the "bu-KAAAK" swell of a cackle).
//! - **Syrinx**: tracheobronchial, at the foot of the trachea. Its vibrating tissue (the lateral
//!   tympaniform membranes and labia, one per bronchus) is modelled by the same Gardner–Mindlin
//!   oscillators the songbird uses, but both sides are locked to one pitch; `rough` detunes them
//!   and jitters the pitch, which gives the harsh, beating timbre of a crow or a squawk.
//! - **Trachea**: a long tube of complete cartilage rings (≈ 4 cm in a chick, 11 cm in a hen,
//!   16 cm in a rooster). Its quarter-wave resonances (≈ 550, 1650, 2750 Hz in a rooster) are the
//!   main "formants" of a chicken. `size` sets its length (and the bird's other dimensions).
//! - **Larynx**: the cranial larynx has no vocal folds; its slit (glottis) narrows and closes,
//!   which is how a chicken cuts a call off and how the sung "k" and "g" are made here.
//! - **Oropharynx** (throat cavity): a Helmholtz-like volume between larynx and beak; swallowing
//!   it larger darkens the sound ("oo", "o"), smaller brightens it ("ee").
//! - **Beak**: a short horn whose gape controls radiation; closing it is the sung "b", "p", "m".
//!
//! Anatomy numbers are textbook ranges (King & McLelland, *Form and Function in Birds*), not
//! measurements of one bird.

use super::build::{TubeArea, TubeOpts, dx_min, fill_areas, tube};
use super::types::{CallDef, CallKey, CreatureDef, CreatureGroup, CreatureSim, Curve, GateTimes, ParamDef};
use crate::airway::network::{AirwayNetwork, NoiseSpec, PortSpec, Tube};
use crate::math::{Drift, cbrt, js_max, js_min, lerp, pow};
use crate::physics::WALL_TRACHEA;
use crate::sources::syrinx::SyrinxLabia;

/// largest opening of one syringeal valve (cm²)
const AMAX: f64 = 0.05;
/// turbulence noise gain (breath through the larynx; half of it at the syrinx)
const NOISE_K: f64 = 0.707e-4;

/// the trachea is built at the chick's length (the finest grid) and only grows from there, so the
/// grid never gets finer than the CFL limit at any `size`
const TRACHEA_MIN: f64 = 4.0;
const TRACHEA_MAX: f64 = 16.0;
const BEAK_MIN: f64 = 1.2;
const BEAK_MAX: f64 = 3.4;

// parameter indices (order of `def().params`)
const PITCH: usize = 0;
const PRESSURE: usize = 1;
const VOICE: usize = 2;
const HARSH: usize = 3;
const TENSION: usize = 4;
const ROUGH: usize = 5;
const BREATH: usize = 6;
const SIZE: usize = 7;
const LARYNX: usize = 8;
const THROAT: usize = 9;
const GAPE: usize = 10;

pub struct Chicken {
    net: AirwayNetwork,
    tubes: Vec<(&'static str, Tube)>,
    trachea: Tube,
    beak: Tube,
    sac: usize,
    v_l: usize,
    v_r: usize,
    larynx: usize,
    oec: usize,
    lab_l: SyrinxLabia,
    lab_r: SyrinxLabia,
    jitter: Drift,
    /// the valves' steady leak (breath without voice), cm²
    leak: f64,
    /// how much the syrinx vibrates (0 = only breath)
    voice: f64,
    /// valve swing (cm²): wider for low notes, as a membrane vibrating slowly swings further
    amax: f64,
    /// the shortest the trachea and beak can be at this rate (they are built at these lengths)
    trachea_min: f64,
    beak_min: f64,
    trachea_areas: Vec<f64>,
    beak_areas: Vec<f64>,
    noise: [NoiseSpec; 2],
}

fn create(fs: f64) -> Box<dyn CreatureSim> {
    let mut net = AirwayNetwork::new(fs);
    // Every tube is built no shorter than its sections can be at this rate (a section under the
    // CFL length rings: see engine.rs `the_voice_holds_for_a_child_at_low_device_rates`). At 96 kHz
    // and up that changes nothing; on a 16 kHz device (32 kHz inside) the chick's beak and trachea
    // come out a little long.
    let floor = |n: f64| n * dx_min(fs) * 1.02;
    let short = TubeOpts { min: Some(2.0), ..Default::default() };
    let bron_len = js_max(1.0, floor(2.0));
    let bron_l = tube(&mut net, "bronchusL", fs, bron_len, TubeArea::Uniform(0.07), short);
    let bron_r = tube(&mut net, "bronchusR", fs, bron_len, TubeArea::Uniform(0.07), short);
    let trachea_min = js_max(TRACHEA_MIN, floor(3.0));
    let trachea = tube(&mut net, "trachea", fs, trachea_min, TubeArea::Uniform(0.15), TubeOpts { wall: Some(WALL_TRACHEA), max: Some(60.0), ..Default::default() });
    let sac = net.add_pressure_node("air sac");
    net.add_port(sac, bron_l.first_node(), PortSpec::new(0.12, 0.5));
    net.add_port(sac, bron_r.first_node(), PortSpec::new(0.12, 0.5));
    let v_l = net.add_port(bron_l.last_node(), trachea.first_node(), PortSpec::new(0.0, 0.1).kinetic(0.5, 1.0));
    let v_r = net.add_port(bron_r.last_node(), trachea.first_node(), PortSpec::new(0.0, 0.1).kinetic(0.5, 1.0));
    let oec = net.add_cavity("oropharynx", 1.5, None);
    let larynx = net.add_port(trachea.last_node(), oec, PortSpec::new(0.12, 0.3).kinetic(0.4, 1.0));
    let beak_min = js_max(BEAK_MIN, floor(2.0));
    let beak = tube(&mut net, "beak", fs, beak_min, TubeArea::Uniform(0.4), short);
    let throat_beak = net.connect(oec, beak.first_node());
    net.add_radiation(beak.last_node());
    net.finalize();
    let noise = [
        // breath through the narrowed larynx (the sung s, h, f)
        NoiseSpec::new(larynx, throat_beak, trachea.branch(trachea.n - 2), 3e-6).band(6000.0, 500.0),
        // turbulence at the syrinx (a squawk's noisy edge)
        NoiseSpec::new(v_l, trachea.branch(1), bron_l.branch(0), 1e-6).band(5000.0, 300.0),
    ];
    let fs_ctrl = fs / 256.0;
    Box::new(Chicken {
        net,
        tubes: vec![("trachea", trachea), ("beak", beak), ("bronchusL", bron_l), ("bronchusR", bron_r)],
        trachea,
        beak,
        sac,
        v_l,
        v_r,
        larynx,
        oec,
        lab_l: SyrinxLabia::new(fs, 0.0),
        lab_r: SyrinxLabia::new(fs, 0.7),
        jitter: Drift::new(fs_ctrl, 9.0, 0x0c4c_6b3e),
        leak: 0.0,
        voice: 1.0,
        amax: AMAX,
        trachea_min,
        beak_min,
        trachea_areas: vec![0.0; trachea.n],
        beak_areas: vec![0.0; beak.n],
        noise,
    })
}

impl CreatureSim for Chicken {
    fn net(&self) -> Option<&AirwayNetwork> {
        Some(&self.net)
    }
    fn net_mut(&mut self) -> Option<&mut AirwayNetwork> {
        Some(&mut self.net)
    }
    fn tubes(&self) -> &[(&'static str, Tube)] {
        &self.tubes
    }
    fn gain(&self) -> f64 {
        2.4e-7
    }
    fn control(&mut self, p: &[f64], gate: f64, _dt: f64) {
        let size = p[SIZE];
        // air-sac pressure, kPa → dyn/cm²
        self.net.set_fixed_pressure(self.sac, p[PRESSURE] * 10000.0 * gate);
        // no syringeal muscles: pitch rides on the breath (+ a small pressure rise), and `rough`
        // adds a wandering jitter of up to ±60 cents
        let jit = pow(2.0, (p[ROUGH] * 60.0 * self.jitter.step()) / 1200.0);
        let f0 = p[PITCH] * (0.86 + 0.14 * js_min(1.0, gate)) * pow(p[PRESSURE] / 1.2, 0.08) * jit;
        let detune = pow(2.0, (p[ROUGH] * 45.0) / 1200.0);
        self.lab_l.f0 = f0;
        self.lab_r.f0 = f0 * detune;
        // `harsh` drives the oscillators toward relaxation pulses, more so with pressure
        let drive = js_min(1.0, p[HARSH] * (0.75 + 0.25 * js_min(2.0, p[PRESSURE] / 1.2)));
        for lab in [&mut self.lab_l, &mut self.lab_r] {
            lab.drive = drive;
            lab.tension = p[TENSION];
            lab.gate = 1.0;
        }
        self.voice = p[VOICE];
        self.amax = AMAX * js_max(0.6, js_min(3.0, 600.0 / f0));
        self.leak = 0.004 + 0.03 * p[BREATH];
        // body: trachea length and bore, throat volume and beak length all scale with size
        let len = js_max(self.trachea_min, lerp(TRACHEA_MIN, TRACHEA_MAX, size));
        let bore = lerp(0.05, 0.2, size);
        let areas = fill_areas(self.trachea, |_| bore, &mut self.trachea_areas);
        self.net.set_tube_targets(self.trachea, areas, Some(len), None);
        let larynx = js_max(0.0, p[LARYNX]) * lerp(0.05, 0.16, size);
        self.net.set_port_target(self.larynx, larynx);
        let throat = p[THROAT] * lerp(0.25, 1.0, size);
        self.net.set_node_area_target(self.oec, throat / cbrt(1.5));
        let gape = p[GAPE];
        let beak_len = js_max(self.beak_min, lerp(BEAK_MIN, BEAK_MAX, size));
        let scale = lerp(0.35, 1.0, size);
        let areas = fill_areas(self.beak, |u| scale * (0.03 + gape * (0.2 + 0.9 * u)), &mut self.beak_areas);
        self.net.set_tube_targets(self.beak, areas, Some(beak_len), None);
        self.noise[0].gain = NOISE_K * (0.04 + 2.0 * p[BREATH]);
        self.noise[1].gain = 0.25 * NOISE_K * p[HARSH] * p[ROUGH] * p[ROUGH];
        self.net.set_noise_sources(&self.noise);
    }
    #[inline]
    fn sample(&mut self) -> f64 {
        let v = self.voice;
        let a = self.leak + self.amax * v * self.lab_l.next();
        self.net.set_port_area(self.v_l, a);
        let a = self.leak + self.amax * v * self.lab_r.next();
        self.net.set_port_area(self.v_r, a);
        self.net.step()
    }
    fn source_state(&self, out: &mut Vec<(&'static str, f64)>) {
        out.clear();
        out.push(("syrinx", 0.5 * (self.lab_l.opening + self.lab_r.opening) * self.voice));
    }
}

// --- calls --------------------------------------------------------------------------------------

/// One "buk": a short, closed-beak cluck at `f` starting at `t`, `len` long.
fn cluck(keys: &mut Vec<CallKey>, t: f64, f: f64, len: f64, gape: f64) {
    keys.push(CallKey::new(t, &[("gate", 0.0), ("pitch", f * 1.08), ("gape", 0.05), ("larynx", 0.0)]).curve(Curve::Step));
    keys.push(CallKey::new(t + 0.008, &[("gate", 1.0), ("larynx", 0.9)]));
    keys.push(CallKey::new(t + 0.02, &[("gape", gape)]));
    keys.push(CallKey::new(t + len * 0.8, &[("gate", 1.0), ("pitch", f * 0.92), ("gape", gape)]).curve(Curve::Exp));
    keys.push(CallKey::new(t + len, &[("gate", 0.0), ("larynx", 0.0), ("gape", 0.05)]));
}

pub fn def() -> CreatureDef {
    let k = CallKey::new;

    // hen's contented clucking: a few soft, low "buk"s
    let mut cluck_keys = vec![k(0.0, &[("size", 0.6), ("pressure", 1.5), ("harsh", 0.45), ("rough", 0.35), ("voice", 1.0), ("throat", 2.0)])];
    for (i, f) in [330.0, 310.0, 340.0].iter().enumerate() {
        cluck(&mut cluck_keys, 0.08 + i as f64 * 0.32, *f, 0.11, 0.5);
    }

    // the laying cackle: "buk buk buk buk ... bu-KAAAK"
    let mut cackle_keys = vec![k(0.0, &[("size", 0.6), ("pressure", 1.6), ("harsh", 0.55), ("rough", 0.4), ("voice", 1.0), ("throat", 1.6)])];
    for i in 0..5 {
        cluck(&mut cackle_keys, 0.08 + i as f64 * 0.17, 420.0 + i as f64 * 15.0, 0.08, 0.55);
    }
    let t = 1.03;
    cackle_keys.extend([
        k(t, &[("gate", 0.0), ("pitch", 450.0), ("gape", 0.1), ("larynx", 0.0), ("pressure", 1.0)]).curve(Curve::Step),
        k(t + 0.01, &[("gate", 1.0), ("larynx", 0.9)]),
        k(t + 0.06, &[("gape", 0.25), ("pitch", 480.0)]),
        k(t + 0.12, &[("gate", 0.2), ("larynx", 0.1)]),
        k(t + 0.15, &[("gate", 1.0), ("larynx", 1.0), ("pitch", 900.0), ("pressure", 2.2), ("gape", 1.0), ("throat", 0.9)]).curve(Curve::Exp),
        k(t + 0.45, &[("pitch", 1000.0)]).curve(Curve::Exp),
        k(t + 0.62, &[("gate", 0.8), ("pitch", 760.0), ("pressure", 1.6)]).curve(Curve::Exp),
        k(t + 0.68, &[("gate", 0.0), ("gape", 0.3)]),
    ]);

    // the rooster's crow, five syllables: cock - a - doo - dle - doooo
    let crow = |t: f64, len: f64, f: f64, f_end: f64, gape: f64| -> Vec<CallKey> {
        vec![
            k(t, &[("larynx", 0.15), ("gape", 0.2)]),
            k(t + 0.02, &[("gate", 1.0), ("larynx", 1.0), ("gape", gape), ("pitch", f)]).curve(Curve::Exp),
            k(t + len - 0.03, &[("gate", 1.0), ("pitch", f_end), ("gape", gape)]).curve(Curve::Exp),
            k(t + len, &[("gate", 0.35), ("larynx", 0.15), ("gape", 0.25)]),
        ]
    };
    let mut crow_keys = vec![k(0.0, &[("gate", 0.0), ("size", 1.0), ("pressure", 2.6), ("harsh", 0.75), ("rough", 0.55), ("voice", 1.0), ("throat", 1.2), ("tension", 0.55), ("pitch", 480.0)])];
    crow_keys.extend(crow(0.12 + 0.0, 0.16, 560.0, 620.0, 0.6));
    crow_keys.extend(crow(0.12 + 0.2, 0.12, 600.0, 640.0, 0.9));
    crow_keys.extend(crow(0.12 + 0.36, 0.22, 700.0, 740.0, 0.5));
    crow_keys.extend(crow(0.12 + 0.62, 0.16, 680.0, 660.0, 0.7));
    crow_keys.extend(crow(0.12 + 0.82, 0.95, 720.0, 520.0, 0.85));
    crow_keys.push(k(1.94, &[("gate", 0.0)]));

    // chick distress peeps: pure, falling, ~3.5 kHz
    let mut peep_keys = vec![k(0.0, &[("size", 0.0), ("pressure", 0.8), ("harsh", 0.1), ("rough", 0.05), ("voice", 1.0), ("throat", 0.4), ("tension", 0.35)])];
    for i in 0..4 {
        let t = 0.12 + i as f64 * 0.28;
        peep_keys.extend([
            k(t, &[("gate", 0.0), ("pitch", 4200.0), ("gape", 0.7)]).curve(Curve::Step),
            k(t + 0.01, &[("gate", 1.0)]),
            k(t + 0.13, &[("gate", 1.0), ("pitch", 2900.0)]).curve(Curve::Exp),
            k(t + 0.15, &[("gate", 0.0)]),
        ]);
    }

    // aerial-predator alarm: a loud, harsh, noisy scream
    let alarm_keys = vec![
        k(0.0, &[("gate", 0.0), ("size", 0.65), ("pressure", 3.0), ("harsh", 1.0), ("rough", 1.0), ("voice", 1.0), ("breath", 0.4), ("throat", 0.7), ("gape", 1.0), ("pitch", 1100.0), ("larynx", 1.0)]),
        k(0.1, &[("gate", 0.0), ("pitch", 1100.0)]),
        k(0.12, &[("gate", 1.0), ("pitch", 1500.0)]).curve(Curve::Exp),
        k(0.45, &[("pitch", 1350.0)]).curve(Curve::Exp),
        k(0.6, &[("gate", 0.0), ("pitch", 1000.0)]).curve(Curve::Exp),
    ];

    // food call: rapid "tuk-tuk-tuk"
    let mut food_keys = vec![k(0.0, &[("size", 0.85), ("pressure", 1.8), ("harsh", 0.5), ("rough", 0.3), ("voice", 1.0), ("throat", 1.4)])];
    for i in 0..8 {
        cluck(&mut food_keys, 0.08 + i as f64 * 0.105, 600.0, 0.05, 0.6);
    }

    CreatureDef {
        id: "chicken",
        name: "Chicken",
        latin: "Gallus gallus domesticus",
        group: CreatureGroup::Bird,
        blurb: "Air sacs push breath through a syrinx at the foot of a long ringed trachea; the larynx and beak cut the call into syllables.",
        role: "Clucky lead vocals, cackling hooks, a rooster's crow",
        // 96 kHz inside: a third of the 4x cost, and the same sound (calls and songs compared by
        // spectrogram; the noise gain is set for this rate)
        oversample: 2,
        gate_times: Some(GateTimes { attack: 0.006, release: 0.02 }),
        params: vec![
            ParamDef::new("pitch", "Syrinx pitch", 150.0, 5000.0, 380.0).unit("Hz").log().smooth(0.01).pitch(),
            ParamDef::new("pressure", "Air-sac pressure", 0.2, 4.0, 1.2).unit("kPa").smooth(0.015).digits(2),
            ParamDef::new("voice", "Syrinx voicing", 0.0, 1.0, 1.0).smooth(0.006),
            ParamDef::new("harsh", "Harshness", 0.0, 1.0, 0.5).smooth(0.02),
            ParamDef::new("tension", "Membrane tension", 0.0, 1.0, 0.45).smooth(0.02),
            ParamDef::new("rough", "Roughness (jitter, detune)", 0.0, 1.0, 0.35).smooth(0.03),
            ParamDef::new("breath", "Breath noise", 0.0, 1.0, 0.0).smooth(0.008),
            ParamDef::new("size", "Chick ↔ hen ↔ rooster", 0.0, 1.0, 0.6).smooth(0.05),
            ParamDef::new("larynx", "Larynx opening", 0.0, 1.0, 1.0).smooth(0.006),
            ParamDef::new("throat", "Throat cavity size", 0.3, 4.0, 1.5).unit("cm³").log().smooth(0.02).digits(2),
            ParamDef::new("gape", "Beak gape", 0.0, 1.0, 0.45).smooth(0.008),
        ],
        calls: vec![
            CallDef::new("cluck", "Clucks", "A hen's contented, low \"buk… buk… buk\"", Some(330.0), cluck_keys),
            CallDef::new("cackle", "Laying cackle", "\"Buk buk buk buk… bu-KAAAK!\"", Some(450.0), cackle_keys),
            CallDef::new("crow", "Rooster crow", "Cock-a-doodle-doo, five syllables, the last held and falling", Some(600.0), crow_keys),
            CallDef::new("peep", "Chick peeps", "Distress peeps: pure falling whistles, ~3.5 kHz", Some(3500.0), peep_keys),
            CallDef::new("alarm", "Alarm scream", "Hawk overhead! A harsh, noisy scream", Some(1300.0), alarm_keys),
            CallDef::new("food", "Food call", "Rapid \"tuk-tuk-tuk\" calling chicks to food", Some(600.0), food_keys),
        ],
        events: vec![],
        create,
    }
    .resolved()
}
