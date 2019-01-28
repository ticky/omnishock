/*
 * Omnishock: Something to do with game controllers!
 * Copyright (C) 2017-2019 Jessica Stokes
 *
 * This file is part of Omnishock.
 *
 * Omnishock is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * Omnishock is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with Omnishock.  If not, see <https://www.gnu.org/licenses/>.
 */

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate clap;
extern crate game_time;
extern crate hex_view;
use hex_view::HexView;
extern crate num;
extern crate sdl2;
extern crate serialport;
extern crate spin_sleep;
use std::cmp::{PartialEq, PartialOrd};
use std::convert::From;
use std::io::prelude::{Read, Write};
use std::ops::{Add, Div, Neg};

#[cfg(feature = "flamegraph-profiling")]
extern crate flame;
#[cfg(feature = "flamegraph-profiling")]
use std::fs::File;

mod sdl_manager;
use sdl_manager::GameController;
use sdl_manager::SDLManager;

// The DualShock protocol uses 0x5A in many places!
const DUALSHOCK_MAGIC: u8 = 0x5A;

// Johnny Chung Lee's firmware responds with "k" on success,
const SEVEN_BYTE_OK_RESPONSE: char = 'k';
// and "x" when it recieves input it doesn't recognise.
const SEVEN_BYTE_ERR_RESPONSE: char = 'x';

// Aaron Clovsky's firmware responds with vibration information
// which begins with the DUALSHOCK_MAGIC.
const TWENTY_BYTE_OK_HEADER: u8 = DUALSHOCK_MAGIC;

// Serial port name hint is different per-OS
#[cfg(target_os = "macos")]
const SERIAL_HINT: &str = "\n(Usually /dev/cu.usbmodem12341 for USB Serial on macOS.)";
#[cfg(all(unix, not(target_os = "macos")))]
const SERIAL_HINT: &str = "\n(Usually /dev/ttyUSB0 for USB Serial on Unix.)";
#[cfg(windows)]
const SERIAL_HINT: &str = "\n(Usually COM3 for USB Serial on Windows.)";

// How many times you need to multiply a u8 converted
// to u16 by to become a u16 of the same magnitude
const U8_TO_U16_MAGNITUDE: u16 = u16::max_value() / u8::max_value() as u16;

enum ControllerEmulatorPacketType {
    None,       // Fallback, just log messages
    SevenByte,  // For Johnny Chung Lee's firmware
    TwentyByte, // For Aaron Clovsky's firmware
}

bitflags! {
    struct Buttons1: u8 {
        const Left = 0b1000_0000;
        const Down = 0b0100_0000;
        const Right = 0b0010_0000;
        const Up = 0b0001_0000;
        const Start = 0b0000_1000;
        const R3 = 0b0000_0100;
        const L3 = 0b0000_0010;
        const Select = 0b0000_0001;
    }
}

bitflags! {
    struct Buttons2: u8 {
        const Square = 0b1000_0000;
        const Cross = 0b0100_0000;
        const Circle = 0b0010_0000;
        const Triangle = 0b0001_0000;
        const R1 = 0b0000_1000;
        const L1 = 0b0000_0100;
        const R2 = 0b0000_0010;
        const L2 = 0b0000_0001;
    }
}

fn main() -> Result<(), Box<std::error::Error>> {
    use clap::{AppSettings, Arg, SubCommand};
    #[cfg(feature = "flamegraph-profiling")]
    flame::start("Parse Arguments");

    let arguments = app_from_crate!()
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg(
            Arg::with_name("verbose")
                .long("verbose")
                .short("v")
                .help("Print more information about activity"),
        )
        .subcommand(
            SubCommand::with_name("ps2ce")
                .about(
                    "Start a transliteration session using a PS2 Controller Emulator over Serial",
                )
                .arg(
                    Arg::with_name("device")
                        .help(&format!("Device to use to communcate.{}", SERIAL_HINT))
                        .index(1)
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("trigger-mode")
                        .long("trigger-mode")
                        .short("t")
                        .help("How to map the analog triggers")
                        .takes_value(true)
                        .default_value("normal")
                        .possible_value("normal")
                        .possible_value("right-stick")
                        .possible_value("cross-and-square"),
                )
                .arg(
                    Arg::with_name("no-stick-normalise")
                        .long("no-stick-normalise")
                        .short("n")
                        .help("Disable stick normalisation")
                        .long_help(
                            "Disable stick normalisation. Normally, stick values \
                             are multiplied by 1.1, to simulate the prominent outer \
                             deadzone exhibited by real DualShock 2 controllers. \
                             This option removes this compensation. May be useful \
                             if you're using another older-style analog controller.",
                        ),
                ),
        )
        .subcommand(SubCommand::with_name("test").about("Tests the game controller subsystem"))
        .get_matches();

    #[cfg(feature = "flamegraph-profiling")]
    flame::end("Parse Arguments");

    let mut sdl_manager = SDLManager::init()?;

    println!(
        "(There are {} controllers connected)",
        sdl_manager.active_controllers.len()
    );

    match arguments.subcommand_name() {
        Some("ps2ce") => {
            send_to_ps2_controller_emulator(&arguments, &mut sdl_manager)?;
        }
        Some("test") => {
            print_events(&arguments, &mut sdl_manager)?;
        }
        _ => (),
    }

    #[cfg(feature = "flamegraph-profiling")]
    flame::dump_html(&mut File::create("flame-graph.html")?)?;
    #[cfg(feature = "flamegraph-profiling")]
    flame::dump_json(&mut File::create("flame-graph.json")?)?;

    Ok(())
}

