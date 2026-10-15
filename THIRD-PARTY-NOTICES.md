# THIRD-PARTY-NOTICES — 第三方组件声明

> 本文件由仓库脚本于 2026-10-13 基于 `src-tauri/Cargo.lock` 与 cargo/npm 注册表元数据生成,覆盖 PulseDeck 安装包分发的全部第三方代码与素材。
> 各组件的**精确版权声明以其实际 LICENSE 文件为准**;本文件同时随附本项目产物中出现的各开源许可证全文(附录)。

本项目的自有代码以 [Apache-2.0](LICENSE) 发布(Copyright 2026 janeluo)。

## 一、随包分发的独立第三方组件

以下组件以**独立程序/文件**形式随安装包分发,不经修改、不与本程序链接,各自附带其原始许可文件(`src-tauri/resources/pawnio/`):

| 组件 | 许可 | 说明 |
| --- | --- | --- |
| `PawnIO_setup.exe` | 专有(All rights reserved) | 官方安装器,**仅允许原样再分发**(其许可原文 "This installer can be redistributed unmodified");未做任何修改,详见 `resources/pawnio/NOTICE.md` |
| `IntelMSR.bin` / `AMDFamily17.bin` | LGPL-2.1 | namazso/PawnIO.Modules v0.2.3 官方签名模块(Intel 读 MSR、AMD 读 SMN),原样分发,许可全文随包(`resources/pawnio/COPYING`);逐文件 SHA-256 见 `resources/pawnio/NOTICE.md` |
| PawnIO 驱动(由上述安装器安装) | GPL-2.0(含 DeviceIoControl 例外) | 本程序仅经 `DeviceIoControl` 与驱动通信,属例外明确豁免的情形;驱动由其官方安装器独立安装 |
| 微软 WebView2 Runtime | 微软免费再分发运行时 | 由系统/微软渠道提供,非本包内容 |

## 二、随包分发的素材

| 素材 | 来源 | 说明 |
| --- | --- | --- |
| 桌面宠物贴图(`src/assets/pet/*.png`,6 档 × 8 帧) | **豆包(字节跳动)AI 图像生成**,由本项目抠底/缩放加工 | 依豆包用户协议,平台不主张生成内容所有权、权利归用户;生成于 2026-10,为该项目专用素材 |
| 应用图标(`src-tauri/icons/*`) | 本项目自绘 | — |

## 三、运行时链接的 npm 依赖(随包分发)

