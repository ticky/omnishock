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
extern crate sdl2;
extern crate serial;
use serial::prelude::SerialPort;
use std::io::prelude::{Read, Write};

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
fn collapse_bits(bytes: &[u8]) -> Result<u8, String> {
    if !bytes.len() == 8 {
        return Err(format!(
            "Input must be 8 bytes long ({} elements provided)",
            bytes.len()
        ));
    }
    let mut result = 0;
    for (i, byte) in bytes.iter().enumerate() {
        let mask = (1 as u8) << i;

        // Are we setting this bit to 0 or 1?
        if *byte <= 0x80 {
            result |= mask;
        } else {
            result &= !mask;
        }
    }
    return Ok(result);
}

fn convert_button(button: bool) -> u8 {
    return match button {
        true => 0xFF,
        false => 0x00,
    };
}

fn convert_whole_axis(number: i16) -> u8 {
    return (number.wrapping_shr(8) + 0x80) as u8;
}

fn convert_half_axis_positive(stick: i16) -> u8 {
    if stick.is_positive() {
        return stick.wrapping_shr(7) as u8;
    }

    return 0;
}

fn convert_half_axis_negative(stick: i16) -> u8 {
    if stick.is_negative() {
        return (-(stick + 1)).wrapping_shr(7) as u8;
    }

    return 0;
}

fn combine_trigger_axes(left: i16, right: i16) -> u8 {
    return convert_whole_axis(left - right);
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

    let raw_left_trigger = controller.axis(Axis::TriggerLeft);
    let raw_right_trigger = controller.axis(Axis::TriggerRight);
    let raw_right_stick_y = controller.axis(Axis::RightY);

    // buttons1
    let select_value = convert_button(controller.button(Button::Back));
    let left_stick_value = convert_button(controller.button(Button::LeftStick));
    let right_stick_value = convert_button(controller.button(Button::RightStick));
    let start_value = convert_button(controller.button(Button::Start));
    let dpad_up_value = convert_button(controller.button(Button::DPadUp));
    let dpad_right_value = convert_button(controller.button(Button::DPadRight));
    let dpad_down_value = convert_button(controller.button(Button::DPadDown));
    let dpad_left_value = convert_button(controller.button(Button::DPadLeft));

    // buttons2
    let l2_button_value;
    let r2_button_value;
    let l1_button_value = convert_button(controller.button(Button::LeftShoulder));
    let r1_button_value = convert_button(controller.button(Button::RightShoulder));
    let triangle_value = convert_button(controller.button(Button::Y));
    let circle_value = convert_button(controller.button(Button::B));
    let cross_value;
    let square_value;

    let mut right_stick_x_value = convert_whole_axis(controller.axis(Axis::RightX));
    let mut right_stick_y_value;
    let mut left_stick_x_value = convert_whole_axis(controller.axis(Axis::LeftX));
    let mut left_stick_y_value = convert_whole_axis(controller.axis(Axis::LeftY));

    let pressure_right = convert_button(controller.button(Button::DPadRight));
    let pressure_left = convert_button(controller.button(Button::DPadLeft));
    let pressure_up = convert_button(controller.button(Button::DPadUp));
    let pressure_down = convert_button(controller.button(Button::DPadDown));
    let pressure_triangle = convert_button(controller.button(Button::Y));
    let pressure_circle = convert_button(controller.button(Button::B));
    let pressure_cross;
    let pressure_square;
    let pressure_l1 = convert_button(controller.button(Button::LeftShoulder));
    let pressure_r1 = convert_button(controller.button(Button::RightShoulder));
    let pressure_l2;
    let pressure_r2;

    match trigger_mode {
        "right-stick" => {
            l2_button_value = convert_half_axis_negative(raw_right_stick_y);
            r2_button_value = convert_half_axis_positive(raw_right_stick_y);

            cross_value = convert_button(controller.button(Button::A));
            square_value = convert_button(controller.button(Button::X));

            right_stick_y_value = combine_trigger_axes(raw_left_trigger, raw_right_trigger);
        }
        "cross-and-square" => {
            l2_button_value = convert_button(controller.button(Button::A));
            r2_button_value = convert_button(controller.button(Button::X));

            cross_value = convert_half_axis_positive(raw_right_trigger);
            square_value = convert_half_axis_positive(raw_left_trigger);

            right_stick_y_value = convert_whole_axis(raw_right_stick_y);
        }
        _ => {
            l2_button_value = convert_half_axis_positive(raw_left_trigger);
            r2_button_value = convert_half_axis_positive(raw_right_trigger);

            cross_value = convert_button(controller.button(Button::A));
            square_value = convert_button(controller.button(Button::X));

            right_stick_y_value = convert_whole_axis(raw_right_stick_y);
        }
    }

    pressure_l2 = l2_button_value;
    pressure_r2 = r2_button_value;
    pressure_cross = cross_value;
    pressure_square = square_value;

    let buttons1 = vec![
        select_value,
        left_stick_value,
        right_stick_value,
        start_value,
        dpad_up_value,
        dpad_right_value,
        dpad_down_value,
        dpad_left_value,
    ];

    let buttons2 = vec![
        l2_button_value,
        r2_button_value,
        l1_button_value,
        r1_button_value,
        triangle_value,
        circle_value,
        cross_value,
        square_value,
    ];

    let mode_footer = match controller.button(Button::Guide) {
        true => 0xAA,
        false => 0x55,
    };

    return vec![
        DUALSHOCK_MAGIC,
        collapse_bits(&buttons1).unwrap(),
        collapse_bits(&buttons2).unwrap(),
        right_stick_x_value,
        right_stick_y_value,
        left_stick_x_value,
        left_stick_y_value,
        pressure_right,
        pressure_left,
        pressure_up,
        pressure_down,
        pressure_triangle,
        pressure_circle,
        pressure_cross,
        pressure_square,
        pressure_l1,
        pressure_r1,
        pressure_l2,
        pressure_r2,
        mode_footer,
    ];
}

