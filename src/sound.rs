const SAMPLE_RATE: u32 = 22_050;
const PEAK: f32 = 0.7;
const GAIN_DB: f32 = -25.0;

pub fn beep_wav() -> Vec<u8> {
    let mut samples = Vec::new();
    tone(&mut samples, 880.0, 0.17);
    silence(&mut samples, 0.03);
    tone(&mut samples, 1320.0, 0.20);
    wav(&samples)
}

fn tone(out: &mut Vec<i16>, freq: f32, secs: f32) {
    let n = (SAMPLE_RATE as f32 * secs) as usize;
    let fade = (SAMPLE_RATE as f32 * 0.006) as usize;
    let amp = PEAK * 10f32.powf(GAIN_DB / 20.0);
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let env = (i.min(n - 1 - i) as f32 / fade as f32).min(1.0);
        let v = (t * freq * std::f32::consts::TAU).sin() * env * amp;
        out.push((v * i16::MAX as f32) as i16);
    }
}

fn silence(out: &mut Vec<i16>, secs: f32) {
    out.extend(std::iter::repeat_n(0, (SAMPLE_RATE as f32 * secs) as usize));
}

fn wav(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    b.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_consistent() {
        let w = beep_wav();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        let riff_len = u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize;
        let data_len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
        assert_eq!(riff_len + 8, w.len());
        assert_eq!(data_len + 44, w.len());
        let secs = data_len as f32 / (SAMPLE_RATE as f32 * 2.0);
        assert!((0.35..0.45).contains(&secs), "{secs}");
    }

    #[test]
    fn tone_is_audible_and_does_not_clip() {
        let w = beep_wav();
        let peak = w[44..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs())
            .max()
            .unwrap();
        let db_vs_previous = 20.0 * (peak as f32 / (PEAK * i16::MAX as f32)).log10();
        assert!(
            (db_vs_previous - GAIN_DB).abs() < 0.1,
            "{db_vs_previous} dB"
        );
        assert!(peak > 500, "{peak}");
    }
}