fn whats_the_midpoint_of_a<T: num::Bounded + Add<Output = T> + Div<Output = T> + From<u8>>() -> T {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("whats_the_midpoint_of_a()");
    (T::max_value() + T::min_value()) / T::from(2)
}

fn convert_button_to_analog<T: num::Bounded>(button: bool) -> T {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("convert_button_to_analog()");
    if button {
        T::max_value()
    } else {
        T::min_value()
    }
}

fn convert_analog_to_button<
    T: num::Bounded + Add<Output = T> + Div<Output = T> + From<u8> + PartialOrd,
>(
    analog: T,
) -> bool {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("convert_analog_to_button()");

    analog > whats_the_midpoint_of_a::<T>()
}

fn convert_for_dualshock(number: i16) -> u8 {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("convert_for_dualshock()");
    (number.wrapping_shr(8) + 0x80) as u8
}

fn convert_half_axis_positive<
    T: num::Bounded + num::Saturating + Copy + Div<Output = T> + PartialEq + From<u8>,
>(
    stick: T,
) -> T {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("convert_half_axis_positive()");
    // Special case the maximum values, so we don't end up with
    if stick == T::max_value() {
        return T::max_value();
    }

    let two_in_target_type = T::from(2);
    let half_minimum = T::min_value().div(two_in_target_type);
    let normalised_stick = stick.saturating_add(half_minimum);

    // This is a weird way to multiply by two but it works eh
    normalised_stick.saturating_add(normalised_stick)
}

fn convert_half_axis_negative<
    T: num::Bounded
        + num::Saturating
        + Copy
        + Neg<Output = T>
        + Div<Output = T>
        + PartialEq
        + From<u8>,
>(
    stick: T,
) -> T {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("convert_half_axis_negative()");
    convert_half_axis_positive(stick.saturating_add(T::from(1)).neg())
}

fn normalise_stick_as_dualshock2(x: &mut i16, y: &mut i16) {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("normalise_stick_as_dualshock2()");
    // Adjust stick positions to match those of the DualShock®2.
    // The DualShock®2 has a prominent outer deadzone,
    // so we shrink the usable area here by 10%.
    *x = x.saturating_add(*x / 10);
    *y = y.saturating_add(*y / 10);
}

fn controller_map_seven_byte<T: GameController>(
    controller: &T,
    trigger_mode: &str,
    normalise_sticks: bool,
) -> Vec<u8> {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("controller_map_seven_byte()");
    // Seven byte controller map is the same as
    // the first seven bytes of the twenty-byte map!
    let mut map = controller_map_twenty_byte(controller, trigger_mode, normalise_sticks);
    map.truncate(7);
    map
}

