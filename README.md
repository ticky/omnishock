# 🎮 Omnishock

[![Travis CI Build Status](https://travis-ci.org/ticky/omnishock.svg?branch=develop)](https://travis-ci.org/ticky/omnishock) [![Appveyor Build status](https://ci.appveyor.com/api/projects/status/9m0lyp0wy8djud7t/branch/develop?svg=true)](https://ci.appveyor.com/project/ticky/omnishock/branch/develop)

Something to do with game controllers!

## Supported Hardware

Omnishock currently supports communicating with a [Teensy 2.0](https://www.pjrc.com/store/teensy.html), running either:

- **Aaron Clovsky's [`teensy-firmware` for PS2 Bluetooth Adapter](http://psx-scene.com/forums/f19/how-build-your-own-ps2-bluetooth-adapter-use-real-ps3-ps4-controllers-wirelessly-your-ps2-127728/)**  
  Supports analog button inputs and force feedback. Source available under GPL2 or later.
- **Johnny Chung Lee's [Teensy PS2 Controller Sim Firmware](https://procrastineering.blogspot.com/2010/12/simulated-ps2-controller-for.html)**  
  Fast & simple. Omnishock has been tested with v2. Source is public but unlicensed.

Support for more hardware, and more firmware variants, is planned for the future.

## Prerequisites

- [Rust](https://www.rust-lang.org/install.html)
- SDL2 (v2.0.6 or later)
- Controller emulator hardware (see above)

### Mac-specific

SDL2 has broad support for many types of USB and Bluetooth gamepads on macOS, however, for Xbox 360 controllers, and for better support for Xbox One controllers, you will likely want [the 360Controller driver](https://github.com/360Controller/360Controller).

### Linux-specific

The version of sdl2 currently in the Debian package library is quite old (it's version 2.0.5 as of writing), so if you have trouble using certain gamepads (like the Xbox Wireless Controller, for instance), you will need to [build sdl from source](https://wiki.libsdl.org/Installation#Linux.2FUnix).

You'll likely need either permissive `udev` rules for your USB gamepads, or to make sure your user is in the `input` group. You can add your user account to the `input` group with the command `sudo usermod --append --groups input $(whoami)`.

For more information specific to setting up gamepads on Linux, I recommend checking out [this article on the Arch Wiki](https://wiki.archlinux.org/index.php/Gamepad).

## Building

- `git clone --recurse-submodules https://github.com/ticky/omnishock.git omnishock && cd omnishock`
- `cargo build --release`

## Running

`cargo run --release`
