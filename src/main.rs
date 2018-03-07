/*
 * Omnishock: Something to do with game controllers!
 * Copyright (C) 2017-2018 Jessica Stokes
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
extern crate clap;
extern crate game_time;
extern crate hex_view;
use hex_view::HexView;
extern crate num;
extern crate sdl2;
extern crate serial;
use serial::prelude::SerialPort;
extern crate spin_sleep;
use std::cmp::{PartialEq, PartialOrd};
use std::convert::From;
use std::io::prelude::{Read, Write};
use std::ops::{Add, Div, Neg};

mod sdl_manager;
use sdl_manager::Gamepad;
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
const SERIAL_HINT: &'static str = "\n(Usually /dev/cu.usbmodem12341 for USB Serial on macOS.)";
#[cfg(all(unix, not(target_os = "macos")))]
const SERIAL_HINT: &'static str = "\n(Usually /dev/ttyUSB0 for USB Serial on Unix.)";
#[cfg(windows)]
const SERIAL_HINT: &'static str = "\n(Usually COM3 for USB Serial on Windows.)";

enum ControllerEmulatorPacketType {
    None,       // Fallback, just log messages
    SevenByte,  // For Johnny Chung Lee's firmware
    TwentyByte, // For Aaron Clovsky's firmware
}

fn main() {
    use clap::{AppSettings, Arg, SubCommand};

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

    let mut sdl_manager = SDLManager::init();

    println!(
        "(There are {} controllers connected)",
        sdl_manager.active_controllers.len()
    );

    match arguments.subcommand_name() {
        Some("ps2ce") => {
            send_to_ps2_controller_emulator(&arguments, &mut sdl_manager).unwrap();
        }
        Some("test") => {
            print_events(&arguments, &mut sdl_manager);
        }
        _ => (),
    }
}

// Misty gave me a special license exception for this stanza
// <https://twitter.com/mistydemeo/status/914745750369714176>
fn collapse_bits<T: num::Bounded + Add<Output = T> + Div<Output = T> + From<u8> + PartialOrd>(
    items: &[T],
) -> Result<u8, String> {
    let mid_point = whats_the_midpoint_of_a::<T>();

    if !items.len() == 8 {
        return Err(format!(
            "Input must be 8 items long ({} provided)",
            items.len()
        ));
    }

    let mut result = 0;

    // We process from most significant to least significant digit
    for (i, byte) in items.iter().enumerate() {
        let mask = u8::max_value() >> i;

        // Are we setting this bit to 0 or 1?
        if *byte > mid_point {
            result |= mask;
        } else {
            result &= !mask;
        }
    }

    return Ok(result);
}

fn whats_the_midpoint_of_a<T: num::Bounded + Add<Output = T> + Div<Output = T> + From<u8>>() -> T {
    return (T::max_value() + T::min_value()) / T::from(2);
}

fn convert_button<T: num::Bounded>(button: bool) -> T {
    return match button {
        true => T::max_value(),
        false => T::min_value(),
    };
}

fn convert_for_dualshock(number: i16) -> u8 {
    return (number.wrapping_shr(8) + 0x80) as u8;
}

fn convert_half_axis_positive<
    T: num::Bounded + num::Saturating + Copy + Div<Output = T> + PartialEq + From<u8>,
>(
    stick: T,
) -> T {
    // Special case the maximum values, so we don't end up with
    if stick == T::max_value() {
        return T::max_value();
    }

    let two_in_target_type = T::from(2);
    let half_minimum = T::min_value().div(two_in_target_type);
    let normalised_stick = stick.saturating_add(half_minimum);

    // This is a weird way to multiply by two but it works eh
    return normalised_stick.saturating_add(normalised_stick);
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
    return convert_half_axis_positive(stick.saturating_add(T::from(1)).neg());
}

fn normalise_stick_as_dualshock2(x: &mut i16, y: &mut i16) {
    // Adjust stick positions to match those of the DualShock®2.
    // The DualShock®2 has a prominent outer deadzone,
    // so we shrink the usable area here by 10%.
    *x = x.saturating_add(*x / 10);
    *y = y.saturating_add(*y / 10);
}

fn controller_map_seven_byte<T: Gamepad>(
    controller_manager: &T,
    trigger_mode: &str,
    normalise_sticks: bool,
) -> Vec<u8> {
    // Seven byte controller map is the same as
    // the first seven bytes of the twenty-byte map!
    let mut map = controller_map_twenty_byte(controller_manager, trigger_mode, normalise_sticks);
    map.truncate(7);
    return map;
}

fn controller_map_twenty_byte<T: Gamepad>(
    controller_manager: &T,
    trigger_mode: &str,
    normalise_sticks: bool,
) -> Vec<u8> {
    use sdl2::controller::{Axis, Button};

    // buttons1
    let dpad_left_value: i16 = convert_button(controller_manager.button(Button::DPadLeft));
    let dpad_down_value: i16 = convert_button(controller_manager.button(Button::DPadDown));
    let dpad_right_value: i16 = convert_button(controller_manager.button(Button::DPadRight));
    let dpad_up_value: i16 = convert_button(controller_manager.button(Button::DPadUp));
    let start_value: i16 = convert_button(controller_manager.button(Button::Start));
    let right_stick_value: i16 = convert_button(controller_manager.button(Button::RightStick));
    let left_stick_value: i16 = convert_button(controller_manager.button(Button::LeftStick));
    let select_value: i16 = convert_button(controller_manager.button(Button::Back));

    // buttons2
    let mut square_value: i16 = convert_button(controller_manager.button(Button::X));
    let mut cross_value: i16 = convert_button(controller_manager.button(Button::A));
    let circle_value: i16 = convert_button(controller_manager.button(Button::B));
    let triangle_value: i16 = convert_button(controller_manager.button(Button::Y));
    let r1_button_value: i16 = convert_button(controller_manager.button(Button::RightShoulder));
    let l1_button_value: i16 = convert_button(controller_manager.button(Button::LeftShoulder));
    let mut r2_button_value: i16 =
        convert_half_axis_positive(controller_manager.axis(Axis::TriggerRight));
    let mut l2_button_value: i16 =
        convert_half_axis_positive(controller_manager.axis(Axis::TriggerLeft));

    let mut right_stick_x_value: i16 = controller_manager.axis(Axis::RightX);
    let mut right_stick_y_value: i16 = controller_manager.axis(Axis::RightY);
    let mut left_stick_x_value: i16 = controller_manager.axis(Axis::LeftX);
    let mut left_stick_y_value: i16 = controller_manager.axis(Axis::LeftY);

    match trigger_mode {
        "right-stick" => {
            l2_button_value = convert_half_axis_negative(controller_manager.axis(Axis::RightY));
            r2_button_value = convert_half_axis_positive(controller_manager.axis(Axis::RightY));

            cross_value = convert_button(controller_manager.button(Button::A));
            square_value = convert_button(controller_manager.button(Button::X));

            // Combine the two raw trigger axes by subtracting one from the other
            // NOTE: This doesn't allow for both to be used at once
            right_stick_y_value = controller_manager.axis(Axis::TriggerLeft)
                - controller_manager.axis(Axis::TriggerRight);
        }
        "cross-and-square" => {
            l2_button_value = convert_button(controller_manager.button(Button::A));
            r2_button_value = convert_button(controller_manager.button(Button::X));

            cross_value = convert_half_axis_positive(controller_manager.axis(Axis::TriggerRight));
            square_value = convert_half_axis_positive(controller_manager.axis(Axis::TriggerLeft));
        }
        _ => (),
    }

    if normalise_sticks {
        normalise_stick_as_dualshock2(&mut right_stick_x_value, &mut right_stick_y_value);
        normalise_stick_as_dualshock2(&mut left_stick_x_value, &mut left_stick_y_value);
    }

    let buttons1 = vec![
        dpad_left_value,
        dpad_down_value,
        dpad_right_value,
        dpad_up_value,
        start_value,
        right_stick_value,
        left_stick_value,
        select_value,
    ];

    let buttons2 = vec![
        square_value,
        cross_value,
        circle_value,
        triangle_value,
        r1_button_value,
        l1_button_value,
        r2_button_value,
        l2_button_value,
    ];

    let mode_footer = match controller_manager.button(Button::Guide) {
        true => 0xAA,
        false => 0x55,
    };

    return vec![
        DUALSHOCK_MAGIC,
        // DualShock protocol considers 0 to mean
        // pressed and 1 to mean not pressed, so
        // we NOT the output from collapse_bits here
        !(collapse_bits(&buttons1).unwrap()),
        !(collapse_bits(&buttons2).unwrap()),
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
) -> std::io::Result<()> {
    let verbose = arguments.is_present("verbose");
    let command_arguments = arguments.subcommand_matches("ps2ce").unwrap();
    let device_path = command_arguments.value_of("device").unwrap();

    if verbose {
        println!(
            "Connecting to PS2 Controller Emulator device at '{}'...",
            device_path
        );
    }

    let serial = match serial::open(device_path) {
        Ok(mut serial) => {
            serial.reconfigure(&|settings| {
                settings.set_baud_rate(serial::Baud9600)?;
                settings.set_char_size(serial::Bits8);
                Ok(())
            })?;

            serial
        }
        Err(error) => panic!("failed to open serial device: {}", error),
    };

    send_to_ps2_controller_emulator_via(arguments, sdl_manager, serial)
}

fn send_to_ps2_controller_emulator_via<I: Read + Write>(
    arguments: &clap::ArgMatches,
    sdl_manager: &mut SDLManager,
    mut serial: I,
) -> std::io::Result<()> {
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
    serial.write(&vec![
        DUALSHOCK_MAGIC,
        // Buttons (0=Pressed)
        //┌─────────── Left
        //│┌────────── Down
        //││┌───────── Right
        //│││┌──────── Up
        //││││┌─────── [Start>
        //│││││┌────── (R3)
        //││││││┌───── (L3)
        //│││││││┌──── [Select]
        0b11111111u8,
        0b11111111u8,
        //│││││││└──── [L2]
        //││││││└───── [R2]
        //│││││└────── [L1]
        //││││└─────── [R1]
        //│││└──────── Triangle
        //││└───────── Circle
        //│└────────── Cross
        //└─────────── Square

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

    let normalise_sticks = command_arguments.is_present("no-stick-normalise") == false;

    if verbose {
        match normalise_sticks {
            true => println!("Normalising stick extents (stick values * 1.1)"),
            false => println!("Not normalising stick extents"),
        }
    }

    let mut event_pump = sdl_manager.context.event_pump().unwrap();

    // We use `game_time` to keep track of "frame" time and try to hit a
    // consistent rate at all times. We use `spin_sleep` instead of
    // `thread::Sleep` to get more accurate sleep times on all platforms.
    use game_time::{FloatDuration, FrameCount, FrameCounter, GameClock};
    use game_time::framerate::RunningAverageSampler;

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
                    match sdl_manager.remove_controller(which) {
                        Some(_) => {
                            println!(
                                "(There are {} controllers connected)",
                                sdl_manager.active_controllers.len()
                            );
                        }
                        None => (),
                    };
                }

                Event::Quit { .. } => break 'outer,
                _ => (),
            }
        }

        // Now that we've kept track of controller additions & removals,
        // post an update for the one controller we currently care about.
        match sdl_manager.active_controllers.get_mut(&0) {
            Some(controller_manager) => {
                let response = send_event_to_controller(
                    &mut serial,
                    controller_manager,
                    &communication_mode,
                    trigger_mode,
                    normalise_sticks,
                    verbose,
                )?;

                // If we've receieved a response from the controller, and our
                // controller supports haptic feedback, update its haptic state
                if !response.is_empty() {
                    let controller_name = controller_manager.name();

                    match controller_manager.haptic {
                        Some(ref mut haptic) => {
                            let small_motor_intensity = response[1];
                            let large_motor_intensity = response[2];

                            // We calculate the rumble intensity as 1/3 of the small
                            // motor, plus the full intensity of the large motor
                            let rumble_intensity = small_motor_intensity as f32 / 255.0 / 3.0
                                + large_motor_intensity as f32 / 255.0;

                            if verbose {
                                println!(
                                    "Setting haptic feedback to {} for {}",
                                    rumble_intensity, controller_name
                                );
                            }

                            // NOTE: Should probably be an <https://wiki.libsdl.org/SDL_HapticLeftRight>,
                            //       but the `sdl2` crate doesn't yet support it.
                            haptic.rumble_stop();
                            if rumble_intensity > 0.0 {
                                haptic.rumble_play(rumble_intensity, 500);
                            }
                        }
                        _ => (),
                    }
                }
            }
            _ => (),
        }

        // Having run all our processing for this iteration, accurately sleep
        // until we need to process the next one
        clock.sleep_remaining_via(&counter, |rem| spin_sleeper.sleep(rem.to_std().unwrap()));
    }

    Ok(())
}

fn send_event_to_controller<I: Read + Write>(
    serial: &mut I,
    controller_manager: &sdl_manager::ControllerManager,
    communication_mode: &ControllerEmulatorPacketType,
    trigger_mode: &str,
    normalise_sticks: bool,
    verbose: bool,
) -> std::io::Result<Vec<u8>> {
    let sent;
    let mut bytes_received = 0;
    let mut received = vec![0; 4];

    match *communication_mode {
        ControllerEmulatorPacketType::None => {
            sent = controller_map_twenty_byte(controller_manager, trigger_mode, normalise_sticks);
        }

        ControllerEmulatorPacketType::SevenByte => {
            let state =
                controller_map_seven_byte(controller_manager, trigger_mode, normalise_sticks);

            serial.write_all(&state)?;
            bytes_received = match serial.read(&mut received) {
                Ok(bytes) => bytes,
                Err(error) => {
                    if verbose {
                        println!("Error reading response: {}", error);
                    }
                    0
                }
            };

            if received[0] != (SEVEN_BYTE_OK_RESPONSE as u8) {
                println!("WARNING: Adapter responded with an error status.")
            }

            sent = state;
        }

        ControllerEmulatorPacketType::TwentyByte => {
            let state =
                controller_map_twenty_byte(controller_manager, trigger_mode, normalise_sticks);

            serial.write_all(&state)?;
            bytes_received = match serial.read(&mut received) {
                Ok(bytes) => bytes,
                Err(error) => {
                    if verbose {
                        println!("Error reading response: {}", error);
                    }

                    0
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

fn print_events(_arguments: &clap::ArgMatches, sdl_manager: &mut SDLManager) {
    println!("Printing all controller events...");

    for event in sdl_manager.context.event_pump().unwrap().wait_iter() {
        use sdl2::event::Event;

        match event {
            Event::ControllerDeviceAdded { which, .. } => {
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
                match sdl_manager.remove_controller(which) {
                    Some(_) => {
                        println!(
                            "(There are {} controllers connected)",
                            sdl_manager.active_controllers.len()
                        );
                    }
                    None => (),
                };
            }

            Event::ControllerAxisMotion {
                which, axis, value, ..
            } => {
                println!(
                    "“{}” (#{}): {:?}: {}",
                    sdl_manager.active_controllers[&which].name(),
                    which,
                    axis,
                    value
                );

                match sdl_manager.active_controllers.get_mut(&which) {
                    Some(controller_manager) => {
                        let controller_name = controller_manager.name();

                        match controller_manager.haptic {
                            Some(ref mut haptic) => {
                                println!("Running haptic feedback for “{}”", controller_name);
                                haptic.rumble_stop();
                                haptic.rumble_play(1.0, 500);
                            }
                            _ => (),
                        }
                    }
                    _ => (),
                };
            }

            Event::ControllerButtonDown { which, button, .. } => {
                println!(
                    "“{}” (#{}): {:?}: down",
                    sdl_manager.active_controllers[&which].name(),
                    which,
                    button
                );
            }

            Event::ControllerButtonUp { which, button, .. } => {
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
}

#[cfg(test)]
mod tests {
    #[test]
    fn collapse_bits_works() {
        use super::collapse_bits;

        assert_eq!(
            collapse_bits::<u8>(&vec![0, 0, 0, 0, 0, 0, 0, 0]).unwrap(),
            0
        );
        assert_eq!(
            collapse_bits::<u8>(&vec![0, 255, 0, 0, 0, 255, 0, 255]).unwrap(),
            0b01000101u8
        );
        assert_eq!(
            !(collapse_bits::<u8>(&vec![0, 255, 0, 0, 0, 255, 0, 255]).unwrap()),
            0b10111010u8
        );

        assert_eq!(
            collapse_bits::<u8>(&vec![130, 150, 170, 180, 128, 200, 220, 240]).unwrap(),
            255
        );
        assert_eq!(
            !(collapse_bits::<u8>(&vec![0, 0, 0, 0, 0, 0, 0, 0]).unwrap()),
            255
        );
    }

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
        use super::whats_the_midpoint_of_a;

        assert_eq!(whats_the_midpoint_of_a::<u8>(), 127_u8);
        assert_eq!(
            whats_the_midpoint_of_a::<u64>(),
            9_223_372_036_854_775_807_u64
        );
        assert_eq!(whats_the_midpoint_of_a::<i16>(), 0_i16);
        assert_eq!(whats_the_midpoint_of_a::<i64>(), 0_i64);
        assert_eq!(whats_the_midpoint_of_a::<f32>(), 0_f32);
    }

    #[test]
    fn convert_button_is_accurate() {
        use super::convert_button;

        assert_eq!(convert_button::<u8>(true), u8::max_value());
        assert_eq!(convert_button::<i64>(true), i64::max_value());
        assert_eq!(convert_button::<u8>(false), u8::min_value());
        assert_eq!(convert_button::<i64>(false), i64::min_value());
    }

    use sdl2;
    use std::collections::HashMap;
    use sdl_manager::Gamepad;

    struct FauxController {
        name: String,
        buttons: HashMap<sdl2::controller::Button, bool>,
        axes: HashMap<sdl2::controller::Axis, i16>,
    }

    impl FauxController {
        fn create_with_name(name: String) -> FauxController {
            let buttons = HashMap::new();
            let axes = HashMap::new();
            let new_controller = FauxController {
                name,
                buttons,
                axes,
            };

            return new_controller;
        }

        fn set_button(&mut self, button: sdl2::controller::Button, value: bool) {
            self.buttons.insert(button, value);
        }

        fn set_axis(&mut self, axis: sdl2::controller::Axis, value: i16) {
            self.axes.insert(axis, value);
        }
    }

    impl Gamepad for FauxController {
        fn name(&self) -> String {
            self.name.clone()
        }

        fn button(&self, button: sdl2::controller::Button) -> bool {
            *self.buttons.get(&button).unwrap_or(&false)
        }

        fn axis(&self, axis: sdl2::controller::Axis) -> i16 {
            *self.axes.get(&axis).unwrap_or(&0)
        }
    }

    #[test]
    fn controller_map_twenty_byte_works() {
        use DUALSHOCK_MAGIC;
        use super::controller_map_twenty_byte;
        use sdl2::controller::{Axis, Button};

        let mut controller =
            FauxController::create_with_name(String::from("Applejack Game-player Pad"));

        assert_eq!(
            controller_map_twenty_byte(&controller, "", true),
            vec![
                DUALSHOCK_MAGIC,
                // buttons1
                255,
                // buttons2
                255,
                // Analog sticks
                128,
                128,
                128,
                128,
                // Pressure values
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                // Mode footer
                85,
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
            controller_map_twenty_byte(&controller, "", true),
            vec![
                DUALSHOCK_MAGIC,
                // buttons1
                127,
                // buttons2
                190,
                // Analog sticks
                24,
                198,
                129,
                110,
                // Pressure values
                0,
                255,
                0,
                0,
                0,
                0,
                255,
                0,
                0,
                0,
                255,
                0,
                // Mode footer
                85,
            ]
        );
    }

    #[test]
    fn controller_map_seven_byte_works() {
        use DUALSHOCK_MAGIC;
        use super::controller_map_seven_byte;
        use sdl2::controller::{Axis, Button};

        let mut controller =
            FauxController::create_with_name(String::from("Apple Pippin Controller"));

        assert_eq!(
            controller_map_seven_byte(&controller, "", true),
            vec![
                DUALSHOCK_MAGIC,
                // buttons1
                255,
                // buttons2
                255,
                // Analog sticks
                128,
                128,
                128,
                128,
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
            controller_map_seven_byte(&controller, "", true),
            vec![
                DUALSHOCK_MAGIC,
                // buttons1
                127,
                // buttons2
                190,
                // Analog sticks
                24,
                198,
                129,
                110,
            ]
        );
    }
}
