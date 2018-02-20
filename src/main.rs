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
extern crate coord_transforms;
extern crate hex_view;
use hex_view::HexView;
extern crate nalgebra;
extern crate num;
extern crate sdl2;
extern crate serial;
use serial::prelude::SerialPort;
use std::convert::From;
use std::cmp::PartialOrd;
use std::io::prelude::{Read, Write};
use std::ops::{Add, Div};

mod sdl_manager;
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
                .about("Start a transliteration session using a Teensy 2.0 PS2 Controller Emulator")
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

fn convert_half_axis_positive(stick: i16) -> i16 {
    if stick == i16::max_value() {
        return i16::max_value();
    }

    if stick <= 0 {
        return i16::min_value();
    }

    return (stick as i32 * 2).saturating_add(i16::min_value() as i32) as i16;
}

fn convert_half_axis_negative(stick: i16) -> i16 {
    return convert_half_axis_positive(-(stick.saturating_add(1)));
}

fn normalise_stick(x: &mut i16, y: &mut i16) {
    use coord_transforms::d2::{cartesian2polar, polar2cartesian};

    let stick_cartesian_vector = nalgebra::Vector2::new(*x as f64, *y as f64);

    let mut stick_polar = cartesian2polar(&stick_cartesian_vector);

    // Corner point for DualShock2: 0.835, Xbox One: 0.764
    let stick_rho = stick_polar[0] * 1.1;
    let stick_theta = stick_polar[1];

    stick_polar = nalgebra::Vector2::new(stick_rho, stick_theta);

    let stick_cartesian = polar2cartesian(&stick_polar);

    println!("{}, {} => {}, {}", x, y, stick_cartesian[0].round() as i16, stick_cartesian[1].round() as i16);

    *x = stick_cartesian[0].round() as i16;
    *y = stick_cartesian[1].round() as i16;
}

fn controller_map_seven_byte(
    controller: &sdl2::controller::GameController,
    trigger_mode: &str,
) -> Vec<u8> {
    // Seven byte controller map is the same as
    // the first seven bytes of the twenty-byte map!
    let mut map = controller_map_twenty_byte(controller, trigger_mode);
    map.truncate(7);
    return map;
}

