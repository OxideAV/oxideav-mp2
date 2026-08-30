//! Optional §2.5.3.2.1.1 **post-dematrix surround processing** for
//! dematrixing procedure `'10'` (phase-mixed surround).
//!
//! After dematrixing a `'10'` stream, ISO/IEC 13818-3 lists two
//! operations that *"may be done on the signals jLSw and jRSw in the
//! 3/2 configuration or jSw in the 3/1 configuration before output
//! (these operations may not be done before dematrixing)"*:
//!
//! * **3a — −90 degrees phase shift**, undoing the encoder side's
//!   +90° shift (§C.2.1.5: the T3/T4 signals "optionally may be
//!   processed by dynamic range compression, and 90 degrees phase
//!   shifting");
//! * **3b — Dynamic expansion** — the spec names the operation but
//!   defines no expander law, ratio or time constants anywhere in
//!   ISO/IEC 13818-3 (it is the notional inverse of the encoder's
//!   equally-unparameterised compression), so this module implements
//!   only the mathematically-defined phase shift. The suite's
//!   references are decoded to ≤ 1 LSB *without* either stage, which
//!   is why both are strictly opt-in.
//!
//! The −90° shift is a linear-phase FIR Hilbert transformer
//! (`X̂(ω) = −j·sgn(ω)·X(ω)`, i.e. every positive-frequency component
//! delayed a quarter cycle): ideal impulse response `h[n] = 2/(πn)`
//! for odd `n`, `0` otherwise, Hann-windowed over `2M + 1` taps with
//! `M =` [`PHASE_SHIFT_DELAY`]. Because a causal FIR realises the
//! shift only `M` samples late, every *other* output — front
//! channels, LFE, multilingual — is delayed to match, so the
//! presentation stays sample-aligned: full-rate channels by `M`, the
//! `Fs/96` LFE by `M/96`, half-rate multilingual channels by `M/2`
//! (`M` is chosen a multiple of 96 exactly so these are integers).
//!
//! Clean-room: ISO/IEC 13818-3 (1997) §2.5.3.2.1.1 / §C.2.1.5 only.

use crate::mc::{McChannel, McDecodedFrame, McDecodedStream};

/// Group delay `M` of the phase-shift FIR (samples at the full
/// sampling frequency). A multiple of 96 so the matching LFE
/// (`Fs/96`) and half-rate multilingual (`Fs/2`) delays are whole
/// samples.
pub const PHASE_SHIFT_DELAY: usize = 96;

/// FIR length (`2M + 1` taps).
const TAPS: usize = 2 * PHASE_SHIFT_DELAY + 1;

/// Hann-windowed ideal Hilbert-transformer taps (`h[M ± n]`,
/// antisymmetric).
fn hilbert_taps() -> Vec<f64> {
    let m = PHASE_SHIFT_DELAY as isize;
    (0..TAPS as isize)
        .map(|i| {
            let n = i - m;
            if n % 2 == 0 {
                0.0
            } else {
                let ideal = 2.0 / (std::f64::consts::PI * n as f64);
                // Hann window over the full length.
                let w = 0.5 * (1.0 + (std::f64::consts::PI * n as f64 / (m as f64 + 1.0)).cos());
                ideal * w
            }
        })
        .collect()
}

/// Streaming −90° phase shifter for one channel: a causal FIR whose
/// output is the Hilbert transform of the input delayed by
/// [`PHASE_SHIFT_DELAY`] samples.
#[derive(Debug, Clone)]
pub struct PhaseShift90 {
    taps: Vec<f64>,
    /// Last `TAPS − 1` input samples (most recent last).
    hist: Vec<f64>,
}

impl PhaseShift90 {
    /// Fresh shifter with zeroed history.
    pub fn new() -> Self {
        PhaseShift90 {
            taps: hilbert_taps(),
            hist: vec![0.0; TAPS - 1],
        }
    }

    /// Shift one buffer in place (streaming: history carries across
    /// calls, so consecutive buffers form one continuous signal).
    pub fn process(&mut self, buf: &mut [f64]) {
        let mut ext = Vec::with_capacity(self.hist.len() + buf.len());
        ext.extend_from_slice(&self.hist);
        ext.extend_from_slice(buf);
        for (i, out) in buf.iter_mut().enumerate() {
            // y[i] = Σ_k taps[k] · x[i + (TAPS−1) − k]
            let base = i; // ext index of the oldest tap input
            let mut acc = 0.0f64;
            for (k, &t) in self.taps.iter().enumerate() {
                if t != 0.0 {
                    acc += t * ext[base + TAPS - 1 - k];
                }
            }
            *out = acc;
        }
        let keep = self.hist.len();
        self.hist.copy_from_slice(&ext[ext.len() - keep..]);
    }
}

