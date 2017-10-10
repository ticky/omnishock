extern crate sdl2;
extern crate serial;
#[macro_use]
extern crate clap;
use serial::prelude::SerialPort;
use std::io::prelude::{Read, Write};

mod sdl_manager;
use sdl_manager::SDLManager;

static DUALSHOCK_MAGIC: u8 = 0x5A;
static SEVEN_BYTE_OK_RESPONSE: char = 'k';
static SEVEN_BYTE_ERR_RESPONSE: char = 'x';

enum ControllerEmulatorPacketType {
    None, // Fallback, just log messages
    SevenByte, // For Johnny Chung Lee's firmware
    TwentyByte, // For Aaron Clovsky's firmware
}

fn main() {
    use clap::{AppSettings, Arg, SubCommand};

    let arguments = app_from_crate!()
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg(Arg::with_name("verbose").long("verbose").short("v").help(
            "Print more information about activity",
        ))
        .subcommand(
            SubCommand::with_name("ps2ce")
                .about(
                    "Start a transliteration session using a Teensy 2.0 PS2 Controller Emulator",
                )
                .arg(
                    Arg::with_name("device")
                        .index(1)
                        .takes_value(true)
                        .required(true)
                        .help(
                            "Device to use to communcate.\n\
                             (Usually /dev/cu.usbmodem12341 for USB Serial on macOS.)",
                        ),
                )
                .arg(
                    Arg::with_name("trigger-mode")
                        .long("trigger-mode")
                        .short("t")
                        .takes_value(true)
                        .help("How to map the analog triggers")
                        .default_value("normal")
                        .possible_value("normal")
                        .possible_value("right-stick")
                        .possible_value("cross-and-square"),
                ),
        )
        .subcommand(SubCommand::with_name("test").about(
            "Tests the game controller subsystem",
        ))
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

// Corner point for DualShock2: 0.835, Xbox One: 0.764

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

    let right_stick_x_value =
        convert_whole_axis(controller.axis(Axis::RightX) /*.saturating_mul(1.1)*/);
    let right_stick_y_value;
    let left_stick_x_value =
        convert_whole_axis(controller.axis(Axis::LeftX) /*.saturating_mul(1.1)*/);
    let left_stick_y_value =
        convert_whole_axis(controller.axis(Axis::LeftY) /*.saturating_mul(1.1)*/);

    // println!("right stick value: {} ({:x})", raw_right_stick_y, raw_right_stick_y);

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

            right_stick_y_value =
                convert_whole_axis(raw_right_stick_y /*.saturating_mul(1.1)*/);
        }
        _ => {
            l2_button_value = convert_half_axis_positive(raw_left_trigger);
            r2_button_value = convert_half_axis_positive(raw_right_trigger);

            cross_value = convert_button(controller.button(Button::A));
            square_value = convert_button(controller.button(Button::X));

            right_stick_y_value =
                convert_whole_axis(raw_right_stick_y /*.saturating_mul(1.1)*/);
        }
    }

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

    return vec![
        DUALSHOCK_MAGIC,
        collapse_bits(&buttons1).unwrap(),
        collapse_bits(&buttons2).unwrap(),
        right_stick_x_value,
        right_stick_y_value,
        left_stick_x_value,
        left_stick_y_value,
    ];
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

    if verbose {
        println!("Clearing serial buffer...");
    }

    // The Teensy might be waiting to send bytes to a previous
    // control session, if things didn't go so well.
    // Let's make sure there's nothing left in that pipe!
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
    }
    {
        if verbose {
            println!("Buffer received: {:?}", response);
        }
    }

    if verbose {
        println!("Determining device type...");
    }

    // Send one space character (this won't do anything on either type)
    serial.write(&vec![0x20])?;

    // Check the response!
    match serial.read(&mut response) {
        Ok(read) => {
            if read == 0 {
                communication_mode = ControllerEmulatorPacketType::TwentyByte;
                if verbose {
                    println!("No response. I suspect this is Aaron Clovsky's work!");
                }
            }
            if response[0] == (SEVEN_BYTE_ERR_RESPONSE as u8) {
                communication_mode = ControllerEmulatorPacketType::SevenByte;
                if verbose {
                    println!(
                        "Response was '{}': this is probably Johnny Chung Lee's work!",
                        SEVEN_BYTE_ERR_RESPONSE
                    );
                }
            } else {
                println!("Unrecognised response: {:?}", response);
            }
        }
        Err(error) => {
            println!("failed reading from device '{}': {}", device_path, error);
        }
    };

    let trigger_mode = command_arguments.value_of("trigger-mode").unwrap();

    if verbose {
        println!("Using trigger mode '{}'...", trigger_mode);
    }

    for event in sdl_manager.context.event_pump().unwrap().wait_iter() {
        use sdl2::event::Event;

        match event {
            Event::ControllerDeviceAdded { which, .. } => {
                if !sdl_manager.has_controller(which as u32).ok().unwrap_or(true) {
                    match sdl_manager.add_controller(which as u32) {
                        Ok(_) => {
                            println!(
                                "(There are {} controllers connected)",
                                sdl_manager.active_controllers.len()
                            );
                        }
                        Err(error) => {
                            println!(
                                "could not initialise connected joystick {}: {:?}",
                                which,
                                error
                            )
                        }
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
                    None => ()
                };
            }

            Event::ControllerAxisMotion { which, .. } |
            Event::ControllerButtonDown { which, .. } |
            Event::ControllerButtonUp { which, .. } => {
                if which != 0 {
                    continue;
                }

                let sent;
                let mut bytes_received = 0;
                let mut received = vec![0; 4];

                match communication_mode {
                    ControllerEmulatorPacketType::None => {
                        sent = controller_map_seven_byte(
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

                        sent = state;
                    }
                };

                if verbose {
                    println!("Sent: {:?}", sent);
                    if bytes_received > 0 {
                        println!("Received: {:?}", received);
                    }
                }
            }

            Event::Quit { .. } => break,
            _ => (),
        }
    }

    // let buf = vec!(
    //     DUALSHOCK_MAGIC,

    //     // Buttons (0=Pressed)
    //     //┌─────────── Left
    //     //│┌────────── Down
    //     //││┌───────── Right
    //     //│││┌──────── Up
    //     //││││┌─────── [Start>
    //     //│││││┌────── (R3)
    //     //││││││┌───── (L3)
    //     //│││││││┌──── [Select]
    //     0b11111111u8,
    //     0b11111111u8,
    //     //│││││││└──── [L2]
    //     //││││││└───── [R2]
    //     //│││││└────── [L1]
    //     //││││└─────── [R1]
    //     //│││└──────── Triangle
    //     //││└───────── Circle
    //     //│└────────── Cross
    //     //└─────────── Square

    //     // Sticks
    //     0x80, // Right stick X
    //     0x80, // Right stick Y
    //     0x80, // Left stick X
    //     0x80, // Left stick Y
    // );

    // serial.write(&buf[..])?;

    Ok(())
}

fn print_events(arguments: &clap::ArgMatches, sdl_manager: &mut SDLManager) {
    println!("Printing all controller events...");

    for event in sdl_manager.context.event_pump().unwrap().wait_iter() {
        use sdl2::event::Event;

        match event {
            Event::ControllerDeviceAdded { which, .. } => {
                if !sdl_manager.has_controller(which as u32).ok().unwrap_or(true) {
                    match sdl_manager.add_controller(which as u32) {
                        Ok(_) => {
                            println!(
                                "(There are {} controllers connected)",
                                sdl_manager.active_controllers.len()
                            );
                        }
                        Err(error) => {
                            println!(
                                "could not initialise connected joystick {}: {:?}",
                                which,
                                error
                            )
                        }
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
                    None => ()
                };
            }

            Event::ControllerDeviceRemapped { which, .. } => {
                println!(
                    "“{}” (#{}) remapped!",
                    sdl_manager.active_controllers[&which].controller.name(),
                    which
                );
            }

            Event::ControllerAxisMotion { which, axis, value, .. } => {
                println!(
                    "“{}” (#{}): {:?}: {}",
                    sdl_manager.active_controllers[&which].controller.name(),
                    which,
                    axis,
                    value
                );

                match sdl_manager.active_controllers.get_mut(&which) {
                    Some(controller_manager) => {
                        match controller_manager.haptic {
                            Some(ref mut haptic) => {
                                println!("Running haptic feedback for “{}”", controller_manager.controller.name());
                                haptic.rumble_stop();
                                haptic.rumble_play(1.0, 500);
                            }
                            _ => ()
                        }
                    }
                    _ => ()
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
