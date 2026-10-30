# Third-party Rust libraries

pzwm is compiled together with these libraries (its dependency tree in
[`Cargo.lock`](../Cargo.lock), for Linux). Cargo downloads their source when
you build; this repository does not include it. Each library keeps its own
license, and its full text is in that library's source.

Generated with `cargo tree -e normal`; regenerate it after changing
dependencies.

| Library | Version | License |
|---|---|---|
| [adler2](https://github.com/oyvindln/adler2) | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| [ahash](https://github.com/tkaitchuck/ahash) | 0.8.12 | MIT OR Apache-2.0 |
| [aliasable](https://github.com/avitex/rust-aliasable) | 0.1.3 | MIT |
| [anyhow](https://github.com/dtolnay/anyhow) | 1.0.104 | MIT OR Apache-2.0 |
| [appendlist](https://github.com/danieldulaney/appendlist) | 1.4.0 | MIT |
| [approx](https://github.com/brendanzab/approx) | 0.4.0 | Apache-2.0 |
| [as-raw-xcb-connection](https://github.com/psychon/as-raw-xcb-connection) | 1.0.1 | MIT OR Apache-2.0 |
| [atomic_float](https://github.com/thomcc/atomic_float) | 1.1.0 | Apache-2.0 OR MIT OR Unlicense |
| [bitflags](https://github.com/bitflags/bitflags) | 1.3.2 | MIT/Apache-2.0 |
| [bitflags](https://github.com/bitflags/bitflags) | 2.13.2 | MIT OR Apache-2.0 |
| [block-buffer](https://github.com/RustCrypto/utils) | 0.10.4 | MIT OR Apache-2.0 |
| [bytemuck](https://github.com/Lokathor/bytemuck) | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| [bytemuck_derive](https://github.com/Lokathor/bytemuck) | 1.12.1 | Zlib OR Apache-2.0 OR MIT |
| [calloop](https://github.com/Smithay/calloop) | 0.13.0 | MIT |
| [calloop](https://github.com/Smithay/calloop) | 0.14.5 | MIT |
| [calloop-wayland-source](https://github.com/smithay/calloop-wayland-source) | 0.3.0 | MIT |
| [cfg-if](https://github.com/rust-lang/cfg-if) | 1.0.5 | MIT OR Apache-2.0 |
| [cgmath](https://github.com/rustgd/cgmath) | 0.18.0 | Apache-2.0 |
| [cpufeatures](https://github.com/RustCrypto/utils) | 0.2.17 | MIT OR Apache-2.0 |
| [crc32fast](https://github.com/srijs/rust-crc32fast) | 1.5.3 | MIT OR Apache-2.0 |
| [crypto-common](https://github.com/RustCrypto/traits) | 0.1.7 | MIT OR Apache-2.0 |
| [cursor-icon](https://github.com/rust-windowing/cursor-icon) | 1.2.0 | MIT OR Apache-2.0 OR Zlib |
| [digest](https://github.com/RustCrypto/traits) | 0.10.7 | MIT OR Apache-2.0 |
| [dlib](https://github.com/elinorbgr/dlib) | 0.5.3 | MIT |
| [downcast-rs](https://github.com/marcianx/downcast-rs) | 1.2.1 | MIT/Apache-2.0 |
| [dpi](https://github.com/rust-windowing/winit) | 0.1.2 | Apache-2.0 AND MIT |
| [drm](https://github.com/Smithay/drm-rs) | 0.14.1 | MIT |
| [drm-ffi](https://github.com/Smithay/drm-rs) | 0.9.1 | MIT |
| [drm-fourcc](https://github.com/danielzfranklin/drm-fourcc-rs) | 2.2.0 | MIT |
| [drm-sys](https://github.com/Smithay/drm-rs) | 0.8.1 | MIT |
| [equivalent](https://github.com/indexmap-rs/equivalent) | 1.0.3 | Apache-2.0 OR MIT |
| [errno](https://github.com/lambda-fairy/rust-errno) | 0.3.14 | MIT OR Apache-2.0 |
| [fastrand](https://github.com/smol-rs/fastrand) | 2.5.0 | Apache-2.0 OR MIT |
| [fdeflate](https://github.com/image-rs/fdeflate) | 0.3.7 | MIT OR Apache-2.0 |
| [flate2](https://github.com/rust-lang/flate2-rs) | 1.1.10 | MIT OR Apache-2.0 |
| [gbm](https://github.com/Smithay/gbm.rs) | 0.18.0 | MIT |
| [gbm-sys](https://github.com/Drakulix/gbm.rs/tree/master/gbm-sys) | 0.4.0 | MIT |
| [generic-array](https://github.com/fizyk20/generic-array.git) | 0.14.7 | MIT |
| [gethostname](https://codeberg.org/swsnr/gethostname.rs.git) | 1.1.0 | Apache-2.0 |
| [getrandom](https://github.com/rust-random/getrandom) | 0.3.4 | MIT OR Apache-2.0 |
| [getrandom](https://github.com/rust-random/getrandom) | 0.4.3 | MIT OR Apache-2.0 |
| [hashbrown](https://github.com/rust-lang/hashbrown) | 0.17.1 | MIT OR Apache-2.0 |
| [indexmap](https://github.com/indexmap-rs/indexmap) | 2.14.2 | Apache-2.0 OR MIT |
| [input](https://github.com/Drakulix/input.rs) | 0.9.1 | MIT |
| [input-sys](https://github.com/Drakulix/input.rs/tree/master/input-sys) | 1.19.0 | MIT |
| [io-lifetimes](https://github.com/sunfishcode/io-lifetimes) | 1.0.11 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [itoa](https://github.com/dtolnay/itoa) | 1.0.18 | MIT OR Apache-2.0 |
| [lazy_static](https://github.com/rust-lang-nursery/lazy-static.rs) | 1.5.1 | MIT OR Apache-2.0 |
| [libc](https://github.com/rust-lang/libc) | 0.2.191 | MIT OR Apache-2.0 |
| [libloading](https://github.com/nagisa/rust_libloading/) | 0.8.9 | ISC |
| [libseat](https://github.com/PolyMeilex/libseat-rs) | 0.2.4 | MIT |
| [libseat-sys](https://github.com/PolyMeilex/libseat-rs) | 0.2.0 | MIT |
| [libudev-sys](https://github.com/dcuddeback/libudev-sys) | 0.1.4 | MIT |
| [linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) | 0.9.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [log](https://github.com/rust-lang/log) | 0.4.34 | MIT OR Apache-2.0 |
| [matchers](https://github.com/hawkw/matchers) | 0.2.0 | MIT |
| [memchr](https://github.com/BurntSushi/memchr) | 2.8.3 | Unlicense OR MIT |
| [memmap2](https://github.com/RazrFalcon/memmap2-rs) | 0.9.12 | MIT OR Apache-2.0 |
| [memoffset](https://github.com/Gilnaa/memoffset) | 0.9.1 | MIT |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| [nix](https://github.com/nix-rust/nix) | 0.31.3 | MIT |
| [nu-ansi-term](https://github.com/nushell/nu-ansi-term) | 0.50.3 | MIT |
| [num-traits](https://github.com/rust-num/num-traits) | 0.2.19 | MIT OR Apache-2.0 |
| [once_cell](https://github.com/matklad/once_cell) | 1.21.4 | MIT OR Apache-2.0 |
| [percent-encoding](https://github.com/servo/rust-url/) | 2.3.2 | MIT OR Apache-2.0 |
| [pin-project-lite](https://github.com/taiki-e/pin-project-lite) | 0.2.17 | Apache-2.0 OR MIT |
| [png](https://github.com/image-rs/image-png) | 0.17.16 | MIT OR Apache-2.0 |
| [polling](https://github.com/smol-rs/polling) | 3.11.0 | Apache-2.0 OR MIT |
| [ppv-lite86](https://github.com/cryptocorrosion/cryptocorrosion) | 0.2.21 | MIT OR Apache-2.0 |
| [proc-macro2](https://github.com/dtolnay/proc-macro2) | 1.0.107 | MIT OR Apache-2.0 |
| [profiling](https://github.com/aclysma/profiling) | 1.0.18 | MIT OR Apache-2.0 |
| [profiling-procmacros](https://github.com/aclysma/profiling) | 1.0.18 | MIT OR Apache-2.0 |
| [quick-xml](https://github.com/tafia/quick-xml) | 0.41.0 | MIT |
| [quote](https://github.com/dtolnay/quote) | 1.0.47 | MIT OR Apache-2.0 |
| [rand](https://github.com/rust-random/rand) | 0.9.5 | MIT OR Apache-2.0 |
| [rand_chacha](https://github.com/rust-random/rand) | 0.9.0 | MIT OR Apache-2.0 |
| [rand_core](https://github.com/rust-random/rand) | 0.9.5 | MIT OR Apache-2.0 |
| [raw-window-handle](https://github.com/rust-windowing/raw-window-handle) | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| [regex-automata](https://github.com/rust-lang/regex) | 0.4.18 | MIT OR Apache-2.0 |
| [regex-syntax](https://github.com/rust-lang/regex) | 0.8.11 | MIT OR Apache-2.0 |
| [rustix](https://github.com/bytecodealliance/rustix) | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [rustix](https://github.com/bytecodealliance/rustix) | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [scoped-tls](https://github.com/alexcrichton/scoped-tls) | 1.0.1 | MIT/Apache-2.0 |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_core](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | 1.0.151 | MIT OR Apache-2.0 |
| [serde_spanned](https://github.com/toml-rs/toml) | 0.6.9 | MIT OR Apache-2.0 |
| [sha2](https://github.com/RustCrypto/hashes) | 0.10.9 | MIT OR Apache-2.0 |
| [sharded-slab](https://github.com/hawkw/sharded-slab) | 0.1.7 | MIT |
| [simd-adler32](https://github.com/mcountryman/simd-adler32) | 0.3.10 | MIT |
| [slab](https://github.com/tokio-rs/slab) | 0.4.12 | MIT |
| [smallvec](https://github.com/servo/rust-smallvec) | 1.16.4 | MIT OR Apache-2.0 |
| [smithay](https://github.com/Smithay/smithay) | 0.7.0 | MIT |
| [smithay-client-toolkit](https://github.com/smithay/client-toolkit) | 0.19.2 | MIT |
| smithay-drm-extras | 0.1.0 | MIT |
| [smol_str](https://github.com/rust-analyzer/smol_str) | 0.2.2 | MIT OR Apache-2.0 |
| [syn](https://github.com/dtolnay/syn) | 2.0.119 | MIT OR Apache-2.0 |
| [syn](https://github.com/dtolnay/syn) | 3.0.9 | MIT OR Apache-2.0 |
| [tempfile](https://github.com/Stebalien/tempfile) | 3.27.0 | MIT OR Apache-2.0 |
| [thiserror](https://github.com/dtolnay/thiserror) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror](https://github.com/dtolnay/thiserror) | 2.0.21 | MIT OR Apache-2.0 |
| [thiserror-impl](https://github.com/dtolnay/thiserror) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror-impl](https://github.com/dtolnay/thiserror) | 2.0.21 | MIT OR Apache-2.0 |
| [thread_local](https://github.com/Amanieu/thread_local-rs) | 1.1.10 | MIT OR Apache-2.0 |
| [toml](https://github.com/toml-rs/toml) | 0.8.23 | MIT OR Apache-2.0 |
| [toml_datetime](https://github.com/toml-rs/toml) | 0.6.11 | MIT OR Apache-2.0 |
| [toml_edit](https://github.com/toml-rs/toml) | 0.22.27 | MIT OR Apache-2.0 |
| [toml_write](https://github.com/toml-rs/toml) | 0.1.2 | MIT OR Apache-2.0 |
| [tracing](https://github.com/tokio-rs/tracing) | 0.1.44 | MIT |
| [tracing-attributes](https://github.com/tokio-rs/tracing) | 0.1.31 | MIT |
| [tracing-core](https://github.com/tokio-rs/tracing) | 0.1.36 | MIT |
| [tracing-log](https://github.com/tokio-rs/tracing) | 0.2.0 | MIT |
| [tracing-subscriber](https://github.com/tokio-rs/tracing) | 0.3.23 | MIT |
| [typenum](https://github.com/paholg/typenum) | 1.20.1 | MIT OR Apache-2.0 |
| [udev](https://github.com/Smithay/udev-rs) | 0.9.3 | MIT |
| [unicode-ident](https://github.com/dtolnay/unicode-ident) | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [wayland-backend](https://github.com/smithay/wayland-rs) | 0.3.17 | MIT |
| [wayland-client](https://github.com/smithay/wayland-rs) | 0.31.15 | MIT |
| [wayland-csd-frame](https://github.com/rust-windowing/wayland-csd-frame) | 0.3.0 | MIT |
| [wayland-cursor](https://github.com/smithay/wayland-rs) | 0.31.14 | MIT |
| [wayland-egl](https://github.com/smithay/wayland-rs) | 0.32.11 | MIT |
| [wayland-protocols](https://github.com/smithay/wayland-rs) | 0.32.13 | MIT |
| [wayland-protocols-misc](https://github.com/smithay/wayland-rs) | 0.3.12 | MIT |
| [wayland-protocols-plasma](https://github.com/smithay/wayland-rs) | 0.3.12 | MIT |
| [wayland-protocols-wlr](https://github.com/smithay/wayland-rs) | 0.3.12 | MIT |
| [wayland-scanner](https://github.com/smithay/wayland-rs) | 0.31.11 | MIT |
| [wayland-server](https://github.com/smithay/wayland-rs) | 0.31.14 | MIT |
| [wayland-sys](https://github.com/smithay/wayland-rs) | 0.31.11 | MIT |
| [winit](https://github.com/rust-windowing/winit) | 0.30.13 | Apache-2.0 |
| [winnow](https://github.com/winnow-rs/winnow) | 0.7.15 | MIT |
| [x11-dl](https://github.com/AltF02/x11-rs.git) | 2.21.0 | MIT |
| [x11rb](https://github.com/psychon/x11rb) | 0.13.2 | MIT OR Apache-2.0 |
| [x11rb-protocol](https://github.com/psychon/x11rb) | 0.13.2 | MIT OR Apache-2.0 |
| [xcursor](https://github.com/esposm03/xcursor-rs) | 0.3.11 | MIT |
| [xkbcommon](https://github.com/rust-x-bindings/xkbcommon-rs) | 0.8.0 | MIT |
| [xkbcommon-dl](https://github.com/rust-windowing/xkbcommon-dl) | 0.4.2 | MIT |
| [xkeysym](https://github.com/notgull/xkeysym) | 0.2.1 | MIT OR Apache-2.0 OR Zlib |
| [zerocopy](https://github.com/google/zerocopy) | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| [zmij](https://github.com/dtolnay/zmij) | 1.0.21 | MIT |