fn controller_map_twenty_byte<T: GameController>(
    controller: &T,
    trigger_mode: &str,
    normalise_sticks: bool,
) -> Vec<u8> {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("controller_map_twenty_byte()");
    use sdl2::controller::{Axis, Button};

    #[cfg(feature = "flamegraph-profiling")]
    flame::start("buttons1");
    // buttons1
    let dpad_left_value: i16 = convert_button_to_analog(controller.button(Button::DPadLeft));
    let dpad_down_value: i16 = convert_button_to_analog(controller.button(Button::DPadDown));
    let dpad_right_value: i16 = convert_button_to_analog(controller.button(Button::DPadRight));
    let dpad_up_value: i16 = convert_button_to_analog(controller.button(Button::DPadUp));
    let start_value: i16 = convert_button_to_analog(controller.button(Button::Start));
    let right_stick_value: i16 = convert_button_to_analog(controller.button(Button::RightStick));
    let left_stick_value: i16 = convert_button_to_analog(controller.button(Button::LeftStick));
    let select_value: i16 = convert_button_to_analog(controller.button(Button::Back));
    #[cfg(feature = "flamegraph-profiling")]
    flame::end("buttons1");

    #[cfg(feature = "flamegraph-profiling")]
    flame::start("buttons2");
    // buttons2
    let mut square_value: i16 = convert_button_to_analog(controller.button(Button::X));
    let mut cross_value: i16 = convert_button_to_analog(controller.button(Button::A));
    let circle_value: i16 = convert_button_to_analog(controller.button(Button::B));
    let triangle_value: i16 = convert_button_to_analog(controller.button(Button::Y));
    let r1_button_value: i16 = convert_button_to_analog(controller.button(Button::RightShoulder));
    let l1_button_value: i16 = convert_button_to_analog(controller.button(Button::LeftShoulder));
    let mut r2_button_value: i16 = convert_half_axis_positive(controller.axis(Axis::TriggerRight));
    let mut l2_button_value: i16 = convert_half_axis_positive(controller.axis(Axis::TriggerLeft));
    #[cfg(feature = "flamegraph-profiling")]
    flame::end("buttons2");

    #[cfg(feature = "flamegraph-profiling")]
    flame::start("sticks");
    let mut right_stick_x_value: i16 = controller.axis(Axis::RightX);
    let mut right_stick_y_value: i16 = controller.axis(Axis::RightY);
    let mut left_stick_x_value: i16 = controller.axis(Axis::LeftX);
    let mut left_stick_y_value: i16 = controller.axis(Axis::LeftY);
    #[cfg(feature = "flamegraph-profiling")]
    flame::end("sticks");

    #[cfg(feature = "flamegraph-profiling")]
    flame::start("handle trigger_mode");
    match trigger_mode {
        "right-stick" => {
            l2_button_value = convert_half_axis_negative(controller.axis(Axis::RightY));
            r2_button_value = convert_half_axis_positive(controller.axis(Axis::RightY));

            cross_value = convert_button_to_analog(controller.button(Button::A));
            square_value = convert_button_to_analog(controller.button(Button::X));

            // Combine the two raw trigger axes by subtracting one from the other
            // NOTE: This doesn't allow for both to be used at once
            right_stick_y_value =
                controller.axis(Axis::TriggerLeft) - controller.axis(Axis::TriggerRight);
        }
        "cross-and-square" => {
            l2_button_value = convert_button_to_analog(controller.button(Button::A));
            r2_button_value = convert_button_to_analog(controller.button(Button::X));

            cross_value = convert_half_axis_positive(controller.axis(Axis::TriggerRight));
            square_value = convert_half_axis_positive(controller.axis(Axis::TriggerLeft));
        }
        _ => (),
    }
    #[cfg(feature = "flamegraph-profiling")]
    flame::end("handle trigger_mode");

    if normalise_sticks {
        normalise_stick_as_dualshock2(&mut right_stick_x_value, &mut right_stick_y_value);
        normalise_stick_as_dualshock2(&mut left_stick_x_value, &mut left_stick_y_value);
    }

    let mut buttons1 = Buttons1::empty();
    buttons1.set(Buttons1::Left, convert_analog_to_button(dpad_left_value));
    buttons1.set(Buttons1::Down, convert_analog_to_button(dpad_down_value));
    buttons1.set(Buttons1::Right, convert_analog_to_button(dpad_right_value));
    buttons1.set(Buttons1::Up, convert_analog_to_button(dpad_up_value));
    buttons1.set(Buttons1::Start, convert_analog_to_button(start_value));
    buttons1.set(Buttons1::R3, convert_analog_to_button(right_stick_value));
    buttons1.set(Buttons1::L3, convert_analog_to_button(left_stick_value));
    buttons1.set(Buttons1::Select, convert_analog_to_button(select_value));

    let mut buttons2 = Buttons2::empty();
    buttons2.set(Buttons2::Square, convert_analog_to_button(square_value));
    buttons2.set(Buttons2::Cross, convert_analog_to_button(cross_value));
    buttons2.set(Buttons2::Circle, convert_analog_to_button(circle_value));
    buttons2.set(Buttons2::Triangle, convert_analog_to_button(triangle_value));
    buttons2.set(Buttons2::R1, convert_analog_to_button(r1_button_value));
    buttons2.set(Buttons2::L1, convert_analog_to_button(l1_button_value));
    buttons2.set(Buttons2::R2, convert_analog_to_button(r2_button_value));
    buttons2.set(Buttons2::L2, convert_analog_to_button(l2_button_value));

    let mode_footer = if controller.button(Button::Guide) {
        0xAA
    } else {
        0x55
    };

    return vec![
        DUALSHOCK_MAGIC,
        // DualShock protocol considers 0 to mean
        // pressed and 1 to mean not pressed, so
        // we NOT the our bitflags here
        !buttons1.bits(),
        !buttons2.bits(),
        // Analog sticks
        convert_for_dualshock(right_stick_x_value),
        convert_for_dualshock(right_stick_y_value),
        convert_for_dualshock(left_stick_x_value),
        convert_for_dualshock(left_stick_y_value),
        // Pressure values
        convert_for_dualshock(dpad_right_value),
        convert_for_dualshock(dpad_left_value),
        convert_for_dualshock(dpad_up_value),
        convert_for_dualshock(dpad_down_value),
        convert_for_dualshock(triangle_value),
        convert_for_dualshock(circle_value),
        convert_for_dualshock(cross_value),
        convert_for_dualshock(square_value),
        convert_for_dualshock(l1_button_value),
        convert_for_dualshock(r1_button_value),
        convert_for_dualshock(l2_button_value),
        convert_for_dualshock(r2_button_value),
        mode_footer,
    ];
}

fn clear_serial_buffer<T: Read>(serial: &mut T) {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("clear_serial_buffer()");
    // NOTE: This should only be used with a SerialPort, as it has weird
    //       behaviour around the read method which is not implied by the Read
    //       trait. I'm hoping to find a better way to deal with this in future.
    //       Possibly <https://doc.rust-lang.org/nightly/std/io/trait.Read.html#method.read_to_end>

    // Create a response buffer
    let mut response = vec![0; 1];

    while {
        match serial.read(&mut response) {
            Err(error) => {
                // "Operation timed out" means we've reached
                // the end of the buffer, which is what we want!
                if error.kind() != std::io::ErrorKind::TimedOut {
                    panic!("Error clearing serial buffer: {}", error);
                }

                false
            }
            _ => true,
        }
    } {}
}

