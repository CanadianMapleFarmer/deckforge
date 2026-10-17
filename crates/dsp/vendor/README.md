# Vendored libraries

| Directory | Upstream | Licence |
|---|---|---|
| `signalsmith-stretch/` | https://github.com/Signalsmith-Audio/signalsmith-stretch (header as bundled with `signalsmith-stretch` crate 0.1.3) | MIT, see `LICENSE.txt` |
| `signalsmith-linear/` | https://github.com/Signalsmith-Audio/linear (`fft.h`, `stft.h` only) | MIT, see `LICENSE.txt` |

The headers are unmodified. `src/signalsmith.cpp` exposes the stretcher to Rust as a small C API.