fn clear_serial_buffer(serial: &mut SerialPort) {
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

    let mut serial = match serial::open(device_path) {
        Ok(serial) => serial,
        Err(error) => panic!("failed to open serial device: {}", error),
    };

    serial.reconfigure(&|settings| {
        settings.set_baud_rate(serial::Baud9600)?;
        settings.set_char_size(serial::Bits8);
        Ok(())
    })?;

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
            println!("failed reading from device '{}': {}", device_path, error);
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

                    let sent;
                    let mut bytes_received = 0;
                    let mut received = vec![0; 4];

                    match communication_mode {
                        ControllerEmulatorPacketType::None => {
                            sent = controller_map_twenty_byte(
                                &sdl_manager.active_controllers[&which].controller,
                                trigger_mode,
                            );
                        }

                        ControllerEmulatorPacketType::SevenByte => {
                            let state = controller_map_seven_byte(
                                &sdl_manager.active_controllers[&which].controller,
                                trigger_mode,
                            );

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
                            let state = controller_map_twenty_byte(
                                &sdl_manager.active_controllers[&which].controller,
                                trigger_mode,
                            );

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

                    let mut received = vec![0; 4];

                    let state = controller_map_twenty_byte(
                        &sdl_manager.active_controllers[&controller_id].controller,
                        trigger_mode,
                    );

                    serial.write_all(&state)?;
                    let bytes_received = match serial.read(&mut received) {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            if verbose {
                                println!("Error reading response: {}", error);
                            }

                            0
                        }
                    };

                    if verbose {
                        println!("Sent: {:x}", HexView::from(&state));

                        if bytes_received > 0 {
                            println!("Received: {:x}", HexView::from(&received));
                        }
                    }
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

fn print_events(arguments: &clap::ArgMatches, sdl_manager: &mut SDLManager) {
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