fn controller_map_twenty_byte(
    controller: &sdl2::controller::GameController,
    trigger_mode: &str,
) -> Vec<u8> {
    use sdl2::controller::{Axis, Button};

    // buttons1
    let dpad_left_value: i16 = convert_button(controller.button(Button::DPadLeft));
    let dpad_down_value: i16 = convert_button(controller.button(Button::DPadDown));
    let dpad_right_value: i16 = convert_button(controller.button(Button::DPadRight));
    let dpad_up_value: i16 = convert_button(controller.button(Button::DPadUp));
    let start_value: i16 = convert_button(controller.button(Button::Start));
    let right_stick_value: i16 = convert_button(controller.button(Button::RightStick));
    let left_stick_value: i16 = convert_button(controller.button(Button::LeftStick));
    let select_value: i16 = convert_button(controller.button(Button::Back));

    // buttons2
    let mut square_value: i16 = convert_button(controller.button(Button::X));
    let mut cross_value: i16 = convert_button(controller.button(Button::A));
    let circle_value: i16 = convert_button(controller.button(Button::B));
    let triangle_value: i16 = convert_button(controller.button(Button::Y));
    let r1_button_value: i16 = convert_button(controller.button(Button::RightShoulder));
    let l1_button_value: i16 = convert_button(controller.button(Button::LeftShoulder));
    let mut r2_button_value: i16 = convert_half_axis_positive(controller.axis(Axis::TriggerRight));
    let mut l2_button_value: i16 = convert_half_axis_positive(controller.axis(Axis::TriggerLeft));

    let mut right_stick_x_value: i16 = controller.axis(Axis::RightX);
    let mut right_stick_y_value: i16 = controller.axis(Axis::RightY);
    let mut left_stick_x_value: i16 = controller.axis(Axis::LeftX);
    let mut left_stick_y_value: i16 = controller.axis(Axis::LeftY);

    match trigger_mode {
        "right-stick" => {
            l2_button_value = convert_half_axis_negative(controller.axis(Axis::RightY));
            r2_button_value = convert_half_axis_positive(controller.axis(Axis::RightY));

            cross_value = convert_button(controller.button(Button::A));
            square_value = convert_button(controller.button(Button::X));

            // Combine the two raw trigger axes by subtracting one from the other
            // NOTE: This doesn't allow for both to be used at once
            right_stick_y_value =
                controller.axis(Axis::TriggerLeft) - controller.axis(Axis::TriggerRight);
        }
        "cross-and-square" => {
            l2_button_value = convert_button(controller.button(Button::A));
            r2_button_value = convert_button(controller.button(Button::X));

            cross_value = convert_half_axis_positive(controller.axis(Axis::TriggerRight));
            square_value = convert_half_axis_positive(controller.axis(Axis::TriggerLeft));
        }
        _ => (),
    }

    normalise_stick(&mut right_stick_x_value, &mut right_stick_y_value);
    normalise_stick(&mut left_stick_x_value, &mut left_stick_y_value);

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

    let mode_footer = match controller.button(Button::Guide) {
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

    let mut event_pump = sdl_manager.context.event_pump().unwrap();

    'outer: loop {
        // Wait for any events; but time out after 500ms
        for event in event_pump.wait_timeout_iter(500) {
            // TODO: Decouple and unit test *this* bit
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

                Event::ControllerAxisMotion { which, .. }
                | Event::ControllerButtonDown { which, .. }
                | Event::ControllerButtonUp { which, .. } => {
                    if which != 0 {
                        continue;
                    }

                    send_event_to_controller(
                        &mut serial,
                        &sdl_manager.active_controllers[&which].controller,
                        &communication_mode,
                        trigger_mode,
                        verbose,
                    )?;
                }

                Event::Quit { .. } => break 'outer,
                _ => (),
            }
        }

        // Timeout reached: If we're talking to a device that needs it,
        // force an update, then continue to truck
        match communication_mode {
            ControllerEmulatorPacketType::TwentyByte => {
                let controller_id = 0;

                if sdl_manager.active_controllers.contains_key(&controller_id) {
                    if verbose {
                        println!("Sending update due to timeout");
                    }

                    send_event_to_controller(
                        &mut serial,
                        &sdl_manager.active_controllers[&controller_id].controller,
                        &communication_mode,
                        trigger_mode,
                        verbose,
                    )?;
                } else {
                    if verbose {
                        println!("Timed out but no controller is connected, so doing nothing.");
                    }
                }
            }
            _ => (),
        };
    }

    Ok(())
}

fn send_event_to_controller<I: Read + Write>(
    serial: &mut I,
    controller: &sdl2::controller::GameController,
    communication_mode: &ControllerEmulatorPacketType,
    trigger_mode: &str,
    verbose: bool,
) -> std::io::Result<()> {
    let sent;
    let mut bytes_received = 0;
    let mut received = vec![0; 4];

    match *communication_mode {
        ControllerEmulatorPacketType::None => {
            sent = controller_map_twenty_byte(controller, trigger_mode);
        }

        ControllerEmulatorPacketType::SevenByte => {
            let state = controller_map_seven_byte(controller, trigger_mode);

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
            let state = controller_map_twenty_byte(controller, trigger_mode);

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

    if verbose {
        println!("Sent: {:x}", HexView::from(&sent));

        if bytes_received > 0 {
            println!("Received: {:x}", HexView::from(&received));
        }
    }

    Ok(())
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
                    sdl_manager.active_controllers[&which].controller.name(),
                    which,
                    axis,
                    value
                );

                match sdl_manager.active_controllers.get_mut(&which) {
                    Some(controller_manager) => match controller_manager.haptic {
                        Some(ref mut haptic) => {
                            println!(
                                "Running haptic feedback for “{}”",
                                controller_manager.controller.name()
                            );
                            haptic.rumble_stop();
                            haptic.rumble_play(1.0, 500);
                        }
                        _ => (),
                    },
                    _ => (),
                };
            }

            Event::ControllerButtonDown { which, button, .. } => {
                println!(
                    "“{}” (#{}): {:?}: down",
                    sdl_manager.active_controllers[&which].controller.name(),
                    which,
                    button
                );
            }

            Event::ControllerButtonUp { which, button, .. } => {
                println!(
                    "“{}” (#{}): {:?}: up",
                    sdl_manager.active_controllers[&which].controller.name(),
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

        assert_eq!(convert_button::<u8>(true), 255_u8);
    }
}
