use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use flacenc::bitsink::ByteSink;
use flacenc::component::{BitRepr, Stream, StreamInfo};
use flacenc::config::Encoder;
use flacenc::error::{Verified, Verify};
use flacenc::source::{Context, Fill, FrameBuf};

const BLOCK: usize = 4096;

pub struct FlacWriter {
    out: BufWriter<File>,
    config: Verified<Encoder>,
    info: StreamInfo,
    frame: FrameBuf,
    context: Context,
    bits: u16,
    pending: Vec<i32>,
    bytes: u64,
    header_bytes: u64,
}

fn io_err(e: impl std::fmt::Debug) -> std::io::Error {
    std::io::Error::other(format!("{e:?}"))
}

impl FlacWriter {
    pub fn create(path: &Path, sample_rate: u32, bits: u16) -> std::io::Result<Self> {
        let config = Encoder::default().into_verified().map_err(io_err)?;
        let info = StreamInfo::new(sample_rate as usize, 2, bits as usize).map_err(io_err)?;
        let mut out = BufWriter::with_capacity(1 << 20, File::create_new(path)?);
        let header = header_bytes(&info)?;
        out.write_all(&header)?;
        Ok(Self {
            out,
            config,
            info,
            frame: FrameBuf::with_size(2, BLOCK).map_err(io_err)?,
            context: Context::new(bits as usize, 2),
            bits,
            pending: Vec::with_capacity(BLOCK * 2),
            bytes: header.len() as u64,
            header_bytes: header.len() as u64,
        })
    }

    pub fn write(&mut self, frames: &[[f32; 2]]) -> std::io::Result<()> {
        let q = if self.bits == 16 { 32_767.0 } else { 8_388_607.0 };
        for f in frames {
            for &s in f {
                self.pending.push((s.clamp(-1.0, 1.0) * q).round() as i32);
            }
            if self.pending.len() == BLOCK * 2 {
                self.encode_pending()?;
            }
        }
        Ok(())
    }

    fn encode_pending(&mut self) -> std::io::Result<()> {
        let frames = self.pending.len() / 2;
        if frames == 0 {
            return Ok(());
        }
        if frames != BLOCK {
            self.frame.resize(frames);
        }
        (&mut self.frame, &mut self.context).fill_interleaved(&self.pending).map_err(io_err)?;
        let number = self.context.current_frame_number().unwrap_or(0);
        let frame = flacenc::encode_fixed_size_frame(&self.config, &self.frame, number, &self.info).map_err(io_err)?;
        self.info.update_frame_info(&frame);
        let mut sink = ByteSink::new();
        frame.write(&mut sink).map_err(io_err)?;
        self.out.write_all(sink.as_slice())?;
        self.bytes += sink.as_slice().len() as u64;
        self.pending.clear();
        Ok(())
    }

    pub fn bytes(&self) -> u64 {
        self.bytes + self.pending.len() as u64 * self.bits as u64 / 8
    }

    /// Flushes and rewrites STREAMINFO so a file cut off here still decodes.
    pub fn checkpoint(&mut self) -> std::io::Result<()> {
        self.out.flush()?;
        if self.context.total_samples() == 0 {
            return Ok(());
        }
        let mut info = self.info.clone();
        info.set_block_sizes(BLOCK, BLOCK).map_err(io_err)?;
        info.set_total_samples(self.context.total_samples());
        let header = header_bytes(&info)?;
        let file = self.out.get_mut();
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header)?;
        file.seek(SeekFrom::End(0))?;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        self.encode_pending()?;
        self.info.set_block_sizes(BLOCK, BLOCK).map_err(io_err)?;
        self.info.set_md5_digest(&self.context.md5_digest());
        self.info.set_total_samples(self.context.total_samples());
        self.out.flush()?;
        let header = header_bytes(&self.info)?;
        debug_assert_eq!(header.len() as u64, self.header_bytes);
        let file = self.out.get_mut();
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header)?;
        file.sync_all()
    }
}

fn header_bytes(info: &StreamInfo) -> std::io::Result<Vec<u8>> {
    let mut stream = Stream::new(info.sample_rate(), info.channels(), info.bits_per_sample()).map_err(io_err)?;
    *stream.stream_info_mut() = info.clone();
    let mut sink = ByteSink::new();
    stream.write(&mut sink).map_err(io_err)?;
    Ok(sink.as_slice().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::tests::{tempdir, tone};

    #[test]
    fn flac_round_trips_bit_exact() {
        let dir = tempdir();
        for bits in [16u16, 24] {
            let path = dir.join(format!("t{bits}.flac"));
            let frames = tone(10_000, bits);
            let mut w = FlacWriter::create(&path, 48_000, bits).unwrap();
            w.write(&frames[..3_000]).unwrap();
            w.write(&frames[3_000..]).unwrap();
            w.finish().unwrap();
            let q = if bits == 16 { 32_767.0 } else { 8_388_607.0 };
            let expect: Vec<i32> = frames.iter().flat_map(|f| f.map(|s| (s * q).round() as i32)).collect();
            assert_eq!(crate::writer::tests::decode_i32(&path), expect);
        }
    }

    #[test]
    fn a_flac_cut_off_mid_recording_still_decodes() {
        let dir = tempdir();
        let path = dir.join("crash.flac");
        let frames = tone(20_000, 24);
        let mut w = FlacWriter::create(&path, 48_000, 24).unwrap();
        w.write(&frames[..12_288]).unwrap();
        w.checkpoint().unwrap();
        w.write(&frames[12_288..]).unwrap();
        drop(w);
        let samples = crate::writer::tests::decode_i32(&path);
        assert!(samples.len() / 2 >= 12_288, "{}", samples.len() / 2);
        assert_eq!(crate::recorded_duration(&path).map(|d| (d * 48_000.0).round() as u64), Some(12_288));
    }
}
