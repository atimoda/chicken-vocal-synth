/**
 * The chicken sings (Bestiary, chicken only): its own words, fitted to one of Sing's tunes the way
 * Sing fits them (src/song/fit.ts), then sent whole to the engine, which turns each syllable into
 * what a chicken can do with its beak, larynx and throat (crates/chimera/src/creatures/chicken_sing.rs).
 */
import { create } from "zustand";
import { engine } from "../audio/AudioEngine.ts";
import { lyricsToSyllables } from "../wasm/engine.ts";
import { TUNES } from "../song/tunes.ts";
import { fitSong, la } from "../song/fit.ts";
import { g2p } from "./speech.ts";
import { useStore } from "./store.ts";
import { useSing } from "./singing.ts";

export const CHICKEN_LYRICS = "Buk buk buk, bah gawk!\nI laid an egg for you.";

interface ChickenSingState {
  lyrics: string;
  /** a TUNES id */
  tune: string;
  setLyrics: (text: string) => void;
  setTune: (id: string) => void;
}

export const useChickenSing = create<ChickenSingState>((set) => ({
  lyrics: CHICKEN_LYRICS,
  tune: TUNES[0].id,
  setLyrics: (lyrics) => set({ lyrics }),
  setTune: (tune) => set({ tune }),
}));

/**
 * Semitones from the tune's (female) key for a bird of this size: a chick peeps about an octave
 * and a half up, a rooster sits a little below a hen.
 */
export function sizeShift(size: number): number {
  return size >= 0.6 ? -Math.round((size - 0.6) * 15) : Math.round((0.6 - size) * 35);
}

/** Sing the words to the tune now, starting the audio if it isn't running. */
export async function singChicken(): Promise<void> {
  const store = useStore.getState();
  await store.start();
  if (!useStore.getState().started) return;
  const lang = useSing.getState().lang;
  // Turkish reads by rule; English waits for the pronouncing dictionary (letter-to-sound if it fails)
  if (lang === "en") await g2p.load().catch(() => {});
  const { lyrics, tune: id } = useChickenSing.getState();
  const tune = TUNES.find((t) => t.id === id) ?? TUNES[0];
  const own = lyricsToSyllables(lyrics, lang);
  const words = own.length ? own : lyricsToSyllables(la(tune), lang);
  const cp = useStore.getState().cparams.chicken ?? {};
  const size = cp.size ?? 0.6;
  const { notes } = fitSong(tune, words, tune.octave.female * 12 + sizeShift(size));
  if (!notes.length) return;
  engine.creatureSing(notes, { bpm: tune.bpm, size, harsh: cp.harsh, rough: cp.rough, t0: 0.3 });
}
