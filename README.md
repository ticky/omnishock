# 🎮 Omnishock

[![Travis CI Build Status](https://travis-ci.org/ticky/omnishock.svg?branch=develop)](https://travis-ci.org/ticky/omnishock) [![Appveyor Build status](https://ci.appveyor.com/api/projects/status/9m0lyp0wy8djud7t/branch/develop?svg=true)](https://ci.appveyor.com/project/ticky/omnishock/branch/develop)

Something to do with game controllers!

## Prerequisites

- Rust
- SDL2 v2.0.6 or later
- Controller emulator hardware (see below)

## Supported Hardware

Omnishock currently supports communicating with a [Teensy 2.0](https://www.pjrc.com/store/teensy.html), running either:

- **Johnny Chung Lee's [Teensy PS2 Controller Sim Firmware](https://procrastineering.blogspot.com/2010/12/simulated-ps2-controller-for.html)**  
  Fast & simple. Omnishock has been tested with v2. Source is public but unlicensed.
- **Aaron Clovsky's [`teensy-firmware` for PS2 Bluetooth Adapter](http://psx-scene.com/forums/f19/how-build-your-own-ps2-bluetooth-adapter-use-real-ps3-ps4-controllers-wirelessly-your-ps2-127728/)**  
  Supports analog button inputs and force feedback. Source available under GPL2 or later.

Support for more hardware, and more firmware variants, is planned for the future.

## Building

- `git clone --recurse-submodules https://github.com/ticky/omnishock.git`
- `cargo build`