| 包 | 版本 | 许可 | 版权 / 出处 |
| --- | --- | --- | --- |
| [@tauri-apps/api](https://github.com/tauri-apps/tauri) | 2.12.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |

## 四、静态链接的 Rust 依赖(随包分发)

以下 crate 经 cargo 静态链接进 `pulse-deck.exe`。许可栏为各 crate `Cargo.toml` 的 SPDX 表达式;「版权 / 出处」取自 crate 元数据的 authors 字段,缺失时以 docs.rs 链接代替。

| crate | 版本 | 许可 | 版权 / 出处 |
| --- | --- | --- | --- |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | Jonas Schievink, oyvindln |
| aho-corasick | 1.1.5 | Unlicense OR MIT | Andrew Gallant |
| alloc-no-stdlib | 3.0.0 | BSD-3-Clause | Daniel Reiter Horn |
| alloc-stdlib | 0.3.0 | BSD-3-Clause | Daniel Reiter Horn |
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | Nicolas Silva |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | David Tolnay |
| atk | 0.18.2 | MIT | The gtk-rs Project Developers |
| atk-sys | 0.18.2 | MIT | The gtk-rs Project Developers |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT | Stjepan Glavina, Contributors to futures-rs |
| auto-launch | 0.6.0 | MIT | zzzgydi |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | Josh Stone |
| base64 | 0.21.7 | MIT OR Apache-2.0 | Alice Maz, Marshall Pierce |
| base64 | 0.22.1 | MIT OR Apache-2.0 | Marshall Pierce |
| base64 | 0.23.1 | MIT OR Apache-2.0 | Marshall Pierce |
| bit-set | 0.8.0 | Apache-2.0 OR MIT | Alexis Beingessner |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT | Alexis Beingessner |
| bitflags | 1.3.2 | MIT OR Apache-2.0 | The Rust Project Developers |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | The Rust Project Developers |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | RustCrypto Developers |
| block2 | 0.6.2 | MIT | Mads Marquart |
| brotli | 9.0.0 | BSD-3-Clause AND MIT | Daniel Reiter Horn, The Brotli Authors |
| brotli-decompressor | 6.0.1 | BSD-3-Clause/MIT | Daniel Reiter Horn, The Brotli Authors |
| bs58 | 0.5.1 | MIT OR Apache-2.0 | [docs.rs/bs58/0.5.1](https://docs.rs/bs58/0.5.1) |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | Nick Fitzgerald |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | Lokathor |
| byteorder | 1.5.0 | Unlicense OR MIT | Andrew Gallant |
| bytes | 1.12.1 | MIT | Carl Lerche, Sean McArthur |
| cairo-rs | 0.18.5 | MIT | The gtk-rs Project Developers |
| cairo-sys-rs | 0.18.2 | MIT | The gtk-rs Project Developers |
| camino | 1.2.6 | MIT OR Apache-2.0 | Without Boats, Ashley Williams, Steve Klabnik, Rain |
| cargo_metadata | 0.19.2 | MIT | Oliver Schneider |
| cargo_toml | 1.0.1 | Apache-2.0 OR MIT | Kornel |
| cargo-platform | 0.1.9 | MIT OR Apache-2.0 | [docs.rs/cargo-platform/0.1.9](https://docs.rs/cargo-platform/0.1.9) |
| cc | 1.6.0 | MIT OR Apache-2.0 | [docs.rs/cc/1.6.0](https://docs.rs/cc/1.6.0) |
| cesu8 | 1.1.0 | Apache-2.0 OR MIT | Eric Kidd |
| cfb | 0.14.0 | MIT | Matthew D. Steele |
| cfg_aliases | 0.2.2 | MIT | [docs.rs/cfg_aliases/0.2.2](https://docs.rs/cfg_aliases/0.2.2) |
| cfg-expr | 0.15.8 | MIT OR Apache-2.0 | Embark, Jake Shadle |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | Alex Crichton |
| chrono | 0.4.45 | MIT OR Apache-2.0 | [docs.rs/chrono/0.4.45](https://docs.rs/chrono/0.4.45) |
| combine | 4.6.8 | MIT | Markus Westerlind |
| cookie | 0.18.2 | MIT OR Apache-2.0 | Sergio Benitez, Alex Crichton |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 | The Servo Project Developers |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | The Servo Project Developers |
| core-graphics | 0.25.0 | MIT OR Apache-2.0 | The Servo Project Developers |
| core-graphics-types | 0.2.0 | MIT OR Apache-2.0 | The Servo Project Developers |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | RustCrypto Developers |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 | Sam Rijs, Alex Crichton |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 | [docs.rs/crossbeam-channel/0.5.17](https://docs.rs/crossbeam-channel/0.5.17) |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | [docs.rs/crossbeam-utils/0.8.23](https://docs.rs/crossbeam-utils/0.8.23) |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | RustCrypto Developers |
| cssparser | 0.37.0 | MPL-2.0 | Simon Sapin |
| cssparser-macros | 0.7.1 | MPL-2.0 | Simon Sapin |
| ctor | 1.0.13 | Apache-2.0 OR MIT | Matt Mastracci |
| darling | 0.20.11 | MIT | Ted Driggs |
| darling | 0.24.1 | MIT | Ted Driggs |
| darling_core | 0.20.11 | MIT | Ted Driggs |
| darling_core | 0.24.1 | MIT | Ted Driggs |
| darling_macro | 0.20.11 | MIT | Ted Driggs |
| darling_macro | 0.24.1 | MIT | Ted Driggs |
| dbus | 0.9.12 | Apache-2.0 OR MIT | David Henningsson |
| defmt | 1.1.1 | MIT OR Apache-2.0 | The Knurling-rs developers |
| defmt-macros | 1.1.1 | MIT OR Apache-2.0 | The Knurling-rs developers |
| defmt-parser | 1.0.0 | MIT OR Apache-2.0 | The Knurling-rs developers |
| deranged | 0.5.8 | MIT OR Apache-2.0 | Jacob Pratt |
| derive_more | 2.1.1 | MIT | Jelte Fennema |
| derive_more-impl | 2.1.1 | MIT | Jelte Fennema |
| digest | 0.10.7 | MIT OR Apache-2.0 | RustCrypto Developers |
| dirs | 6.0.0 | MIT OR Apache-2.0 | [docs.rs/dirs/6.0.0](https://docs.rs/dirs/6.0.0) |
| dirs | 7.0.0 | MIT OR Apache-2.0 | Simon Ochsenreither |
| dirs-sys | 0.5.0 | MIT OR Apache-2.0 | Simon Ochsenreither |
| dispatch2 | 0.3.1 | Zlib OR Apache-2.0 OR MIT | Mads Marquart, Mary |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | Jane Lusby |
| dlopen2 | 0.8.2 | MIT | Szymon Wieloch, Ahmed Masud, OpenByte |
| dlopen2_derive | 0.4.3 | MIT | Szymon Wieloch, OpenByte |
| dom_query | 0.28.0 | MIT | niklak, importcjj |
| dpi | 0.1.2 | Apache-2.0 OR MIT | [docs.rs/dpi/0.1.2](https://docs.rs/dpi/0.1.2) |
| dtoa | 1.0.11 | MIT OR Apache-2.0 | David Tolnay |
| dtoa-short | 0.3.5 | MPL-2.0 | Xidorn Quan |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 | Kornel |
| dyn-clone | 1.0.20 | MIT OR Apache-2.0 | David Tolnay |
| embed_plist | 1.2.2 | MIT OR Apache-2.0 | Nikolai Vazquez |
| embed-resource | 3.0.12 | MIT | наб, Cat Plus Plus, Liigo, azyobuzin, Peter Atashian, pravic, Gabriel Majeri, SonnyX, Johan Andersson, Jordan Poles, MSxDOS, Jim McGrath, roblabla, Jasper Bekkers, Richard Markiewicz, Emerson de Freitas Barcelos, Li Keqing, Alexis Bourget, Michael Farrell, Jacob Okamoto, Marijn Suijten, Lucas Nogueira, CharlesChen0823, Daniel Schaefer, Rene Leonhardt, ssrlive, Kan-Ru Chen, Tony, Berrysoft, Marcus Ahlberg, Thomas Eizinger |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | [docs.rs/equivalent/1.0.2](https://docs.rs/equivalent/1.0.2) |
| erased-serde | 0.4.10 | MIT OR Apache-2.0 | David Tolnay |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | Stjepan Glavina |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | The image-rs Developers |
| field-offset | 0.3.6 | MIT OR Apache-2.0 | Diggory Blake |
| find-msvc-tools | 0.1.14 | MIT OR Apache-2.0 | [docs.rs/find-msvc-tools/0.1.14](https://docs.rs/find-msvc-tools/0.1.14) |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | Alex Crichton, Josh Triplett |
| fnv | 1.0.7 | Apache-2.0 OR MIT | Alex Crichton |
| foldhash | 0.2.0 | Zlib | Orson Peters |
| foreign-types | 0.5.0 | MIT OR Apache-2.0 | Steven Fackler |
| foreign-types-macros | 0.2.4 | MIT OR Apache-2.0 | Steven Fackler |
| foreign-types-shared | 0.3.1 | MIT OR Apache-2.0 | Steven Fackler |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | The rust-url developers |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 | [docs.rs/futures-channel/0.3.34](https://docs.rs/futures-channel/0.3.34) |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | [docs.rs/futures-core/0.3.34](https://docs.rs/futures-core/0.3.34) |
| futures-executor | 0.3.34 | MIT OR Apache-2.0 | [docs.rs/futures-executor/0.3.34](https://docs.rs/futures-executor/0.3.34) |
| futures-io | 0.3.34 | MIT OR Apache-2.0 | [docs.rs/futures-io/0.3.34](https://docs.rs/futures-io/0.3.34) |
| futures-macro | 0.3.34 | MIT OR Apache-2.0 | [docs.rs/futures-macro/0.3.34](https://docs.rs/futures-macro/0.3.34) |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | [docs.rs/futures-sink/0.3.34](https://docs.rs/futures-sink/0.3.34) |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | [docs.rs/futures-task/0.3.34](https://docs.rs/futures-task/0.3.34) |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | [docs.rs/futures-util/0.3.34](https://docs.rs/futures-util/0.3.34) |
| gdk | 0.18.2 | MIT | The gtk-rs Project Developers |
| gdk-pixbuf | 0.18.5 | MIT | The gtk-rs Project Developers |
| gdk-pixbuf-sys | 0.18.0 | MIT | The gtk-rs Project Developers |
| gdk-sys | 0.18.2 | MIT | The gtk-rs Project Developers |
| gdkwayland-sys | 0.18.2 | MIT | The gtk-rs Project Developers |
| gdkx11 | 0.18.2 | MIT | The gtk-rs Project Developers |
| gdkx11-sys | 0.18.2 | MIT | The gtk-rs Project Developers |
| generic-array | 0.14.7 | MIT | Bartłomiej Kamiński, Aaron Trent |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | The Rand Project Developers |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | The Rand Project Developers |
| gio | 0.18.4 | MIT | The gtk-rs Project Developers |
| gio-sys | 0.18.1 | MIT | The gtk-rs Project Developers |
| glib | 0.18.5 | MIT | The gtk-rs Project Developers |
| glib-macros | 0.18.5 | MIT | The gtk-rs Project Developers |
| glib-sys | 0.18.1 | MIT | The gtk-rs Project Developers |
| glob | 0.3.4 | MIT OR Apache-2.0 | The Rust Project Developers |
| gobject-sys | 0.18.0 | MIT | The gtk-rs Project Developers |
| gtk | 0.18.2 | MIT | The gtk-rs Project Developers |
| gtk-sys | 0.18.2 | MIT | The gtk-rs Project Developers |
| gtk3-macros | 0.18.2 | MIT | The gtk-rs Project Developers |
| hashbrown | 0.12.3 | MIT OR Apache-2.0 | Amanieu d'Antras |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | [docs.rs/hashbrown/0.17.1](https://docs.rs/hashbrown/0.17.1) |
| heck | 0.4.1 | MIT OR Apache-2.0 | Without Boats |
| heck | 0.5.0 | MIT OR Apache-2.0 | [docs.rs/heck/0.5.0](https://docs.rs/heck/0.5.0) |
| hex | 0.4.3 | MIT OR Apache-2.0 | KokaKiwi |
| html5ever | 0.39.0 | MIT OR Apache-2.0 | The html5ever Project Developers |
| http | 1.5.0 | MIT OR Apache-2.0 | Alex Crichton, Carl Lerche, Sean McArthur |
| http-body | 1.1.0 | MIT | Carl Lerche, Lucio Franco, Sean McArthur |
| http-body-util | 0.1.5 | MIT | Carl Lerche, Lucio Franco, Sean McArthur |
| httparse | 1.10.1 | MIT OR Apache-2.0 | Sean McArthur |
| hyper | 1.12.0 | MIT | Sean McArthur |
| hyper-util | 0.1.21 | MIT | Sean McArthur |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 | Andrew Straw, René Kijewski, Ryan Lopopolo |
| iana-time-zone-haiku | 0.1.2 | MIT OR Apache-2.0 | René Kijewski |
| ico | 0.5.0 | MIT | Matthew D. Steele |
| icu_collections | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_locale_core | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_normalizer | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_normalizer_data | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_properties | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_properties_data | 2.3.0 | Unicode-3.0 | The ICU4X Project Developers |
| icu_provider | 2.3.1 | Unicode-3.0 | The ICU4X Project Developers |
| ident_case | 1.0.1 | MIT OR Apache-2.0 | Ted Driggs |
| idna | 1.1.0 | MIT OR Apache-2.0 | The rust-url developers |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | The rust-url developers |
| indexmap | 1.9.3 | Apache-2.0 OR MIT | [docs.rs/indexmap/1.9.3](https://docs.rs/indexmap/1.9.3) |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | [docs.rs/indexmap/2.14.2](https://docs.rs/indexmap/2.14.2) |
| infer | 0.22.0 | MIT | Bojan |
| ipnet | 2.12.2 | MIT OR Apache-2.0 | Kris Price |
| itoa | 1.0.18 | MIT OR Apache-2.0 | David Tolnay |
| javascriptcore-rs | 1.1.2 | MIT | [docs.rs/javascriptcore-rs/1.1.2](https://docs.rs/javascriptcore-rs/1.1.2) |
| javascriptcore-rs-sys | 1.1.1 | MIT | The Gtk-rs Project Developers |
| jiff | 0.2.38 | Unlicense OR MIT | Andrew Gallant |
| jiff-core | 0.1.1 | Unlicense OR MIT | Andrew Gallant |
| jiff-static | 0.2.38 | Unlicense OR MIT | Andrew Gallant |
| jiff-tzdb | 0.1.9 | Unlicense OR MIT | Andrew Gallant |
| jiff-tzdb-platform | 0.1.3 | Unlicense OR MIT | Andrew Gallant |
| jni | 0.21.1 | MIT OR Apache-2.0 | Josh Chase |
| jni-sys | 0.3.1 | MIT OR Apache-2.0 | Steven Fackler |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 | Steven Fackler, Robert Bragg |
| jni-sys-macros | 0.4.1 | MIT OR Apache-2.0 | Robert Bragg |
| js-sys | 0.3.106 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| json-patch | 4.2.0 | MIT OR Apache-2.0 | Ivan Dubrov |
| jsonptr | 0.7.1 | MIT OR Apache-2.0 | chance dinkins, André Sá de Mello, Oliver Wangler |
| keyboard-types | 0.8.3 | MIT OR Apache-2.0 | Pyfisch |
| libappindicator | 0.9.0 | Apache-2.0 OR MIT | [docs.rs/libappindicator/0.9.0](https://docs.rs/libappindicator/0.9.0) |
| libappindicator-sys | 0.9.0 | Apache-2.0 OR MIT | [docs.rs/libappindicator-sys/0.9.0](https://docs.rs/libappindicator-sys/0.9.0) |
| libc | 0.2.190 | MIT OR Apache-2.0 | [docs.rs/libc/0.2.190](https://docs.rs/libc/0.2.190) |
| libdbus-sys | 0.2.7 | Apache-2.0 OR MIT | David Henningsson |
| libloading | 0.7.4 | ISC | Simonas Kazlauskas |
| libloading | 0.8.9 | ISC | Simonas Kazlauskas |
| libredox | 0.1.25 | MIT | 4lDO2 |
| litemap | 0.8.3 | Unicode-3.0 | The ICU4X Project Developers |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | Amanieu d'Antras |
| log | 0.4.34 | MIT OR Apache-2.0 | The Rust Project Developers |
| markup5ever | 0.39.0 | MIT OR Apache-2.0 | The html5ever Project Developers |
| memchr | 2.8.3 | Unlicense OR MIT | Andrew Gallant, bluss |
| memoffset | 0.9.1 | MIT | Gilad Naaman |
| mime | 0.3.17 | MIT OR Apache-2.0 | Sean McArthur |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | Frommi, oyvindln, Rich Geldreich richgel99@gmail.com |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | Frommi, oyvindln, Rich Geldreich richgel99@gmail.com |
| mio | 1.2.4 | MIT | Carl Lerche, Thomas de Zeeuw, Tokio Contributors |
| muda | 0.20.0 | Apache-2.0 OR MIT | [docs.rs/muda/0.20.0](https://docs.rs/muda/0.20.0) |
| ndk | 0.9.0 | MIT OR Apache-2.0 | The Rust Mobile contributors |
| ndk-context | 0.1.1 | MIT OR Apache-2.0 | The Rust Windowing contributors |
| ndk-sys | 0.6.0+11769913 | MIT OR Apache-2.0 | The Rust Windowing contributors |
| new_debug_unreachable | 1.0.6 | MIT | Matt Brubeck, Jonathan Reem |
| nix | 0.31.3 | MIT | [docs.rs/nix/0.31.3](https://docs.rs/nix/0.31.3) |
| ntapi | 0.4.3 | Apache-2.0 OR MIT | MSxDOS |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | Daniel Wagner-Hall, Daniel Henry-Mantilla, Vincent Esche |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | Daniel Wagner-Hall, Daniel Henry-Mantilla, Vincent Esche |
| num-conv | 0.2.2 | MIT OR Apache-2.0 | Jacob Pratt |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | The Rust Project Developers |
| nvml-wrapper | 0.13.0 | MIT OR Apache-2.0 | Cldfire |
| nvml-wrapper-sys | 0.10.0 | MIT OR Apache-2.0 | Cldfire |
| objc2 | 0.6.5 | MIT | Mads Marquart |
| objc2-app-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-app-kit/0.3.2](https://docs.rs/objc2-app-kit/0.3.2) |
| objc2-cloud-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-cloud-kit/0.3.2](https://docs.rs/objc2-cloud-kit/0.3.2) |
| objc2-core-data | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-core-data/0.3.2](https://docs.rs/objc2-core-data/0.3.2) |
| objc2-core-foundation | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-core-foundation/0.3.2](https://docs.rs/objc2-core-foundation/0.3.2) |
| objc2-core-graphics | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-core-graphics/0.3.2](https://docs.rs/objc2-core-graphics/0.3.2) |
| objc2-core-image | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-core-image/0.3.2](https://docs.rs/objc2-core-image/0.3.2) |
| objc2-core-location | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-core-location/0.3.2](https://docs.rs/objc2-core-location/0.3.2) |
| objc2-core-text | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-core-text/0.3.2](https://docs.rs/objc2-core-text/0.3.2) |
| objc2-encode | 4.1.0 | MIT | Mads Marquart |
| objc2-exception-helper | 0.1.1 | Zlib OR Apache-2.0 OR MIT | Mads Marquart |
| objc2-foundation | 0.3.2 | MIT | [docs.rs/objc2-foundation/0.3.2](https://docs.rs/objc2-foundation/0.3.2) |
| objc2-io-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-io-kit/0.3.2](https://docs.rs/objc2-io-kit/0.3.2) |
| objc2-io-surface | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-io-surface/0.3.2](https://docs.rs/objc2-io-surface/0.3.2) |
| objc2-open-directory | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-open-directory/0.3.2](https://docs.rs/objc2-open-directory/0.3.2) |
| objc2-quartz-core | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-quartz-core/0.3.2](https://docs.rs/objc2-quartz-core/0.3.2) |
| objc2-security | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-security/0.3.2](https://docs.rs/objc2-security/0.3.2) |
| objc2-service-management | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-service-management/0.3.2](https://docs.rs/objc2-service-management/0.3.2) |
| objc2-ui-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-ui-kit/0.3.2](https://docs.rs/objc2-ui-kit/0.3.2) |
| objc2-user-notifications | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-user-notifications/0.3.2](https://docs.rs/objc2-user-notifications/0.3.2) |
| objc2-web-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | [docs.rs/objc2-web-kit/0.3.2](https://docs.rs/objc2-web-kit/0.3.2) |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | Aleksey Kladov |
| option-ext | 0.2.0 | MPL-2.0 | Simon Ochsenreither |
| os_info | 3.15.0 | MIT | Jan Schulte, Stanislav Tkach |
| pango | 0.18.3 | MIT | The gtk-rs Project Developers |
| pango-sys | 0.18.0 | MIT | The gtk-rs Project Developers |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | Amanieu d'Antras |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | Amanieu d'Antras |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | The rust-url developers |
| phf | 0.13.1 | MIT | Steven Fackler |
| phf_codegen | 0.13.1 | MIT | Steven Fackler |
| phf_generator | 0.13.1 | MIT | Steven Fackler |
| phf_macros | 0.13.1 | MIT | Steven Fackler |
| phf_shared | 0.13.1 | MIT | Steven Fackler |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | [docs.rs/pin-project-lite/0.2.17](https://docs.rs/pin-project-lite/0.2.17) |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | Alex Crichton |
| plist | 1.10.1 | MIT | Ed Barnard |
| png | 0.17.16 | MIT OR Apache-2.0 | The image-rs Developers |
| png | 0.18.1 | MIT OR Apache-2.0 | The image-rs Developers |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | [docs.rs/portable-atomic/1.15.0](https://docs.rs/portable-atomic/1.15.0) |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | [docs.rs/portable-atomic-util/0.2.8](https://docs.rs/portable-atomic-util/0.2.8) |
| potential_utf | 0.1.6 | Unicode-3.0 | The ICU4X Project Developers |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 | Jacob Pratt |
| precomputed-hash | 0.1.1 | MIT | Emilio Cobos Álvarez |
| proc-macro-crate | 1.3.1 | MIT OR Apache-2.0 | Bastian Köcher |
| proc-macro-crate | 2.0.2 | MIT OR Apache-2.0 | Bastian Köcher |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | Bastian Köcher |
| proc-macro-error | 1.0.4 | MIT OR Apache-2.0 | CreepySkeleton |
| proc-macro-error-attr | 1.0.4 | MIT OR Apache-2.0 | CreepySkeleton |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | David Tolnay, Alex Crichton |
| quick-xml | 0.42.0 | MIT | [docs.rs/quick-xml/0.42.0](https://docs.rs/quick-xml/0.42.0) |
| quote | 1.0.47 | MIT OR Apache-2.0 | David Tolnay |
| r-efi | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | [docs.rs/r-efi/5.3.0](https://docs.rs/r-efi/5.3.0) |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | [docs.rs/r-efi/6.0.0](https://docs.rs/r-efi/6.0.0) |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | Osspial |
| redox_syscall | 0.5.18 | MIT | Jeremy Soller |
| redox_users | 0.5.3 | MIT | Jose Narvaez, Wesley Hershberger |
| ref-cast | 1.0.27 | MIT OR Apache-2.0 | David Tolnay |
| ref-cast-impl | 1.0.27 | MIT OR Apache-2.0 | David Tolnay |
| regex | 1.13.1 | MIT OR Apache-2.0 | The Rust Project Developers, Andrew Gallant |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | The Rust Project Developers, Andrew Gallant |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | The Rust Project Developers, Andrew Gallant |
| reqwest | 0.13.5 | MIT OR Apache-2.0 | Sean McArthur |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 | [docs.rs/rustc_version/0.4.1](https://docs.rs/rustc_version/0.4.1) |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT | The Rust Project Developers |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | David Tolnay |
| same-file | 1.0.6 | Unlicense/MIT | Andrew Gallant |
| schemars | 0.8.22 | MIT | Graham Esau |
| schemars | 0.9.0 | MIT | Graham Esau |
| schemars | 1.2.2 | MIT | Graham Esau |
| schemars_derive | 0.8.22 | MIT | Graham Esau |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | bluss |
| selectors | 0.38.0 | MPL-2.0 | The Servo Project Developers |
| semver | 1.0.28 | MIT OR Apache-2.0 | David Tolnay |
| serde | 1.0.229 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_derive_internals | 0.29.1 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | Erick Tryzelaar, David Tolnay |
| serde_repr | 0.1.21 | MIT OR Apache-2.0 | David Tolnay |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | [docs.rs/serde_spanned/0.6.9](https://docs.rs/serde_spanned/0.6.9) |
| serde_spanned | 1.1.2 | MIT OR Apache-2.0 | [docs.rs/serde_spanned/1.1.2](https://docs.rs/serde_spanned/1.1.2) |
| serde_with | 3.24.0 | MIT OR Apache-2.0 | Jonas Bushart, Marcin Kaźmierczak |
| serde_with_macros | 3.24.0 | MIT OR Apache-2.0 | Jonas Bushart |
| serde-untagged | 0.1.9 | MIT OR Apache-2.0 | David Tolnay |
| serialize-to-javascript | 0.1.2 | MIT OR Apache-2.0 | Chip Reed |
| serialize-to-javascript-impl | 0.1.2 | MIT OR Apache-2.0 | Chip Reed |
| servo_arc | 0.4.3 | MIT OR Apache-2.0 | The Servo Project Developers |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | RustCrypto Developers |
| shlex | 2.0.1 | MIT OR Apache-2.0 | comex, Fenhl, Adrian Taylor, Alex Touchet, Daniel Parks, Garrett Berg |
| simd-adler32 | 0.3.10 | MIT | Marvin Countryman |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | Frank Denis |
| slab | 0.4.12 | MIT | Carl Lerche |
| smallvec | 1.16.2 | MIT OR Apache-2.0 | The Servo Project Developers |
| smappservice-rs | 0.1.3 | MIT | [docs.rs/smappservice-rs/0.1.3](https://docs.rs/smappservice-rs/0.1.3) |
| socket2 | 0.6.5 | MIT OR Apache-2.0 | Alex Crichton, Thomas de Zeeuw |
| softbuffer | 0.4.8 | MIT OR Apache-2.0 | [docs.rs/softbuffer/0.4.8](https://docs.rs/softbuffer/0.4.8) |
| soup3 | 0.5.0 | MIT | [docs.rs/soup3/0.5.0](https://docs.rs/soup3/0.5.0) |
| soup3-sys | 0.5.0 | MIT | The Gtk-rs Project Developers |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | Robert Grosse |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | Nikolai Vazquez |
| string_cache | 0.9.0 | MIT OR Apache-2.0 | The Servo Project Developers |
| string_cache_codegen | 0.6.1 | MIT OR Apache-2.0 | The Servo Project Developers |
| strsim | 0.11.1 | MIT | Danny Guo, maxbachmann |
| swift-rs | 1.0.8 | MIT OR Apache-2.0 | The swift-rs contributors |
| syn | 1.0.109 | MIT OR Apache-2.0 | David Tolnay |
| syn | 2.0.119 | MIT OR Apache-2.0 | David Tolnay |
| syn | 3.0.6 | MIT OR Apache-2.0 | David Tolnay |
| sync_wrapper | 1.0.2 | Apache-2.0 | Actyx AG |
| synstructure | 0.14.0 | MIT | Nika Layzell |
| sysinfo | 0.39.6 | MIT | Guillaume Gomez |
| system-deps | 6.2.2 | MIT OR Apache-2.0 | Guillaume Desmottes, Josh Triplett |
| tao | 0.37.1 | Apache-2.0 | Tauri Programme within The Commons Conservancy, The winit contributors |
| tao-macros | 0.1.4 | MIT OR Apache-2.0 | Tauri Programme within The Commons Conservancy |
| target-lexicon | 0.12.16 | Apache-2.0 WITH LLVM-exception | Dan Gohman |
| tauri | 2.12.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| tauri-build | 2.7.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| tauri-codegen | 2.7.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| tauri-macros | 2.7.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| tauri-plugin | 2.7.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| tauri-plugin-autostart | 2.7.0 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| tauri-runtime | 2.12.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| tauri-runtime-wry | 2.12.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| tauri-utils | 2.10.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| tauri-winres | 0.3.6 | MIT | Tauri Programme within The Commons Conservancy, Max Resch |
| tendril | 0.5.1 | MIT OR Apache-2.0 | Keegan McAllister, Simon Sapin, Chris Morgan |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | David Tolnay |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | David Tolnay |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 | David Tolnay |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | David Tolnay |
| time | 0.3.55 | MIT OR Apache-2.0 | Jacob Pratt, Time contributors |
| time-core | 0.1.9 | MIT OR Apache-2.0 | Jacob Pratt, Time contributors |
| time-macros | 0.2.32 | MIT OR Apache-2.0 | Jacob Pratt, Time contributors |
| tinystr | 0.8.4 | Unicode-3.0 | The ICU4X Project Developers |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | Lokathor |
| tokio | 1.53.2 | MIT | Tokio Contributors |
| tokio-util | 0.7.19 | MIT | Tokio Contributors |
| toml | 0.8.2 | MIT OR Apache-2.0 | Alex Crichton |
| toml | 1.1.7+spec-1.1.0 | MIT OR Apache-2.0 | [docs.rs/toml/1.1.7+spec-1.1.0](https://docs.rs/toml/1.1.7+spec-1.1.0) |
| toml_datetime | 0.6.3 | MIT OR Apache-2.0 | Alex Crichton |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | [docs.rs/toml_datetime/1.1.2+spec-1.1.0](https://docs.rs/toml_datetime/1.1.2+spec-1.1.0) |
| toml_edit | 0.19.15 | MIT OR Apache-2.0 | Andronik Ordian, Ed Page |
| toml_edit | 0.20.2 | MIT OR Apache-2.0 | Andronik Ordian, Ed Page |
| toml_edit | 0.25.16+spec-1.1.0 | MIT OR Apache-2.0 | [docs.rs/toml_edit/0.25.16+spec-1.1.0](https://docs.rs/toml_edit/0.25.16+spec-1.1.0) |
| toml_parser | 1.1.4+spec-1.1.0 | MIT OR Apache-2.0 | [docs.rs/toml_parser/1.1.4+spec-1.1.0](https://docs.rs/toml_parser/1.1.4+spec-1.1.0) |
| toml_writer | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | [docs.rs/toml_writer/1.1.3+spec-1.1.0](https://docs.rs/toml_writer/1.1.3+spec-1.1.0) |
| tower | 0.5.3 | MIT | Tower Maintainers |
| tower-http | 0.6.11 | MIT | Tower Maintainers |
| tower-layer | 0.3.3 | MIT | Tower Maintainers |
| tower-service | 0.3.3 | MIT | Tower Maintainers |
| tracing | 0.1.44 | MIT | Eliza Weisman, Tokio Contributors |
| tracing-core | 0.1.36 | MIT | Tokio Contributors |
| tray-icon | 0.25.1 | MIT OR Apache-2.0 | [docs.rs/tray-icon/0.25.1](https://docs.rs/tray-icon/0.25.1) |
| try-lock | 0.2.5 | MIT | Sean McArthur |
| typeid | 1.0.3 | MIT OR Apache-2.0 | David Tolnay |
| typenum | 1.20.1 | MIT OR Apache-2.0 | [docs.rs/typenum/1.20.1](https://docs.rs/typenum/1.20.1) |
| unicode-ident | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | David Tolnay |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | kwantam, Manish Goregaokar |
| url | 2.5.8 | MIT OR Apache-2.0 | The rust-url developers |
| urlpattern | 0.6.0 | MIT | the Deno authors, crowlKats |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT | Henri Sivonen |
| uuid | 1.27.0 | Apache-2.0 OR MIT | Ashley Mannix, Dylan DPC, Hunar Roop Kahlon |
| version_check | 0.9.5 | MIT OR Apache-2.0 | Sergio Benitez |
| version-compare | 0.2.1 | MIT | Tim Visee |
| vswhom | 0.1.0 | MIT | nabijaczleweli |
| vswhom-sys | 0.1.3 | MIT | наб, forrestsmithfb |
| walkdir | 2.5.0 | Unlicense/MIT | Andrew Gallant |
| want | 0.3.2 | MIT | Sean McArthur |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | The Cranelift Project Developers |
| wasip2 | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [docs.rs/wasip2/1.0.4+wasi-0.2.12](https://docs.rs/wasip2/1.0.4+wasi-0.2.12) |
| wasm-bindgen | 0.2.129 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wasm-bindgen-futures | 0.4.79 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wasm-bindgen-macro | 0.2.129 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wasm-bindgen-macro-support | 0.2.129 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wasm-bindgen-shared | 0.2.129 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| wasm-streams | 0.5.0 | MIT OR Apache-2.0 | Mattias Buelens |
| web_atoms | 0.2.6 | MIT OR Apache-2.0 | The html5ever Project Developers |
| web-sys | 0.3.106 | MIT OR Apache-2.0 | The wasm-bindgen Developers |
| web-time | 1.1.0 | MIT OR Apache-2.0 | [docs.rs/web-time/1.1.0](https://docs.rs/web-time/1.1.0) |
| webkit2gtk | 2.0.2 | MIT | [docs.rs/webkit2gtk/2.0.2](https://docs.rs/webkit2gtk/2.0.2) |
| webkit2gtk-sys | 2.0.2 | MIT | [docs.rs/webkit2gtk-sys/2.0.2](https://docs.rs/webkit2gtk-sys/2.0.2) |
| webview2-com | 0.39.1 | MIT | [docs.rs/webview2-com/0.39.1](https://docs.rs/webview2-com/0.39.1) |
| webview2-com-macros | 0.8.1 | MIT | [docs.rs/webview2-com-macros/0.8.1](https://docs.rs/webview2-com-macros/0.8.1) |
| webview2-com-sys | 0.39.1 | MIT | [docs.rs/webview2-com-sys/0.39.1](https://docs.rs/webview2-com-sys/0.39.1) |
| winapi | 0.3.9 | MIT OR Apache-2.0 | Peter Atashian |
| winapi-i686-pc-windows-gnu | 0.4.0 | MIT OR Apache-2.0 | Peter Atashian |
| winapi-util | 0.1.11 | Unlicense OR MIT | Andrew Gallant |
| winapi-x86_64-pc-windows-gnu | 0.4.0 | MIT OR Apache-2.0 | Peter Atashian |
| window-vibrancy | 0.8.1 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| windows | 0.62.2 | MIT OR Apache-2.0 | [docs.rs/windows/0.62.2](https://docs.rs/windows/0.62.2) |
| windows_aarch64_gnullvm | 0.42.2 | MIT OR Apache-2.0 | Microsoft |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_aarch64_msvc | 0.42.2 | MIT OR Apache-2.0 | Microsoft |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_gnu | 0.42.2 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_msvc | 0.42.2 | MIT OR Apache-2.0 | Microsoft |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_gnu | 0.42.2 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_gnullvm | 0.42.2 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_msvc | 0.42.2 | MIT OR Apache-2.0 | Microsoft |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | [docs.rs/windows-collections/0.3.2](https://docs.rs/windows-collections/0.3.2) |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | [docs.rs/windows-core/0.62.2](https://docs.rs/windows-core/0.62.2) |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | [docs.rs/windows-future/0.3.2](https://docs.rs/windows-future/0.3.2) |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | [docs.rs/windows-implement/0.60.2](https://docs.rs/windows-implement/0.60.2) |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | [docs.rs/windows-interface/0.59.3](https://docs.rs/windows-interface/0.59.3) |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | [docs.rs/windows-link/0.2.1](https://docs.rs/windows-link/0.2.1) |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | [docs.rs/windows-numerics/0.3.1](https://docs.rs/windows-numerics/0.3.1) |
| windows-registry | 0.6.1 | MIT OR Apache-2.0 | [docs.rs/windows-registry/0.6.1](https://docs.rs/windows-registry/0.6.1) |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | [docs.rs/windows-result/0.4.1](https://docs.rs/windows-result/0.4.1) |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | [docs.rs/windows-strings/0.5.1](https://docs.rs/windows-strings/0.5.1) |
| windows-sys | 0.45.0 | MIT OR Apache-2.0 | Microsoft |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 | Microsoft |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | [docs.rs/windows-sys/0.61.2](https://docs.rs/windows-sys/0.61.2) |
| windows-targets | 0.42.2 | MIT OR Apache-2.0 | Microsoft |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | Microsoft |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | [docs.rs/windows-threading/0.2.1](https://docs.rs/windows-threading/0.2.1) |
| windows-version | 0.1.7 | MIT OR Apache-2.0 | [docs.rs/windows-version/0.1.7](https://docs.rs/windows-version/0.1.7) |
| winnow | 0.5.40 | MIT | [docs.rs/winnow/0.5.40](https://docs.rs/winnow/0.5.40) |
| winnow | 1.0.4 | MIT | [docs.rs/winnow/1.0.4](https://docs.rs/winnow/1.0.4) |
| winreg | 0.56.0 | MIT | Igor Shaula |
| wit-bindgen | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Alex Crichton |
| wrapcenum-derive | 0.4.1 | MIT OR Apache-2.0 | Cldfire |
| writeable | 0.6.4 | Unicode-3.0 | The ICU4X Project Developers |
| wry | 0.57.0 | Apache-2.0 OR MIT | Tauri Programme within The Commons Conservancy |
| x11 | 2.21.0 | MIT | daggerbot, Erle Pereira, AltF02 |
| x11-dl | 2.21.0 | MIT | daggerbot, Erle Pereira, AltF02 |
| yoke | 0.8.3 | Unicode-3.0 | Manish Goregaokar |
| yoke-derive | 0.8.4 | Unicode-3.0 | Manish Goregaokar |
| zerofrom | 0.1.8 | Unicode-3.0 | The ICU4X Project Developers |
| zerofrom-derive | 0.1.8 | Unicode-3.0 | Manish Goregaokar |
| zerotrie | 0.2.5 | Unicode-3.0 | The ICU4X Project Developers |
| zerovec | 0.11.8 | Unicode-3.0 | The ICU4X Project Developers |
| zerovec-derive | 0.11.6 | Unicode-3.0 | Manish Goregaokar |
| zlib-rs | 0.6.8 | Zlib | [docs.rs/zlib-rs/0.6.8](https://docs.rs/zlib-rs/0.6.8) |
| zmij | 1.0.23 | MIT | David Tolnay |

共 439 个 crate。其中 `nix`、`dirs 6.0`、`cfg_aliases`、`objc2-*`、`smappservice-rs` 等仅为其他平台(macOS/Linux)的传递依赖,不会编入 Windows 产物,一并列出仅为完整。

## 五、仅构建期使用的 npm 依赖(不随包分发,仅供核查)

| 包 | 版本 | 许可 |
| --- | --- | --- |
| vite | 7.3.7 | MIT |
| esbuild | 0.28.2 | MIT |
| rollup | 4.64.3 | MIT |
| postcss | 8.5.29 | MIT |
| nanoid | 3.3.20 | MIT |
| picocolors | 1.1.1 | ISC |
| picomatch | 4.0.7 | MIT |
| fdir | 6.5.0 | MIT |
| tinyglobby | 0.2.17 | MIT |
| source-map-js | 1.2.2 | BSD-3-Clause |
| @types/estree | 1.0.9 | MIT |
| @tauri-apps/cli(含平台二进制) | 2.12.1 | Apache-2.0 OR MIT |
| @esbuild/*、@rollup/* 平台二进制 | 同主包 | MIT |

## 附录:许可证全文

本项目产物中出现的各开源许可证原文如下(同一许可证只收录一份)。来源为各 crate 发布包内自带的 LICENSE 文件。
> 注:部分 crate 以「多许可任选其一」方式发布(如 `MIT OR Apache-2.0`),本项目按惯例优先采用 MIT/Apache 选项;附录仍收录其表达式涉及的全部许可证文本以备核查。

### Apache-2.0

> 摘自 adler2-2.0.1 发布包

```text
                              Apache License
                        Version 2.0, January 2004
                     https://www.apache.org/licenses/LICENSE-2.0

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

   "License" shall mean the terms and conditions for use, reproduction,
   and distribution as defined by Sections 1 through 9 of this document.

   "Licensor" shall mean the copyright owner or entity authorized by
   the copyright owner that is granting the License.

   "Legal Entity" shall mean the union of the acting entity and all
   other entities that control, are controlled by, or are under common
   control with that entity. For the purposes of this definition,
   "control" means (i) the power, direct or indirect, to cause the
   direction or management of such entity, whether by contract or
   otherwise, or (ii) ownership of fifty percent (50%) or more of the
   outstanding shares, or (iii) beneficial ownership of such entity.

   "You" (or "Your") shall mean an individual or Legal Entity
   exercising permissions granted by this License.

   "Source" form shall mean the preferred form for making modifications,
   including but not limited to software source code, documentation
   source, and configuration files.

   "Object" form shall mean any form resulting from mechanical
   transformation or translation of a Source form, including but
   not limited to compiled object code, generated documentation,
   and conversions to other media types.

   "Work" shall mean the work of authorship, whether in Source or
   Object form, made available under the License, as indicated by a
   copyright notice that is included in or attached to the work
   (an example is provided in the Appendix below).

   "Derivative Works" shall mean any work, whether in Source or Object
   form, that is based on (or derived from) the Work and for which the
   editorial revisions, annotations, elaborations, or other modifications
   represent, as a whole, an original work of authorship. For the purposes
   of this License, Derivative Works shall not include works that remain
   separable from, or merely link (or bind by name) to the interfaces of,
   the Work and Derivative Works thereof.

   "Contribution" shall mean any work of authorship, including
   the original version of the Work and any modifications or additions
   to that Work or Derivative Works thereof, that is intentionally
   submitted to Licensor for inclusion in the Work by the copyright owner
   or by an individual or Legal Entity authorized to submit on behalf of
   the copyright owner. For the purposes of this definition, "submitted"
   means any form of electronic, verbal, or written communication sent
   to the Licensor or its representatives, including but not limited to
   communication on electronic mailing lists, source code control systems,
   and issue tracking systems that are managed by, or on behalf of, the
   Licensor for the purpose of discussing and improving the Work, but
   excluding communication that is conspicuously marked or otherwise
   designated in writing by the copyright owner as "Not a Contribution."

   "Contributor" shall mean Licensor and any individual or Legal Entity
   on behalf of whom a Contribution has been received by Licensor and
   subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   copyright license to reproduce, prepare Derivative Works of,
   publicly display, publicly perform, sublicense, and distribute the
   Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of
   this License, each Contributor hereby grants to You a perpetual,
   worldwide, non-exclusive, no-charge, royalty-free, irrevocable
   (except as stated in this section) patent license to make, have made,
   use, offer to sell, sell, import, and otherwise transfer the Work,
   where such license applies only to those patent claims licensable
   by such Contributor that are necessarily infringed by their
   Contribution(s) alone or by combination of their Contribution(s)
   with the Work to which such Contribution(s) was submitted. If You
   institute patent litigation against any entity (including a
   cross-claim or counterclaim in a lawsuit) alleging that the Work
   or a Contribution incorporated within the Work constitutes direct
   or contributory patent infringement, then any patent licenses
   granted to You under this License for that Work shall terminate
   as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the
   Work or Derivative Works thereof in any medium, with or without
   modifications, and in Source or Object form, provided that You
   meet the following conditions:

   (a) You must give any other recipients of the Work or
       Derivative Works a copy of this License; and

   (b) You must cause any modified files to carry prominent notices
       stating that You changed the files; and

   (c) You must retain, in the Source form of any Derivative Works
       that You distribute, all copyright, patent, trademark, and
       attribution notices from the Source form of the Work,
       excluding those notices that do not pertain to any part of
       the Derivative Works; and

   (d) If the Work includes a "NOTICE" text file as part of its
       distribution, then any Derivative Works that You distribute must
       include a readable copy of the attribution notices contained
       within such NOTICE file, excluding those notices that do not
       pertain to any part of the Derivative Works, in at least one
       of the following places: within a NOTICE text file distributed
       as part of the Derivative Works; within the Source form or
       documentation, if provided along with the Derivative Works; or,
       within a display generated by the Derivative Works, if and
       wherever such third-party notices normally appear. The contents
       of the NOTICE file are for informational purposes only and
       do not modify the License. You may add Your own attribution
       notices within Derivative Works that You distribute, alongside
       or as an addendum to the NOTICE text from the Work, provided
       that such additional attribution notices cannot be construed
       as modifying the License.

   You may add Your own copyright statement to Your modifications and
   may provide additional or different license terms and conditions
   for use, reproduction, or distribution of Your modifications, or
   for any such Derivative Works as a whole, provided Your use,
   reproduction, and distribution of the Work otherwise complies with
   the conditions stated in this License.

5. Submission of Contributions. Unless You explicitly state otherwise,
   any Contribution intentionally submitted for inclusion in the Work
   by You to the Licensor shall be under the terms and conditions of
   this License, without any additional terms or conditions.
   Notwithstanding the above, nothing herein shall supersede or modify
   the terms of any separate license agreement you may have executed
   with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade
   names, trademarks, service marks, or product names of the Licensor,
   except as required for reasonable and customary use in describing the
   origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or
   agreed to in writing, Licensor provides the Work (and each
   Contributor provides its Contributions) on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
   implied, including, without limitation, any warranties or conditions
   of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
   PARTICULAR PURPOSE. You are solely responsible for determining the
   appropriateness of using or redistributing the Work and assume any
   risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory,
   whether in tort (including negligence), contract, or otherwise,
   unless required by applicable law (such as deliberate and grossly
   negligent acts) or agreed to in writing, shall any Contributor be
   liable to You for damages, including any direct, indirect, special,
   incidental, or consequential damages of any character arising as a
   result of this License or out of the use or inability to use the
   Work (including but not limited to damages for loss of goodwill,
   work stoppage, computer failure or malfunction, or any and all
   other commercial damages or losses), even if such Contributor
   has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing
   the Work or Derivative Works thereof, You may choose to offer,
   and charge a fee for, acceptance of support, warranty, indemnity,
   or other liability obligations and/or rights consistent with this
   License. However, in accepting such obligations, You may act only
   on Your own behalf and on Your sole responsibility, not on behalf
   of any other Contributor, and only if You agree to indemnify,
   defend, and hold each Contributor harmless for any liability
   incurred by, or claims asserted against, such Contributor by reason
   of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

APPENDIX: How to apply the Apache License to your work.

   To apply the Apache License to your work, attach the following
   boilerplate notice, with the fields enclosed by brackets "[]"
   replaced with your own identifying information. (Don't include
   the brackets!)  The text should be enclosed in the appropriate
   comment syntax for the file format. We also recommend that a
   file or class name and description of purpose be included on the
   same "printed page" as the copyright notice for easier
   identification within third-party archives.

Copyright [yyyy] [name of copyright owner]

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

	https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

### MIT

> 摘自 adler2-2.0.1 发布包

```text
Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

### BSD-3-Clause

> 摘自 alloc-no-stdlib-3.0.0 发布包

```text
Copyright (c) 2016 Dropbox, Inc.
All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### ISC

全文见:[ISC 官方文本](https://spdx.org/licenses/ISC.html)。

### 0BSD

全文见:[0BSD 官方文本](https://spdx.org/licenses/0BSD.html)。

### Zlib

> 摘自 bytemuck-1.25.2 发布包

```text
Copyright (c) 2019 Daniel "Lokathor" Gee.

This software is provided 'as-is', without any express or implied warranty. In no event will the authors be held liable for any damages arising from the use of this software.

Permission is granted to anyone to use this software for any purpose, including commercial applications, and to alter it and redistribute it freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim that you wrote the original software. If you use this software in a product, an acknowledgment in the product documentation would be appreciated but is not required.

2. Altered source versions must be plainly marked as such, and must not be misrepresented as being the original software.

3. This notice may not be removed or altered from any source distribution.
```

### MPL-2.0

> 摘自 cssparser-0.37.0 发布包

```text
Mozilla Public License Version 2.0
==================================

1. Definitions
--------------

1.1. "Contributor"
    means each individual or legal entity that creates, contributes to
    the creation of, or owns Covered Software.

1.2. "Contributor Version"
    means the combination of the Contributions of others (if any) used
    by a Contributor and that particular Contributor's Contribution.

1.3. "Contribution"
    means Covered Software of a particular Contributor.

1.4. "Covered Software"
    means Source Code Form to which the initial Contributor has attached
    the notice in Exhibit A, the Executable Form of such Source Code
    Form, and Modifications of such Source Code Form, in each case
    including portions thereof.

1.5. "Incompatible With Secondary Licenses"
    means

    (a) that the initial Contributor has attached the notice described
        in Exhibit B to the Covered Software; or

    (b) that the Covered Software was made available under the terms of
        version 1.1 or earlier of the License, but not also under the
        terms of a Secondary License.

1.6. "Executable Form"
    means any form of the work other than Source Code Form.

1.7. "Larger Work"
    means a work that combines Covered Software with other material, in 
    a separate file or files, that is not Covered Software.

1.8. "License"
    means this document.

1.9. "Licensable"
    means having the right to grant, to the maximum extent possible,
    whether at the time of the initial grant or subsequently, any and
    all of the rights conveyed by this License.

1.10. "Modifications"
    means any of the following:

    (a) any file in Source Code Form that results from an addition to,
        deletion from, or modification of the contents of Covered
        Software; or

    (b) any new file in Source Code Form that contains any Covered
        Software.

1.11. "Patent Claims" of a Contributor
    means any patent claim(s), including without limitation, method,
    process, and apparatus claims, in any patent Licensable by such
    Contributor that would be infringed, but for the grant of the
    License, by the making, using, selling, offering for sale, having
    made, import, or transfer of either its Contributions or its
    Contributor Version.

1.12. "Secondary License"
    means either the GNU General Public License, Version 2.0, the GNU
    Lesser General Public License, Version 2.1, the GNU Affero General
    Public License, Version 3.0, or any later versions of those
    licenses.

1.13. "Source Code Form"
    means the form of the work preferred for making modifications.

1.14. "You" (or "Your")
    means an individual or a legal entity exercising rights under this
    License. For legal entities, "You" includes any entity that
    controls, is controlled by, or is under common control with You. For
    purposes of this definition, "control" means (a) the power, direct
    or indirect, to cause the direction or management of such entity,
    whether by contract or otherwise, or (b) ownership of more than
    fifty percent (50%) of the outstanding shares or beneficial
    ownership of such entity.

2. License Grants and Conditions
--------------------------------

2.1. Grants

Each Contributor hereby grants You a world-wide, royalty-free,
non-exclusive license:

(a) under intellectual property rights (other than patent or trademark)
    Licensable by such Contributor to use, reproduce, make available,
    modify, display, perform, distribute, and otherwise exploit its
    Contributions, either on an unmodified basis, with Modifications, or
    as part of a Larger Work; and

(b) under Patent Claims of such Contributor to make, use, sell, offer
    for sale, have made, import, and otherwise transfer either its
    Contributions or its Contributor Version.

2.2. Effective Date

The licenses granted in Section 2.1 with respect to any Contribution
become effective for each Contribution on the date the Contributor first
distributes such Contribution.

2.3. Limitations on Grant Scope

The licenses granted in this Section 2 are the only rights granted under
this License. No additional rights or licenses will be implied from the
distribution or licensing of Covered Software under this License.
Notwithstanding Section 2.1(b) above, no patent license is granted by a
Contributor:

(a) for any code that a Contributor has removed from Covered Software;
    or

(b) for infringements caused by: (i) Your and any other third party's
    modifications of Covered Software, or (ii) the combination of its
    Contributions with other software (except as part of its Contributor
    Version); or

(c) under Patent Claims infringed by Covered Software in the absence of
    its Contributions.

This License does not grant any rights in the trademarks, service marks,
or logos of any Contributor (except as may be necessary to comply with
the notice requirements in Section 3.4).

2.4. Subsequent Licenses

No Contributor makes additional grants as a result of Your choice to
distribute the Covered Software under a subsequent version of this
License (see Section 10.2) or under the terms of a Secondary License (if
permitted under the terms of Section 3.3).

2.5. Representation

Each Contributor represents that the Contributor believes its
Contributions are its original creation(s) or it has sufficient rights
to grant the rights to its Contributions conveyed by this License.

2.6. Fair Use

This License is not intended to limit any rights You have under
applicable copyright doctrines of fair use, fair dealing, or other
equivalents.

2.7. Conditions

Sections 3.1, 3.2, 3.3, and 3.4 are conditions of the licenses granted
in Section 2.1.

3. Responsibilities
-------------------

3.1. Distribution of Source Form

All distribution of Covered Software in Source Code Form, including any
Modifications that You create or to which You contribute, must be under
the terms of this License. You must inform recipients that the Source
Code Form of the Covered Software is governed by the terms of this
License, and how they can obtain a copy of this License. You may not
attempt to alter or restrict the recipients' rights in the Source Code
Form.

3.2. Distribution of Executable Form

If You distribute Covered Software in Executable Form then:

(a) such Covered Software must also be made available in Source Code
    Form, as described in Section 3.1, and You must inform recipients of
    the Executable Form how they can obtain a copy of such Source Code
    Form by reasonable means in a timely manner, at a charge no more
    than the cost of distribution to the recipient; and

(b) You may distribute such Executable Form under the terms of this
    License, or sublicense it under different terms, provided that the
    license for the Executable Form does not attempt to limit or alter
    the recipients' rights in the Source Code Form under this License.

3.3. Distribution of a Larger Work

You may create and distribute a Larger Work under terms of Your choice,
provided that You also comply with the requirements of this License for
the Covered Software. If the Larger Work is a combination of Covered
Software with a work governed by one or more Secondary Licenses, and the
Covered Software is not Incompatible With Secondary Licenses, this
License permits You to additionally distribute such Covered Software
under the terms of such Secondary License(s), so that the recipient of
the Larger Work may, at their option, further distribute the Covered
Software under the terms of either this License or such Secondary
License(s).

3.4. Notices

You may not remove or alter the substance of any license notices
(including copyright notices, patent notices, disclaimers of warranty,
or limitations of liability) contained within the Source Code Form of
the Covered Software, except that You may alter any license notices to
the extent required to remedy known factual inaccuracies.

3.5. Application of Additional Terms

You may choose to offer, and to charge a fee for, warranty, support,
indemnity or liability obligations to one or more recipients of Covered
Software. However, You may do so only on Your own behalf, and not on
behalf of any Contributor. You must make it absolutely clear that any
such warranty, support, indemnity, or liability obligation is offered by
You alone, and You hereby agree to indemnify every Contributor for any
liability incurred by such Contributor as a result of warranty, support,
indemnity or liability terms You offer. You may include additional
disclaimers of warranty and limitations of liability specific to any
jurisdiction.

4. Inability to Comply Due to Statute or Regulation
---------------------------------------------------

If it is impossible for You to comply with any of the terms of this
License with respect to some or all of the Covered Software due to
statute, judicial order, or regulation then You must: (a) comply with
the terms of this License to the maximum extent possible; and (b)
describe the limitations and the code they affect. Such description must
be placed in a text file included with all distributions of the Covered
Software under this License. Except to the extent prohibited by statute
or regulation, such description must be sufficiently detailed for a
recipient of ordinary skill to be able to understand it.

5. Termination
--------------

5.1. The rights granted under this License will terminate automatically
if You fail to comply with any of its terms. However, if You become
compliant, then the rights granted under this License from a particular
Contributor are reinstated (a) provisionally, unless and until such
Contributor explicitly and finally terminates Your grants, and (b) on an
ongoing basis, if such Contributor fails to notify You of the
non-compliance by some reasonable means prior to 60 days after You have
come back into compliance. Moreover, Your grants from a particular
Contributor are reinstated on an ongoing basis if such Contributor
notifies You of the non-compliance by some reasonable means, this is the
first time You have received notice of non-compliance with this License
from such Contributor, and You become compliant prior to 30 days after
Your receipt of the notice.

5.2. If You initiate litigation against any entity by asserting a patent
infringement claim (excluding declaratory judgment actions,
counter-claims, and cross-claims) alleging that a Contributor Version
directly or indirectly infringes any patent, then the rights granted to
You by any and all Contributors for the Covered Software under Section
2.1 of this License shall terminate.

5.3. In the event of termination under Sections 5.1 or 5.2 above, all
end user license agreements (excluding distributors and resellers) which
have been validly granted by You or Your distributors under this License
prior to termination shall survive termination.

************************************************************************
*                                                                      *
*  6. Disclaimer of Warranty                                           *
*  -------------------------                                           *
*                                                                      *
*  Covered Software is provided under this License on an "as is"       *
*  basis, without warranty of any kind, either expressed, implied, or  *
*  statutory, including, without limitation, warranties that the       *
*  Covered Software is free of defects, merchantable, fit for a        *
*  particular purpose or non-infringing. The entire risk as to the     *
*  quality and performance of the Covered Software is with You.        *
*  Should any Covered Software prove defective in any respect, You     *
*  (not any Contributor) assume the cost of any necessary servicing,   *
*  repair, or correction. This disclaimer of warranty constitutes an   *
*  essential part of this License. No use of any Covered Software is   *
*  authorized under this License except under this disclaimer.         *
*                                                                      *
************************************************************************

************************************************************************
*                                                                      *
*  7. Limitation of Liability                                          *
*  --------------------------                                          *
*                                                                      *
*  Under no circumstances and under no legal theory, whether tort      *
*  (including negligence), contract, or otherwise, shall any           *
*  Contributor, or anyone who distributes Covered Software as          *
*  permitted above, be liable to You for any direct, indirect,         *
*  special, incidental, or consequential damages of any character      *
*  including, without limitation, damages for lost profits, loss of    *
*  goodwill, work stoppage, computer failure or malfunction, or any    *
*  and all other commercial damages or losses, even if such party      *
*  shall have been informed of the possibility of such damages. This   *
*  limitation of liability shall not apply to liability for death or   *
*  personal injury resulting from such party's negligence to the       *
*  extent applicable law prohibits such limitation. Some               *
*  jurisdictions do not allow the exclusion or limitation of           *
*  incidental or consequential damages, so this exclusion and          *
*  limitation may not apply to You.                                    *
*                                                                      *
************************************************************************

8. Litigation
-------------

Any litigation relating to this License may be brought only in the
courts of a jurisdiction where the defendant maintains its principal
place of business and such litigation shall be governed by laws of that
jurisdiction, without reference to its conflict-of-law provisions.
Nothing in this Section shall prevent a party's ability to bring
cross-claims or counter-claims.

9. Miscellaneous
----------------

This License represents the complete agreement concerning the subject
matter hereof. If any provision of this License is held to be
unenforceable, such provision shall be reformed only to the extent
necessary to make it enforceable. Any law or regulation which provides
that the language of a contract shall be construed against the drafter
shall not be used to construe this License against a Contributor.

10. Versions of the License
---------------------------

10.1. New Versions

Mozilla Foundation is the license steward. Except as provided in Section
10.3, no one other than the license steward has the right to modify or
publish new versions of this License. Each version will be given a
distinguishing version number.

10.2. Effect of New Versions

You may distribute the Covered Software under the terms of the version
of the License under which You originally received the Covered Software,
or under the terms of any subsequent version published by the license
steward.

10.3. Modified Versions

If you create software not governed by this License, and you want to
create a new license for such software, you may create and use a
modified version of this License if you rename the license and remove
any references to the name of the license steward (except to note that
such modified license differs from this License).

10.4. Distributing Source Code Form that is Incompatible With Secondary
Licenses

If You choose to distribute Source Code Form that is Incompatible With
Secondary Licenses under the terms of this version of the License, the
notice described in Exhibit B of this License must be attached.

Exhibit A - Source Code Form License Notice
-------------------------------------------

  This Source Code Form is subject to the terms of the Mozilla Public
  License, v. 2.0. If a copy of the MPL was not distributed with this
  file, You can obtain one at http://mozilla.org/MPL/2.0/.

If it is not possible or desirable to put the notice in a particular
file, then You may include the notice in a location (such as a LICENSE
file in a relevant directory) where a recipient would be likely to look
for such a notice.

You may add additional accurate notices of copyright ownership.

Exhibit B - "Incompatible With Secondary Licenses" Notice
---------------------------------------------------------

  This Source Code Form is "Incompatible With Secondary Licenses", as
  defined by the Mozilla Public License, v. 2.0.
```

### Unicode-3.0

> 摘自 icu_collections-2.3.0 发布包

```text
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 2020-2024 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.

SPDX-License-Identifier: Unicode-3.0

—

Portions of ICU4X may have been adapted from ICU4C and/or ICU4J.
ICU 1.8.1 to ICU 57.1 © 1995-2016 International Business Machines Corporation and others.
```

### CC0-1.0

> 摘自 dunce-1.0.5 发布包

```text
Creative Commons Legal Code

CC0 1.0 Universal

    CREATIVE COMMONS CORPORATION IS NOT A LAW FIRM AND DOES NOT PROVIDE
    LEGAL SERVICES. DISTRIBUTION OF THIS DOCUMENT DOES NOT CREATE AN
    ATTORNEY-CLIENT RELATIONSHIP. CREATIVE COMMONS PROVIDES THIS
    INFORMATION ON AN "AS-IS" BASIS. CREATIVE COMMONS MAKES NO WARRANTIES
    REGARDING THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS
    PROVIDED HEREUNDER, AND DISCLAIMS LIABILITY FOR DAMAGES RESULTING FROM
    THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS PROVIDED
    HEREUNDER.

Statement of Purpose

The laws of most jurisdictions throughout the world automatically confer
exclusive Copyright and Related Rights (defined below) upon the creator
and subsequent owner(s) (each and all, an "owner") of an original work of
authorship and/or a database (each, a "Work").

Certain owners wish to permanently relinquish those rights to a Work for
the purpose of contributing to a commons of creative, cultural and
scientific works ("Commons") that the public can reliably and without fear
of later claims of infringement build upon, modify, incorporate in other
works, reuse and redistribute as freely as possible in any form whatsoever
and for any purposes, including without limitation commercial purposes.
These owners may contribute to the Commons to promote the ideal of a free
culture and the further production of creative, cultural and scientific
works, or to gain reputation or greater distribution for their Work in
part through the use and efforts of others.

For these and/or other purposes and motivations, and without any
expectation of additional consideration or compensation, the person
associating CC0 with a Work (the "Affirmer"), to the extent that he or she
is an owner of Copyright and Related Rights in the Work, voluntarily
elects to apply CC0 to the Work and publicly distribute the Work under its
terms, with knowledge of his or her Copyright and Related Rights in the
Work and the meaning and intended legal effect of CC0 on those rights.

1. Copyright and Related Rights. A Work made available under CC0 may be
protected by copyright and related or neighboring rights ("Copyright and
Related Rights"). Copyright and Related Rights include, but are not
limited to, the following:

  i. the right to reproduce, adapt, distribute, perform, display,
     communicate, and translate a Work;
 ii. moral rights retained by the original author(s) and/or performer(s);
iii. publicity and privacy rights pertaining to a person's image or
     likeness depicted in a Work;
 iv. rights protecting against unfair competition in regards to a Work,
     subject to the limitations in paragraph 4(a), below;
  v. rights protecting the extraction, dissemination, use and reuse of data
     in a Work;
 vi. database rights (such as those arising under Directive 96/9/EC of the
     European Parliament and of the Council of 11 March 1996 on the legal
     protection of databases, and under any national implementation
     thereof, including any amended or successor version of such
     directive); and
vii. other similar, equivalent or corresponding rights throughout the
     world based on applicable law or treaty, and any national
     implementations thereof.

2. Waiver. To the greatest extent permitted by, but not in contravention
of, applicable law, Affirmer hereby overtly, fully, permanently,
irrevocably and unconditionally waives, abandons, and surrenders all of
Affirmer's Copyright and Related Rights and associated claims and causes
of action, whether now known or unknown (including existing as well as
future claims and causes of action), in the Work (i) in all territories
worldwide, (ii) for the maximum duration provided by applicable law or
treaty (including future time extensions), (iii) in any current or future
medium and for any number of copies, and (iv) for any purpose whatsoever,
including without limitation commercial, advertising or promotional
purposes (the "Waiver"). Affirmer makes the Waiver for the benefit of each
member of the public at large and to the detriment of Affirmer's heirs and
successors, fully intending that such Waiver shall not be subject to
revocation, rescission, cancellation, termination, or any other legal or
equitable action to disrupt the quiet enjoyment of the Work by the public
as contemplated by Affirmer's express Statement of Purpose.

3. Public License Fallback. Should any part of the Waiver for any reason
be judged legally invalid or ineffective under applicable law, then the
Waiver shall be preserved to the maximum extent permitted taking into
account Affirmer's express Statement of Purpose. In addition, to the
extent the Waiver is so judged Affirmer hereby grants to each affected
person a royalty-free, non transferable, non sublicensable, non exclusive,
irrevocable and unconditional license to exercise Affirmer's Copyright and
Related Rights in the Work (i) in all territories worldwide, (ii) for the
maximum duration provided by applicable law or treaty (including future
time extensions), (iii) in any current or future medium and for any number
of copies, and (iv) for any purpose whatsoever, including without
limitation commercial, advertising or promotional purposes (the
"License"). The License shall be deemed effective as of the date CC0 was
applied by Affirmer to the Work. Should any part of the License for any
reason be judged legally invalid or ineffective under applicable law, such
partial invalidity or ineffectiveness shall not invalidate the remainder
of the License, and in such case Affirmer hereby affirms that he or she
will not (i) exercise any of his or her remaining Copyright and Related
Rights in the Work or (ii) assert any associated claims and causes of
action with respect to the Work, in either case contrary to Affirmer's
express Statement of Purpose.

4. Limitations and Disclaimers.

 a. No trademark or patent rights held by Affirmer are waived, abandoned,
    surrendered, licensed or otherwise affected by this document.
 b. Affirmer offers the Work as-is and makes no representations or
    warranties of any kind concerning the Work, express, implied,
    statutory or otherwise, including without limitation warranties of
    title, merchantability, fitness for a particular purpose, non
    infringement, or the absence of latent or other defects, accuracy, or
    the present or absence of errors, whether or not discoverable, all to
    the greatest extent permissible under applicable law.
 c. Affirmer disclaims responsibility for clearing rights of other persons
    that may apply to the Work or any use thereof, including without
    limitation any person's Copyright and Related Rights in the Work.
    Further, Affirmer disclaims responsibility for obtaining any necessary
    consents, permissions or other rights required for any use of the
    Work.
 d. Affirmer understands and acknowledges that Creative Commons is not a
    party to this document and has no duty or obligation with respect to
    this CC0 or use of the Work.
```

### Unlicense

> 摘自 aho-corasick-1.1.5 发布包

```text
This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or
distribute this software, either in source code form or as a compiled
binary, for any purpose, commercial or non-commercial, and by any
means.

In jurisdictions that recognize copyright laws, the author or authors
of this software dedicate any and all copyright interest in the
software to the public domain. We make this dedication for the benefit
of the public at large and to the detriment of our heirs and
successors. We intend this dedication to be an overt act of
relinquishment in perpetuity of all present and future rights to this
software under copyright law.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
OTHER DEALINGS IN THE SOFTWARE.

For more information, please refer to <http://unlicense.org/>
```

### LLVM-exception

> 摘自 target-lexicon-0.12.16 发布包

```text

                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.


--- LLVM Exceptions to the Apache 2.0 License ----

As an exception, if, as a result of your compiling your source code, portions
of this Software are embedded into an Object form of such source code, you
may redistribute such embedded portions in such Object form without complying
with the conditions of Sections 4(a), 4(b) and 4(d) of the License.

In addition, if you combine or link compiled forms of this Software with
software that is licensed under the GPLv2 ("Combined Software") and if a
court of competent jurisdiction determines that the patent provision (Section
3), the indemnity provision (Section 9) or other Section of the License
conflicts with the conditions of the GPLv2, you may retroactively and
prospectively choose to deem waived or otherwise exclude such Section(s) of
the License, but only in their entirety and only with respect to the Combined
Software.
```

### MIT-0

全文见:[MIT-0 官方文本](https://spdx.org/licenses/MIT-0.html)。

### LGPL-2.1-or-later

全文见:[LGPL-2.1-or-later 官方文本](https://spdx.org/licenses/LGPL-2.1-or-later.html)。
