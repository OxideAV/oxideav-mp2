//! Developer probe: decode an ISO/IEC 13818-3 Layer II multichannel
//! stream and compare every reconstructed channel against the
//! per-channel reference PCM files sitting next to it.
//!
//! Usage: `mc_probe <base.mpg> [base.ext]`
//!
//! Reference PCM convention (see
//! `docs/audio/mp3/iso-13818-4-audio-conformance.md`): 16-bit signed
//! **big-endian**, headerless, one file per channel named
//! `<stem>_<suffix>.pcm` with suffixes `l r c ls rs s l2 r2 lfe
//! m1..m7`.

use oxideav_mp2::mc::{decode_mc_stream, McChannel};
use std::path::Path;

fn suffix_of(ch: McChannel) -> &'static str {
    match ch {
        McChannel::Left => "l",
        McChannel::Right => "r",
        McChannel::Centre => "c",
        McChannel::LeftSurround => "ls",
        McChannel::RightSurround => "rs",
        McChannel::MonoSurround => "s",
        McChannel::SecondLeft => "l2",
        McChannel::SecondRight => "r2",
    }
}

fn read_ref_be(path: &Path) -> Option<Vec<i16>> {
    let bytes = std::fs::read(path).ok()?;
    Some(
        bytes
            .chunks_exact(2)
            .map(|c| i16::from_be_bytes([c[0], c[1]]))
            .collect(),
    )
}

fn compare(name: &str, ours: &[f64], reference: &[i16]) {
    let n = ours.len().min(reference.len());
    let mut max_diff = 0i64;
    let mut sum_sq = 0f64;
    let mut exact = 0usize;
    for i in 0..n {
        let s = (ours[i] * 32768.0).round().clamp(-32768.0, 32767.0) as i64;
        let d = (s - reference[i] as i64).abs();
        max_diff = max_diff.max(d);
        sum_sq += (d * d) as f64;
        if d == 0 {
            exact += 1;
        }
    }
    let rms = (sum_sq / n as f64).sqrt();
    println!(
        "  {name:>4}: len ours {} vs ref {} | max {} LSB rms {:.3} bitexact {:.2}%",
        ours.len(),
        reference.len(),
        max_diff,
        rms,
        100.0 * exact as f64 / n as f64
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: mc_probe <base.mpg> [base.ext]");
        std::process::exit(2);
    }
    let base_path = Path::new(&args[1]);
    let base = std::fs::read(base_path).expect("read base stream");
    let ext = args
        .get(2)
        .map(|p| std::fs::read(p).expect("read ext stream"));

    let decoded = match decode_mc_stream(&base, ext.as_deref()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("DECODE FAILED: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "frames {} | header {:?} | config {:?} | layout {:?}",
        decoded.frames, decoded.mc_header, decoded.config, decoded.layout
    );

    let stem = base_path.with_extension("");
    let stem = stem.to_string_lossy();
    for (label, pcm) in decoded.layout.iter().zip(&decoded.channels) {
        let suffix = suffix_of(*label);
        let ref_path = format!("{stem}_{suffix}.pcm");
        match read_ref_be(Path::new(&ref_path)) {
            Some(r) => compare(suffix, pcm, &r),
            None => println!("  {suffix:>4}: (no reference file)"),
        }
    }
    if let Some(lfe) = &decoded.lfe {
        let ref_path = format!("{stem}_lfe.pcm");
        match read_ref_be(Path::new(&ref_path)) {
            Some(r) => compare("lfe", lfe, &r),
            None => println!("   lfe: (no reference file)"),
        }
    }
    for (i, ml) in decoded.multilingual.iter().enumerate() {
        let ref_path = format!("{stem}_m{}.pcm", i + 1);
        match read_ref_be(Path::new(&ref_path)) {
            Some(r) => compare(&format!("m{}", i + 1), ml, &r),
            None => println!("    m{}: (no reference file)", i + 1),
        }
    }
}