impl Default for PhaseShift90 {
    fn default() -> Self {
        Self::new()
    }
}

/// Streaming integer delay line for one channel.
#[derive(Debug, Clone)]
struct DelayLine {
    buf: Vec<f64>,
}

impl DelayLine {
    fn new(delay: usize) -> Self {
        DelayLine {
            buf: vec![0.0; delay],
        }
    }

    fn process(&mut self, data: &mut [f64]) {
        if self.buf.is_empty() {
            return;
        }
        let mut ext = Vec::with_capacity(self.buf.len() + data.len());
        ext.extend_from_slice(&self.buf);
        ext.extend_from_slice(data);
        let keep = self.buf.len();
        data.copy_from_slice(&ext[..data.len()]);
        self.buf.copy_from_slice(&ext[ext.len() - keep..]);
    }
}

/// Streaming §2.5.3.2.1.1 surround processor: applies the −90° phase
/// shift to the surround presentation channels of a decoded `'10'`
/// stream and the matching delay to everything else.
///
/// Lazily sizes itself from the first frame; feed frames of one
/// stream in order (seek ⇒ [`SurroundProcessor::reset`] /
/// a fresh processor).
#[derive(Debug, Default)]
pub struct SurroundProcessor {
    shifters: Vec<Option<PhaseShift90>>,
    delays: Vec<DelayLine>,
    lfe_delay: Option<DelayLine>,
    ml_delays: Vec<DelayLine>,
}

impl SurroundProcessor {
    /// Fresh processor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-zero all histories (seek / discontinuity).
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Whether a frame is eligible: dematrixing procedure `'10'` with
    /// at least one surround presentation channel (the spec defines
    /// the processing exactly there).
    pub fn applies(frame: &McDecodedFrame) -> bool {
        frame.mc_header.dematrix_procedure == 2
            && frame.layout.iter().any(|c| {
                matches!(
                    c,
                    McChannel::LeftSurround | McChannel::RightSurround | McChannel::MonoSurround
                )
            })
    }

    /// Process one decoded frame in place. Returns `true` when the
    /// processing applies (procedure `'10'` with surround channels);
    /// a non-`'10'` frame is left untouched.
    pub fn process_frame(&mut self, frame: &mut McDecodedFrame) -> bool {
        if !Self::applies(frame) {
            return false;
        }
        if self.shifters.len() < frame.channels.len() {
            for ch in self.shifters.len()..frame.channels.len() {
                let surround = matches!(
                    frame.layout.get(ch),
                    Some(
                        McChannel::LeftSurround
                            | McChannel::RightSurround
                            | McChannel::MonoSurround
                    )
                );
                self.shifters.push(surround.then(PhaseShift90::new));
                self.delays.push(DelayLine::new(PHASE_SHIFT_DELAY));
            }
        }
        for (ch, data) in frame.channels.iter_mut().enumerate() {
            match &mut self.shifters[ch] {
                Some(shift) => shift.process(data),
                None => self.delays[ch].process(data),
            }
        }
        if let Some(lfe) = &mut frame.lfe {
            let d = self
                .lfe_delay
                .get_or_insert_with(|| DelayLine::new(PHASE_SHIFT_DELAY / 96));
            d.process(lfe);
        }
        if !frame.multilingual.is_empty() {
            let ml_delay = if frame.mc_header.multi_lingual_fs_half {
                PHASE_SHIFT_DELAY / 2
            } else {
                PHASE_SHIFT_DELAY
            };
            while self.ml_delays.len() < frame.multilingual.len() {
                self.ml_delays.push(DelayLine::new(ml_delay));
            }
            for (d, data) in self.ml_delays.iter_mut().zip(&mut frame.multilingual) {
                d.process(data);
            }
        }
        true
    }
}

