//! Мини-синтезатор: все звуки игры генерируются на лету из простых волн.

use std::sync::{Arc, Mutex};
use tinyaudio::prelude::*;

const RATE: f32 = 44100.0;

#[derive(Clone, Copy)]
pub enum Wave {
    Sine,
    Tri,
    Square,
}

struct Voice {
    freq: f32,
    phase: f32,
    t: f32,
    dur: f32,
    vol: f32,
    wave: Wave,
}

pub struct Audio {
    voices: Arc<Mutex<Vec<Voice>>>,
    _dev: Option<OutputDevice>,
    pub muted: bool,
}

impl Audio {
    pub fn new(enabled: bool) -> Self {
        let voices = Arc::new(Mutex::new(Vec::<Voice>::new()));
        let dev = if enabled {
            let shared = voices.clone();
            let params = OutputDeviceParameters {
                channels_count: 2,
                sample_rate: RATE as usize,
                // ~100 мс: с меньшим буфером DirectSound не успевает и звук заикается.
                channel_sample_count: 4410,
            };
            run_output_device(params, move |data| {
                let mut vs = match shared.lock() {
                    Ok(v) => v,
                    Err(_) => return,
                };
                for frame in data.chunks_mut(2) {
                    let mut s = 0.0f32;
                    for v in vs.iter_mut() {
                        v.t += 1.0 / RATE;
                        if v.t < 0.0 {
                            continue;
                        }
                        v.phase = (v.phase + v.freq / RATE).fract();
                        let w = match v.wave {
                            Wave::Sine => (v.phase * std::f32::consts::TAU).sin(),
                            Wave::Tri => 1.0 - 4.0 * (v.phase - 0.5).abs(),
                            Wave::Square => {
                                if v.phase < 0.5 {
                                    0.6
                                } else {
                                    -0.6
                                }
                            }
                        };
                        let k = (1.0 - v.t / v.dur).max(0.0);
                        let env = (v.t / 0.006).min(1.0) * k * k;
                        s += w * env * v.vol;
                    }
                    let s = s.clamp(-1.0, 1.0);
                    for o in frame {
                        *o = s;
                    }
                }
                vs.retain(|v| v.t < v.dur);
            })
            .ok()
        } else {
            None
        };
        Audio { voices, _dev: dev, muted: false }
    }

    /// Сыграть ноту: частота (Гц), длительность (с), громкость, форма волны, задержка (с).
    pub fn play(&self, freq: f32, dur: f32, vol: f32, wave: Wave, delay: f32) {
        if self.muted || self._dev.is_none() {
            return;
        }
        if let Ok(mut vs) = self.voices.lock() {
            if vs.len() < 32 {
                vs.push(Voice { freq, phase: 0.0, t: -delay, dur, vol, wave });
            }
        }
    }
}