fn send_to_ps2_controller_emulator(
    arguments: &clap::ArgMatches,
    sdl_manager: &mut SDLManager,
) -> Result<(), Box<std::error::Error>> {
    use serialport::prelude::*;
    use std::time::Duration;

    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("send_to_ps2_controller_emulator()");

    let verbose = arguments.is_present("verbose");
    let command_arguments = arguments.subcommand_matches("ps2ce").unwrap();
    let device_path = command_arguments.value_of("device").unwrap();

    if verbose {
        println!(
            "Connecting to PS2 Controller Emulator device at '{}'...",
            device_path
        );
    }

    let serial_settings = SerialPortSettings {
        baud_rate: 9600,
        data_bits: DataBits::Eight,
        flow_control: FlowControl::None,
        parity: Parity::None,
        stop_bits: StopBits::One,
        // This started out as 100ms, but that's HEAPS!
        // Let's bank on it being less than half our
        // target frame length (16ms/2) instead!
        timeout: Duration::from_millis(8),
    };

    let serial = match serialport::open_with_settings(device_path, &serial_settings) {
        Ok(serial) => serial,
        Err(error) => panic!("failed to open serial device: {}", error),
    };

    send_to_ps2_controller_emulator_via(arguments, sdl_manager, serial)
}

fn send_to_ps2_controller_emulator_via<I: Read + Write>(
    arguments: &clap::ArgMatches,
    sdl_manager: &mut SDLManager,
    mut serial: I,
) -> Result<(), Box<std::error::Error>> {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("send_to_ps2_controller_emulator_via()");
    let verbose = arguments.is_present("verbose");
    let command_arguments = arguments.subcommand_matches("ps2ce").unwrap();

    let mut communication_mode = ControllerEmulatorPacketType::None;

    // Create a four-byte response buffer
    let mut response = vec![0; 4];

    // The Teensy might be waiting to send bytes to a previous
    // control session, if things didn't go so well.
    // Let's make sure there's nothing left in that pipe!
    if verbose {
        println!("Clearing serial buffer...");
    }

    clear_serial_buffer(&mut serial);

    if verbose {
        println!("Determining device type...");
    }

    // Send a twenty-byte, packet of a neutral controller state.
    serial.write_all(&[
        DUALSHOCK_MAGIC,
        !Buttons1::empty().bits(),
        !Buttons2::empty().bits(),
        // Sticks
        0x80, // Right stick X
        0x80, // Right stick Y
        0x80, // Left stick X
        0x80, // Left stick Y
        // Pressure
        0x00, // Right
        0x00, // Left
        0x00, // Up
        0x00, // Down
        0x00, // Triangle
        0x00, // Circle
        0x00, // Cross
        0x00, // Square
        0x00, // [L1]
        0x00, // [R1]
        0x00, // [L2]
        0x00, // [R2]
        // Mode
        0x55, // Normal
    ])?;

    // Check the response!
    match serial.read(&mut response) {
        Ok(_) => {
            if response[0] == TWENTY_BYTE_OK_HEADER {
                if verbose {
                    println!(
                        "Response began with '{}': this is probably Aaron Clovsky's work!",
                        TWENTY_BYTE_OK_HEADER
                    );
                }

                communication_mode = ControllerEmulatorPacketType::TwentyByte;
            } else if response[0] == (SEVEN_BYTE_ERR_RESPONSE as u8) {
                if verbose {
                    println!(
                        "Response began with '{}': this is probably Johnny Chung Lee's work!",
                        SEVEN_BYTE_ERR_RESPONSE
                    );
                }

                communication_mode = ControllerEmulatorPacketType::SevenByte;
            } else {
                println!("Unrecognised response: {:x}", HexView::from(&response));
            }
        }
        Err(error) => {
            println!("failed reading from device: {}", error);
        }
    };

    // Clear the buffer again!
    if verbose {
        println!("Clearing serial buffer...");
    }

    clear_serial_buffer(&mut serial);

    let trigger_mode = command_arguments.value_of("trigger-mode").unwrap();

    if verbose {
        println!("Using trigger mode '{}'...", trigger_mode);
    }

    let normalise_sticks = !command_arguments.is_present("no-stick-normalise");

    if verbose {
        if normalise_sticks {
            println!("Normalising stick extents (stick values * 1.1)")
        } else {
            println!("Not normalising stick extents")
        }
    }

    let mut event_pump = sdl_manager.context.event_pump()?;

    // We use `game_time` to keep track of "frame" time and try to hit a
    // consistent rate at all times. We use `spin_sleep` instead of
    // `thread::Sleep` to get more accurate sleep times on all platforms.
    use game_time::framerate::RunningAverageSampler;
    use game_time::{FloatDuration, FrameCount, FrameCounter, GameClock};

    let mut clock = GameClock::new();
    let mut counter = FrameCounter::new(60.0, RunningAverageSampler::with_max_samples(60));
    let mut sim_time;
    let warning_threshold = FloatDuration::milliseconds(500.0);

    // `spin_sleeper` gives us a more accurate sleep timer.
    // With it we will trust `thread::Sleep` for all but the last 1ms
    // (1,000,000ns) of the sleep timer, then it will spin for the remainder.
    // With this in place we only dip below 95% of our speed target a handful
    // of times in a 4-minute period, rather than nearly every iteration.
    let spin_sleeper = spin_sleep::SpinSleeper::new(1_000_000);

    'outer: loop {
        #[cfg(feature = "flamegraph-profiling")]
        let _outer_guard = flame::start_guard("frame");
        // Tick the "frame" timer and counters forward
        sim_time = clock.tick(&game_time::step::FixedStep::new(&counter));
        counter.tick(&sim_time);

        if verbose {
            // If we're `--verbose`, we print out stats for every iteration
            println!(
                "Frame @ {:.2} ({:.2}ms, {:}fps avg / {:.2}fps target, slow: {})",
                sim_time.total_wall_time(),
                sim_time.elapsed_wall_time().as_milliseconds(),
                counter.average_frame_rate(),
                sim_time.instantaneous_frame_rate(),
                counter.is_running_slow(&sim_time),
            );
        } else if counter.is_running_slow(&sim_time)
            && sim_time.total_wall_time() > warning_threshold
        {
            // If we're not `--verbose`, and in a debug build, we print out
            // stats only on slow iterations
            #[cfg(debug_assertions)]
            println!(
                "Warning: slow frame @ {:.2} ({:.2}ms, {:.2}fps avg / {:}fps target)",
                sim_time.total_wall_time(),
                sim_time.elapsed_wall_time().as_milliseconds(),
                counter.average_frame_rate(),
                sim_time.instantaneous_frame_rate(),
            );
        }

        // Now that we've said we're restarting the frame,
        // let's iterate over controller events we've got from SDL2
        for event in event_pump.poll_iter() {
            use sdl2::event::Event;

            match event {
                Event::ControllerDeviceAdded { which, .. } => {
                    #[cfg(feature = "flamegraph-profiling")]
                    let _guard = flame::start_guard("Event::ControllerDeviceAdded");
                    if !sdl_manager.has_controller(which).ok().unwrap_or(true) {
                        match sdl_manager.add_controller(which) {
                            Ok(_) => {
                                println!(
                                    "(There are {} controllers connected)",
                                    sdl_manager.active_controllers.len()
                                );
                            }
                            Err(error) => println!(
                                "could not initialise connected joystick {}: {:?}",
                                which, error
                            ),
                        };
                    }
                }

                Event::ControllerDeviceRemoved { which, .. } => {
                    #[cfg(feature = "flamegraph-profiling")]
                    let _guard = flame::start_guard("Event::ControllerDeviceRemoved");
                    if sdl_manager.remove_controller(which).is_some() {
                        println!(
                            "(There are {} controllers connected)",
                            sdl_manager.active_controllers.len()
                        );
                    };
                }

                Event::Quit { .. } => break 'outer,
                _ => (),
            }
        }

        // Now that we've kept track of controller additions & removals,
        // post an update for the one controller we currently care about.
        if let Some(controller) = sdl_manager.active_controllers.get_mut(&0) {
            let response = send_event_to_controller(
                &mut serial,
                controller,
                &communication_mode,
                trigger_mode,
                normalise_sticks,
                verbose,
            )?;

            // If we've receieved a response from the controller,
            // try updating its haptic state
            if !response.is_empty() {
                let small_motor_intensity = u16::from(response[1]) * U8_TO_U16_MAGNITUDE;
                let large_motor_intensity = u16::from(response[2]) * U8_TO_U16_MAGNITUDE;

                if verbose {
                    println!(
                        "“{}”: Setting rumble to ({},{})",
                        controller.name(),
                        small_motor_intensity,
                        large_motor_intensity
                    );
                }

                // We don't care if `set_rumble` actually worked,
                // because if it's unsupported, it won't break anything,
                // so we just ignore the result entirely here.
                #[allow(unused_must_use)]
                {
                    controller.set_rumble(small_motor_intensity, large_motor_intensity, 500);
                }
            }
        }

        {
            #[cfg(feature = "flamegraph-profiling")]
            let _sleep_guard = flame::start_guard("post-frame sleep");
            // Having run all our processing for this iteration, accurately sleep
            // until we need to process the next one
            clock.sleep_remaining_via(&counter, |rem| spin_sleeper.sleep(rem.to_std().unwrap()));
        };
    }

    Ok(())
}

