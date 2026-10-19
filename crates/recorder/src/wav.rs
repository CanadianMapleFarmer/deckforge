use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

pub const RF64_THRESHOLD: u64 = u32::MAX as u64 - (1 << 20);
const HEADER_BYTES: u64 = 80;
const JUNK_OFFSET: u64 = 12;

pub struct WavWriter {
    out: BufWriter<File>,
    bits: u16,
    data_bytes: u64,
    threshold: u64,
    rf64: bool,
    scratch: Vec<u8>,
}

impl WavWriter {
    pub fn create(path: &Path, sample_rate: u32, bits: u16) -> std::io::Result<Self> {
        Self::with_threshold(path, sample_rate, bits, RF64_THRESHOLD)
    }

    pub fn with_threshold(path: &Path, sample_rate: u32, bits: u16, threshold: u64) -> std::io::Result<Self> {
        let mut out = BufWriter::with_capacity(1 << 20, File::create_new(path)?);
        let block_align = 2 * (bits / 8);
        let mut h = Vec::with_capacity(HEADER_BYTES as usize);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&(HEADER_BYTES as u32 - 8).to_le_bytes());
        h.extend_from_slice(b"WAVE");
        h.extend_from_slice(b"JUNK");
        h.extend_from_slice(&28u32.to_le_bytes());
        h.extend_from_slice(&[0; 28]);
        h.extend_from_slice(b"fmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&2u16.to_le_bytes());
        h.extend_from_slice(&sample_rate.to_le_bytes());
        h.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
        h.extend_from_slice(&block_align.to_le_bytes());
        h.extend_from_slice(&bits.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&0u32.to_le_bytes());
        out.write_all(&h)?;
        Ok(Self { out, bits, data_bytes: 0, threshold, rf64: false, scratch: Vec::new() })
    }

    pub fn write(&mut self, frames: &[[f32; 2]]) -> std::io::Result<()> {
        self.scratch.clear();
        for f in frames {
            for &s in f {
                let s = s.clamp(-1.0, 1.0);
                if self.bits == 16 {
                    self.scratch.extend_from_slice(&((s * 32_767.0).round() as i16).to_le_bytes());
                } else {
                    self.scratch.extend_from_slice(&((s * 8_388_607.0).round() as i32).to_le_bytes()[..3]);
                }
            }
        }
        self.out.write_all(&self.scratch)?;
        self.data_bytes += self.scratch.len() as u64;
        Ok(())
    }

    pub fn bytes(&self) -> u64 {
        HEADER_BYTES + self.data_bytes
    }

    /// Flushes and rewrites the sizes so the file is playable as it stands.
    pub fn checkpoint(&mut self) -> std::io::Result<()> {
        self.out.flush()?;
        if self.data_bytes > self.threshold {
            self.rf64 = true;
        }
        let file = self.out.get_mut();
        let riff = HEADER_BYTES - 8 + self.data_bytes;
        if self.rf64 {
            file.seek(SeekFrom::Start(0))?;
            file.write_all(b"RF64")?;
            file.write_all(&u32::MAX.to_le_bytes())?;
            file.seek(SeekFrom::Start(JUNK_OFFSET))?;
            let mut ds64 = Vec::with_capacity(36);
            ds64.extend_from_slice(b"ds64");
            ds64.extend_from_slice(&28u32.to_le_bytes());
            ds64.extend_from_slice(&riff.to_le_bytes());
            ds64.extend_from_slice(&self.data_bytes.to_le_bytes());
            ds64.extend_from_slice(&(self.data_bytes / (2 * (self.bits as u64 / 8))).to_le_bytes());
            ds64.extend_from_slice(&0u32.to_le_bytes());
            file.write_all(&ds64)?;
            file.seek(SeekFrom::Start(HEADER_BYTES - 4))?;
            file.write_all(&u32::MAX.to_le_bytes())?;
        } else {
            file.seek(SeekFrom::Start(4))?;
            file.write_all(&(riff as u32).to_le_bytes())?;
            file.seek(SeekFrom::Start(HEADER_BYTES - 4))?;
            file.write_all(&(self.data_bytes as u32).to_le_bytes())?;
        }
        file.seek(SeekFrom::End(0))?;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        self.checkpoint()?;
        self.out.get_mut().sync_all()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn tone(n: usize, bits: u16) -> Vec<[f32; 2]> {
        let q = if bits == 16 { 32_767.0 } else { 8_388_607.0 };
        (0..n)
            .map(|i| {
                let x = (i as f32 * 0.03).sin() * 0.7;
                let l = (x * q).round() / q;
                [l, -l]
            })
            .collect()
    }

    fn read_all(path: &Path) -> (hound::WavSpec, Vec<i32>) {
        let mut r = hound::WavReader::open(path).unwrap();
        let spec = r.spec();
        (spec, r.samples::<i32>().map(Result::unwrap).collect())
    }

    #[test]
    fn wav_round_trips_24_and_16_bit() {
        let dir = tempdir();
        for bits in [16u16, 24] {
            let path = dir.join(format!("t{bits}.wav"));
            let frames = tone(10_000, bits);
            let mut w = WavWriter::create(&path, 48_000, bits).unwrap();
            w.write(&frames).unwrap();
            w.finish().unwrap();
            let (spec, samples) = read_all(&path);
            assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (2, 48_000, bits));
            let q = if bits == 16 { 32_767.0 } else { 8_388_607.0 };
            let expect: Vec<i32> = frames.iter().flat_map(|f| f.map(|s| (s * q).round() as i32)).collect();
            assert_eq!(samples, expect);
        }
    }

    #[test]
    fn the_header_is_valid_while_recording() {
        let dir = tempdir();
        let path = dir.join("live.wav");
        let mut w = WavWriter::create(&path, 48_000, 24).unwrap();
        w.write(&tone(4_800, 24)).unwrap();
        w.checkpoint().unwrap();
        w.write(&tone(4_800, 24)).unwrap();
        let (_, samples) = read_all(&path);
        assert!(samples.len() >= 4_800 * 2, "{}", samples.len());
        w.finish().unwrap();
        assert_eq!(read_all(&path).1.len(), 9_600 * 2);
    }

    #[test]
    fn rf64_promotion_past_the_limit() {
        let dir = tempdir();
        let path = dir.join("big.wav");
        let frames = tone(20_000, 24);
        let mut w = WavWriter::with_threshold(&path, 48_000, 24, 64 * 1024).unwrap();
        w.write(&frames).unwrap();
        w.finish().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RF64");
        assert_eq!(&bytes[12..16], b"ds64");
        let le64 = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        assert_eq!(le64(20), bytes.len() as u64 - 8);
        assert_eq!(le64(28), 20_000 * 6);
        assert_eq!(le64(36), 20_000);
        assert_eq!(&bytes[72..76], b"data");
        let data: Vec<i32> = bytes[80..].as_chunks::<3>().0.iter().map(|b| i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8).collect();
        let expect: Vec<i32> = frames.iter().flat_map(|f| f.map(|s| (s * 8_388_607.0).round() as i32)).collect();
        assert_eq!(data, expect);
    }

    pub fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deckforge-recorder-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn rand_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed) * 1_000_003 + std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() as u64
    }
}