/// Apply the §2.5.3.2.1.1 surround processing to a whole decoded
/// stream in place. Returns `true` when it applied (dematrixing
/// procedure `'10'` with surround channels — anything else is left
/// untouched, per the spec's "may be done" wording).
pub fn apply_surround_processing(stream: &mut McDecodedStream) -> bool {
    if stream.mc_header.dematrix_procedure != 2 {
        return false;
    }
    let surround_idx: Vec<usize> = stream
        .layout
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            matches!(
                c,
                McChannel::LeftSurround | McChannel::RightSurround | McChannel::MonoSurround
            )
            .then_some(i)
        })
        .collect();
    if surround_idx.is_empty() {
        return false;
    }
    for (ch, data) in stream.channels.iter_mut().enumerate() {
        if surround_idx.contains(&ch) {
            PhaseShift90::new().process(data);
        } else {
            DelayLine::new(PHASE_SHIFT_DELAY).process(data);
        }
    }
    if let Some(lfe) = &mut stream.lfe {
        DelayLine::new(PHASE_SHIFT_DELAY / 96).process(lfe);
    }
    let ml_delay = if stream.mc_header.multi_lingual_fs_half {
        PHASE_SHIFT_DELAY / 2
    } else {
        PHASE_SHIFT_DELAY
    };
    for data in &mut stream.multilingual {
        DelayLine::new(ml_delay).process(data);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_are_antisymmetric_with_zero_even_entries() {
        let taps = hilbert_taps();
        assert_eq!(taps.len(), TAPS);
        let m = PHASE_SHIFT_DELAY;
        assert_eq!(taps[m], 0.0);
        for n in 0..=m {
            assert!(
                (taps[m + n] + taps[m - n]).abs() < 1e-15,
                "antisymmetry at ±{n}"
            );
            if n % 2 == 0 {
                assert_eq!(taps[m + n], 0.0, "even tap {n}");
            }
        }
    }

    #[test]
    fn tone_is_shifted_minus_90_degrees_at_unit_gain() {
        // −90°: sin(ωt) ↦ sin(ωt − π/2) = −cos(ωt), delayed M samples.
        let fs = 48_000.0f64;
        for freq in [500.0f64, 1_000.0, 3_000.0, 8_000.0, 15_000.0] {
            let omega = 2.0 * std::f64::consts::PI * freq / fs;
            let n = 4096usize;
            let mut buf: Vec<f64> = (0..n).map(|i| (omega * i as f64).sin()).collect();
            let mut shift = PhaseShift90::new();
            shift.process(&mut buf);
            // Compare the steady middle against the expected output.
            let (mut err, mut sig) = (0.0f64, 0.0f64);
            for (off, &y) in buf[TAPS..n - TAPS].iter().enumerate() {
                let t = (TAPS + off - PHASE_SHIFT_DELAY) as f64;
                let want = -(omega * t).cos();
                let e = y - want;
                err += e * e;
                sig += want * want;
            }
            // The Hann-windowed 193-tap realisation leaves ~1 %
            // amplitude ripple at the band edges (−35 dB error) —
            // ample for an optional psychoacoustic output stage.
            assert!(
                err < 2e-3 * sig,
                "{freq} Hz: phase-shift error {err:.3e} vs {sig:.3e}"
            );
        }
    }

    #[test]
    fn streaming_chunks_equal_one_shot_processing() {
        let mut seed = 0x0135_79bd_f246_8ace_u64;
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let signal: Vec<f64> = (0..3 * 1152).map(|_| rand()).collect();
        let mut whole = signal.clone();
        PhaseShift90::new().process(&mut whole);
        let mut chunked = signal.clone();
        let mut shift = PhaseShift90::new();
        for chunk in chunked.chunks_mut(1152) {
            shift.process(chunk);
        }
        for (a, b) in whole.iter().zip(&chunked) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn delay_line_delays_exactly() {
        let mut d = DelayLine::new(3);
        let mut a = vec![1.0, 2.0, 3.0, 4.0];
        d.process(&mut a);
        assert_eq!(a, vec![0.0, 0.0, 0.0, 1.0]);
        let mut b = vec![5.0, 6.0];
        d.process(&mut b);
        assert_eq!(b, vec![2.0, 3.0]);
        // Zero delay is the identity.
        let mut z = DelayLine::new(0);
        let mut c = vec![7.0, 8.0];
        z.process(&mut c);
        assert_eq!(c, vec![7.0, 8.0]);
    }

    #[test]
    fn delay_constant_admits_integer_lfe_and_half_rate_delays() {
        assert_eq!(PHASE_SHIFT_DELAY % 96, 0);
        assert_eq!(PHASE_SHIFT_DELAY % 2, 0);
    }
}