fn send_event_to_controller<I: Read + Write, T: GameController>(
    serial: &mut I,
    controller: &T,
    communication_mode: &ControllerEmulatorPacketType,
    trigger_mode: &str,
    normalise_sticks: bool,
    verbose: bool,
) -> Result<Vec<u8>, Box<std::error::Error>> {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("send_event_to_controller()");
    let sent;
    let mut bytes_received = 0;
    let mut received = vec![0; 4];

    match *communication_mode {
        ControllerEmulatorPacketType::None => {
            #[cfg(feature = "flamegraph-profiling")]
            let _guard = flame::start_guard("ControllerEmulatorPacketType::None");
            sent = controller_map_twenty_byte(controller, trigger_mode, normalise_sticks);
        }

        ControllerEmulatorPacketType::SevenByte => {
            #[cfg(feature = "flamegraph-profiling")]
            let _guard = flame::start_guard("ControllerEmulatorPacketType::SevenByte");
            let state = controller_map_seven_byte(controller, trigger_mode, normalise_sticks);

            {
                #[cfg(feature = "flamegraph-profiling")]
                let _guard = flame::start_guard("serial write");
                serial.write_all(&state)?;
            };
            bytes_received = {
                #[cfg(feature = "flamegraph-profiling")]
                let _guard = flame::start_guard("serial read");
                match serial.read(&mut received) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        if verbose {
                            println!("Error reading response: {}", error);
                        }
                        0
                    }
                }
            };

            if received[0] != (SEVEN_BYTE_OK_RESPONSE as u8) {
                println!("WARNING: Adapter responded with an error status.")
            }

            sent = state;
        }

        ControllerEmulatorPacketType::TwentyByte => {
            #[cfg(feature = "flamegraph-profiling")]
            let _guard = flame::start_guard("ControllerEmulatorPacketType::TwentyByte");
            let state = controller_map_twenty_byte(controller, trigger_mode, normalise_sticks);

            {
                #[cfg(feature = "flamegraph-profiling")]
                let _guard = flame::start_guard("serial write");
                serial.write_all(&state)?;
            };
            bytes_received = {
                #[cfg(feature = "flamegraph-profiling")]
                let _guard = flame::start_guard("serial read");
                match serial.read(&mut received) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        if verbose {
                            println!("Error reading response: {}", error);
                        }
                        0
                    }
                }
            };

            sent = state;
        }
    };

    received.truncate(bytes_received);

    if verbose {
        println!("Sent: {:x}", HexView::from(&sent));

        if bytes_received > 0 {
            println!("Received: {:x}", HexView::from(&received));
        }
    }

    Ok(received)
}

fn print_events(
    _arguments: &clap::ArgMatches,
    sdl_manager: &mut SDLManager,
) -> Result<(), Box<std::error::Error>> {
    #[cfg(feature = "flamegraph-profiling")]
    let _guard = flame::start_guard("print_events()");
    println!("Printing all controller events...");

    for event in sdl_manager.context.event_pump()?.wait_iter() {
        use sdl2::event::Event;

        match event {
            Event::ControllerDeviceAdded { which, .. } => {
                #[cfg(feature = "flamegraph-profiling")]
                let _guard = flame::start_guard("Event::ControllerDeviceAdded");
                if !sdl_manager.has_controller(which).ok().unwrap_or(true) {
                    match sdl_manager.add_controller(which) {
                        Ok(_) => {
                            println!(
                                "(There are {} controllers connected)",
                                sdl_manager.active_controllers.len()
                            );
                        }
                        Err(error) => println!(
                            "could not initialise connected joystick {}: {:?}",
                            which, error
                        ),
                    };
                }
            }

            Event::ControllerDeviceRemoved { which, .. } => {
                #[cfg(feature = "flamegraph-profiling")]
                let _guard = flame::start_guard("Event::ControllerDeviceRemoved");
                if sdl_manager.remove_controller(which).is_some() {
                    println!(
                        "(There are {} controllers connected)",
                        sdl_manager.active_controllers.len()
                    );
                };
            }

            Event::ControllerAxisMotion {
                which, axis, value, ..
            } => {
                #[cfg(feature = "flamegraph-profiling")]
                let _guard = flame::start_guard("Event::ControllerAxisMotion");
                println!(
                    "“{}” (#{}): {:?}: {}",
                    sdl_manager.active_controllers[&which].name(),
                    which,
                    axis,
                    value
                );

                if let Some(controller) = sdl_manager.active_controllers.get_mut(&which) {
                    #[cfg(feature = "flamegraph-profiling")]
                    let _guard = flame::start_guard("set rumble");

                    println!("“{}”: Rumbling", controller.name());

                    // We don't care if `set_rumble` actually worked,
                    // because if it's unsupported, it won't break anything,
                    // so we just ignore the result entirely here.
                    #[allow(unused_must_use)]
                    {
                        controller.set_rumble(0xFFFF, 0xFFFF, 500);
                    }
                };
            }

            Event::ControllerButtonDown { which, button, .. } => {
                #[cfg(feature = "flamegraph-profiling")]
                let _guard = flame::start_guard("Event::ControllerButtonDown");
                println!(
                    "“{}” (#{}): {:?}: down",
                    sdl_manager.active_controllers[&which].name(),
                    which,
                    button
                );
            }

            Event::ControllerButtonUp { which, button, .. } => {
                #[cfg(feature = "flamegraph-profiling")]
                let _guard = flame::start_guard("Event::ControllerButtonUp");
                println!(
                    "“{}” (#{}): {:?}: up",
                    sdl_manager.active_controllers[&which].name(),
                    which,
                    button
                );
            }

            Event::Quit { .. } => break,
            _ => (),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate mockstream;

    #[test]
    fn convert_half_axis_positive_is_accurate() {
        use super::convert_half_axis_positive;

        assert_eq!(
            convert_half_axis_positive(i16::min_value()),
            i16::min_value()
        );
        assert_eq!(convert_half_axis_positive(0), i16::min_value());
        assert_eq!(convert_half_axis_positive(i16::max_value() / 2 + 1), 0);
        assert_eq!(
            convert_half_axis_positive(i16::max_value()),
            i16::max_value()
        );
    }

    #[test]
    fn convert_half_axis_negative_is_accurate() {
        use super::convert_half_axis_negative;

        assert_eq!(
            convert_half_axis_negative(i16::max_value()),
            i16::min_value()
        );
        assert_eq!(convert_half_axis_negative(0), i16::min_value());
        assert_eq!(convert_half_axis_negative(i16::min_value() / 2 - 1), 0);
        assert_eq!(
            convert_half_axis_negative(i16::min_value()),
            i16::max_value()
        );
    }

    #[test]
    fn whats_the_midpoint_of_a_is_accurate() {
        #![allow(clippy::float_cmp)]
        use super::whats_the_midpoint_of_a;

        assert_eq!(whats_the_midpoint_of_a::<u8>(), 127_u8);
        assert_eq!(
            whats_the_midpoint_of_a::<u64>(),
            9_223_372_036_854_775_807_u64
        );
        assert_eq!(whats_the_midpoint_of_a::<i16>(), 0_i16);
        assert_eq!(whats_the_midpoint_of_a::<i64>(), 0_i64);
        // NOTE: This would normally trip the `clippy::float_cmp` rule,
        //       which is why we've explicitly allowed it above.
        assert_eq!(whats_the_midpoint_of_a::<f32>(), 0_f32);
    }

    #[test]
    fn convert_button_to_analog_is_accurate() {
        use super::convert_button_to_analog;

        assert_eq!(convert_button_to_analog::<u8>(true), u8::max_value());
        assert_eq!(convert_button_to_analog::<i64>(true), i64::max_value());
        assert_eq!(convert_button_to_analog::<u8>(false), u8::min_value());
        assert_eq!(convert_button_to_analog::<i64>(false), i64::min_value());
    }

    #[test]
    fn convert_analog_to_button_is_accurate() {
        use super::convert_analog_to_button;

        assert_eq!(convert_analog_to_button(127u8), false);
        assert_eq!(convert_analog_to_button(128u8), true);

        assert_eq!(convert_analog_to_button(u8::max_value()), true);
        assert_eq!(convert_analog_to_button(i64::max_value()), true);
        assert_eq!(convert_analog_to_button(u8::min_value()), false);
        assert_eq!(convert_analog_to_button(i64::min_value()), false);
    }

    use sdl2;
    use sdl_manager::GameController;
    use std::collections::HashMap;

    struct FauxController {
        name: String,
        buttons: HashMap<sdl2::controller::Button, bool>,
        axes: HashMap<sdl2::controller::Axis, i16>,
    }

    impl FauxController {
        fn create_with_name(name: String) -> FauxController {
            let buttons = HashMap::new();
            let axes = HashMap::new();
            FauxController {
                name,
                buttons,
                axes,
            }
        }

        fn set_button(&mut self, button: sdl2::controller::Button, value: bool) {
            self.buttons.insert(button, value);
        }

        fn set_axis(&mut self, axis: sdl2::controller::Axis, value: i16) {
            self.axes.insert(axis, value);
        }
    }

    impl GameController for FauxController {
        fn name(&self) -> String {
            self.name.clone()
        }

        fn button(&self, button: sdl2::controller::Button) -> bool {
            *self.buttons.get(&button).unwrap_or(&false)
        }

        fn axis(&self, axis: sdl2::controller::Axis) -> i16 {
            *self.axes.get(&axis).unwrap_or(&0)
        }

        fn set_rumble(
            &mut self,
            _low_frequency_rumble: u16,
            _high_frequency_rumble: u16,
            _duration_ms: u32,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn controller_map_twenty_byte_works() {
        use super::controller_map_twenty_byte;
        use super::{Buttons1, Buttons2};
        use sdl2::controller::{Axis, Button};
        use DUALSHOCK_MAGIC;

        let mut controller =
            FauxController::create_with_name(String::from("Applejack Game-player Pad"));

        assert_eq!(
            controller_map_twenty_byte(&controller, "normal", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::empty().bits(),
                !Buttons2::empty().bits(),
                // Analog sticks
                0x80,
                0x80,
                0x80,
                0x80,
                // Pressure values
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                // Mode footer
                0x55,
            ]
        );

        assert_eq!(
            controller_map_twenty_byte(&controller, "right-stick", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::empty().bits(),
                !Buttons2::empty().bits(),
                // Analog sticks
                0x80,
                0x80,
                0x80,
                0x80,
                // Pressure values
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                // Mode footer
                0x55,
            ]
        );

        assert_eq!(
            controller_map_twenty_byte(&controller, "cross-and-square", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::empty().bits(),
                !Buttons2::empty().bits(),
                // Analog sticks
                0x80,
                0x80,
                0x80,
                0x80,
                // Pressure values
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                // Mode footer
                0x55,
            ]
        );

        // Do some stuff to the controller state, and test again
        controller.set_button(Button::DPadLeft, true);
        controller.set_button(Button::A, true);
        controller.set_axis(Axis::TriggerLeft, i16::max_value());
        controller.set_axis(Axis::RightX, -24_000);
        controller.set_axis(Axis::RightY, 16_500);
        controller.set_axis(Axis::LeftX, 255);
        controller.set_axis(Axis::LeftY, -4_096);

        assert_eq!(
            controller_map_twenty_byte(&controller, "normal", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::Left.bits(),
                !(Buttons2::Cross | Buttons2::L2).bits(),
                // Analog sticks
                0x18,
                0xC6,
                0x81,
                0x6E,
                // Pressure values
                0x00,
                0xFF,
                0x00,
                0x00,
                0x00,
                0x00,
                0xFF,
                0x00,
                0x00,
                0x00,
                0xFF,
                0x00,
                // Mode footer
                0x55,
            ]
        );

        assert_eq!(
            controller_map_twenty_byte(&controller, "right-stick", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::Left.bits(),
                !(Buttons2::Cross | Buttons2::R2).bits(),
                // Analog sticks
                0x18,
                0xFF,
                0x81,
                0x6E,
                // Pressure values
                0x00,
                0xFF,
                0x00,
                0x00,
                0x00,
                0x00,
                0xFF,
                0x00,
                0x00,
                0x00,
                0x00,
                0x80,
                // Mode footer
                0x55,
            ]
        );

        assert_eq!(
            controller_map_twenty_byte(&controller, "cross-and-square", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::Left.bits(),
                !(Buttons2::Square | Buttons2::L2).bits(),
                // Analog sticks
                0x18,
                0xC6,
                0x81,
                0x6E,
                // Pressure values
                0x00,
                0xFF,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0xFF,
                0x00,
                0x00,
                0xFF,
                0x00,
                // Mode footer
                0x55,
            ]
        );
    }

    #[test]
    fn controller_map_seven_byte_works() {
        use super::controller_map_seven_byte;
        use super::{Buttons1, Buttons2};
        use sdl2::controller::{Axis, Button};
        use DUALSHOCK_MAGIC;

        let mut controller =
            FauxController::create_with_name(String::from("Apple Pippin Controller"));

        assert_eq!(
            controller_map_seven_byte(&controller, "normal", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::empty().bits(),
                !Buttons2::empty().bits(),
                // Analog sticks
                0x80,
                0x80,
                0x80,
                0x80,
            ]
        );

        assert_eq!(
            controller_map_seven_byte(&controller, "right-stick", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::empty().bits(),
                !Buttons2::empty().bits(),
                // Analog sticks
                0x80,
                0x80,
                0x80,
                0x80,
            ]
        );

        assert_eq!(
            controller_map_seven_byte(&controller, "cross-and-square", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::empty().bits(),
                !Buttons2::empty().bits(),
                // Analog sticks
                0x80,
                0x80,
                0x80,
                0x80,
            ]
        );

        // Do some stuff to the controller state, and test again
        controller.set_button(Button::DPadLeft, true);
        controller.set_button(Button::A, true);
        controller.set_axis(Axis::TriggerLeft, i16::max_value());
        controller.set_axis(Axis::RightX, -24_000);
        controller.set_axis(Axis::RightY, 16_500);
        controller.set_axis(Axis::LeftX, 255);
        controller.set_axis(Axis::LeftY, -4_096);

        assert_eq!(
            controller_map_seven_byte(&controller, "normal", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::Left.bits(),
                !(Buttons2::Cross | Buttons2::L2).bits(),
                // Analog sticks
                0x18,
                0xC6,
                0x81,
                0x6E,
            ]
        );

        assert_eq!(
            controller_map_seven_byte(&controller, "right-stick", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::Left.bits(),
                !(Buttons2::Cross | Buttons2::R2).bits(),
                // Analog sticks
                0x18,
                0xFF,
                0x81,
                0x6E,
            ]
        );

        assert_eq!(
            controller_map_seven_byte(&controller, "cross-and-square", true),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::Left.bits(),
                !(Buttons2::Square | Buttons2::L2).bits(),
                // Analog sticks
                0x18,
                0xC6,
                0x81,
                0x6E,
            ]
        );
    }

    #[test]
    fn send_event_to_controller_works() -> Result<(), Box<std::error::Error>> {
        use self::mockstream::SharedMockStream;
        use super::send_event_to_controller;
        use super::ControllerEmulatorPacketType;
        use super::{Buttons1, Buttons2};
        use DUALSHOCK_MAGIC;
        use SEVEN_BYTE_OK_RESPONSE;
        use TWENTY_BYTE_OK_HEADER;

        let controller = FauxController::create_with_name(String::from("Apple Pippin Controller"));

        let seven_byte_console_response = vec![SEVEN_BYTE_OK_RESPONSE as u8];

        let mut serial = SharedMockStream::new();
        serial.push_bytes_to_read(&seven_byte_console_response);

        assert_eq!(
            send_event_to_controller(
                &mut serial,
                &controller,
                &ControllerEmulatorPacketType::SevenByte,
                "normal",
                false,
                false,
            )?,
            seven_byte_console_response
        );
        assert_eq!(
            serial.pop_bytes_written(),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::empty().bits(),
                !Buttons2::empty().bits(),
                // Analog sticks
                0x80,
                0x80,
                0x80,
                0x80,
            ]
        );

        let twenty_byte_console_response = vec![TWENTY_BYTE_OK_HEADER, 0x00, 0x00, 0x55];

        serial.push_bytes_to_read(&twenty_byte_console_response);

        assert_eq!(
            send_event_to_controller(
                &mut serial,
                &controller,
                &ControllerEmulatorPacketType::TwentyByte,
                "normal",
                false,
                false,
            )?,
            twenty_byte_console_response
        );
        assert_eq!(
            serial.pop_bytes_written(),
            vec![
                DUALSHOCK_MAGIC,
                !Buttons1::empty().bits(),
                !Buttons2::empty().bits(),
                // Analog sticks
                0x80,
                0x80,
                0x80,
                0x80,
                // Pressure values
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                // Mode footer
                0x55,
            ]
        );

        Ok(())
    }
}
