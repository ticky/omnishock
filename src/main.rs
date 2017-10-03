extern crate sdl2;
extern crate serial;
#[macro_use]
extern crate clap;
use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use serial::prelude::*;

static DUALSHOCK_MAGIC: u8 = 0x5A;

enum ControllerEmulatorPacketType {
    None,       // Fallback, just log messages
    SevenByte,  // For Johnny Chung Lee's firmware
    TwentyByte, // For pelvicthrustman's firmware
}

fn main() {
    let global_arguments = app_from_crate!()
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .arg(clap::Arg::with_name("verbose")
            .long("verbose")
            .short("v")
            .help("Print more information about activity"))
        .subcommand(clap::SubCommand::with_name("ps2ce")
            .about("Start a transliteration session using a Teensy 2.0 PS2 Controller Emulator")
            .arg(clap::Arg::with_name("device")
                .index(1)
                .takes_value(true)
                .required(true)
                .help("USB Serial device to use to communcate.\nUsually /dev/cu.usbmodem12341 on macOS.")))
        .subcommand(clap::SubCommand::with_name("test").about("Tests the game controller subsystem"))
        .get_matches();

    // Initialise SDL2, and the game controller subsystem
    let sdl_context = sdl2::init().unwrap();
    let game_controller_subsystem = sdl_context.game_controller().unwrap();

    // Load pre-set controller mappings (note that SDL will still read
    // others from the SDL_GAMECONTROLLERCONFIG environment variable)
    let controller_mappings = include_str!("../vendor/SDL_GameControllerDB/gamecontrollerdb.txt")
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'));

    // Load each mapping individually rather than using load_mappings,
    // as it turns out doing them together can break without warning
    // if the file's syntax is ever invalid
    for mapping in controller_mappings {
        match game_controller_subsystem.add_mapping(mapping) {
            Err(error) => panic!("failed to load mapping: {}", error),
            _ => (),
        }
    }

    // Keep track of the controllers we know of
    let mut active_controllers: HashMap<i32, sdl2::controller::GameController> = HashMap::new();

    // Look into controllers that were already connected at start-up
    let joystick_count = match game_controller_subsystem.num_joysticks() {
        Ok(count) => count,
        Err(error) => panic!("failed to enumerate joysticks: {}", error),
    };

    for id in 0..joystick_count {
        if game_controller_subsystem.is_game_controller(id) {
            match game_controller_subsystem.open(id) {
                Ok(controller) => {
                    let controller_id = &controller.instance_id();
                    println!("{} (#{}): found", controller.name(), controller_id);
                    active_controllers.insert(*controller_id, controller);
                }
                Err(error) => {
                    println!("could not initialise joystick {} as controller: {:?}",
                             id,
                             error)
                }
            }
        }
    }

    println!("(There are {} controllers connected)",
             active_controllers.len());

    let subcommand_name = global_arguments.subcommand_name();

    match subcommand_name {
        Some("ps2ce") => {
            let command_arguments = global_arguments.subcommand_matches("ps2ce").unwrap();

            send_to_ps2_controller_emulator(&global_arguments,
                                            command_arguments,
                                            sdl_context,
                                            game_controller_subsystem,
                                            &mut active_controllers).unwrap();
        }
        Some("test") => {
            print_events(sdl_context,
                         game_controller_subsystem,
                         &mut active_controllers);
        }
        _ => (),
    }
}

// Misty gave me a special license exception for this stanza
// <https://twitter.com/mistydemeo/status/914745750369714176>
fn collapse_bits(bytes : &[u8]) -> Result<u8, String> {
    if !bytes.len() == 8 {
        return Err(format!("Input must be 8 bytes long ({} elements provided)", bytes.len()));
    }
    let mut result = 0;
    for (i, byte) in bytes.iter().enumerate() {
        let mask = (1 as u8) << i;

        // Are we setting this bit to 0 or 1?
        // Values are expected to be 0 or greater.
        if *byte == 0 {
            result |= mask;
        } else {
            result &= !mask;
        }
    }
    return Ok(result);
}

fn controller_button_to_bit(button: bool) -> u8 {
    return match button {
        true => 1,
        false => 0
    }
}

fn controller_map_for_ps2_controller_emulator(controller: &sdl2::controller::GameController) -> Vec<u8> {
    let buttons1 = vec!(
        controller_button_to_bit(controller.button(sdl2::controller::Button::Back)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::LeftStick)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::RightStick)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::Start)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::DPadUp)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::DPadRight)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::DPadDown)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::DPadLeft))
    );

    let buttons2 = vec!(
        // These trigger axes use 0i16...i16::max_value(),
        // not i16::min_value()..i16::max_value()
        (controller.axis(sdl2::controller::Axis::TriggerLeft) as u16 >> 7) as u8,
        (controller.axis(sdl2::controller::Axis::TriggerRight) as u16 >> 7) as u8,
        controller_button_to_bit(controller.button(sdl2::controller::Button::LeftShoulder)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::RightShoulder)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::Y)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::B)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::A)),
        controller_button_to_bit(controller.button(sdl2::controller::Button::X))
    );

    return vec!(
        DUALSHOCK_MAGIC,
        collapse_bits(&buttons1).unwrap(),
        collapse_bits(&buttons2).unwrap(),
        ((controller.axis(sdl2::controller::Axis::RightX) >> 8) + 0x80) as u8,
        ((controller.axis(sdl2::controller::Axis::RightY) >> 8) + 0x80) as u8,
        ((controller.axis(sdl2::controller::Axis::LeftX) >> 8) + 0x80) as u8,
        ((controller.axis(sdl2::controller::Axis::LeftY) >> 8) + 0x80) as u8
    );
}

fn send_to_ps2_controller_emulator(global_arguments: &clap::ArgMatches,
                                   command_arguments: &clap::ArgMatches,
                                   sdl_context: sdl2::Sdl,
                                   game_controller_subsystem: sdl2::GameControllerSubsystem,
                                   active_controllers: &mut HashMap<i32, sdl2::controller::GameController>) -> io::Result<()> {
    let verbose = global_arguments.is_present("verbose");
    let device_path = command_arguments.value_of("device").unwrap();

    if verbose {
        println!("Connecting to PS2 Controller Emulator device at '{}'...", device_path);
    }

    let mut serial = match serial::open(device_path) {
        Ok(serial) => serial,
        Err(error) => panic!("failed to open serial device: {}", error)
    };

    serial.reconfigure(&|settings| {
        settings.set_baud_rate(serial::Baud9600)?;
        settings.set_char_size(serial::Bits8);
        Ok(())
    })?;

    let mut communication_mode = ControllerEmulatorPacketType::None;

    if verbose {
        println!("Connected! Determining device type...");
    }

    // Create a four-byte response buffer
    let mut response = vec![0; 4];

    // Send one space character (this won't do anything on either type)
    serial.write(&vec!(0x20))?;

    // Check the response!
    match serial.read(&mut response) {
        Ok(read) => {
            if read == 0 {
                communication_mode = ControllerEmulatorPacketType::TwentyByte;
                if verbose {
                    println!("No response. I suspect this is pelvicthrustman's work!");
                }
            } if response[0] == ('x' as u8) {
                communication_mode = ControllerEmulatorPacketType::SevenByte;
                if verbose {
                    println!("We got an 'x' back - this is probably Johnny Chung Lee's work!");
                }
            } else {
                println!("Unrecognised response: {:?}", response);
            }
        },
        Err(error) => {
            println!("failed reading from device '{}': {}", device_path, error);
        }
    };

    for event in sdl_context.event_pump().unwrap().wait_iter() {
        use sdl2::event::Event;

        match event {
            Event::ControllerDeviceAdded { which, .. } => {
                match game_controller_subsystem.open(which as u32) {
                    Ok(controller) => {
                        let controller_id = &controller.instance_id();
                        if !active_controllers.contains_key(controller_id) {
                            println!("{} (#{}): connected", controller.name(), controller_id);
                            println!("(There are {} controllers connected)",
                                     active_controllers.len() + 1);
                            active_controllers.insert(*controller_id, controller);
                        }
                    }
                    Err(error) => {
                        println!("could not initialise connected joystick {}: {:?}",
                                 which,
                                 error)
                    }
                }
            }

            Event::ControllerDeviceRemoved { which, .. } => {
                println!("{} (#{}): disconnected",
                         active_controllers[&which].name(),
                         which);
                println!("(There are {} controllers connected)",
                         active_controllers.len() - 1);
                active_controllers.remove(&which);
            }

            Event::ControllerAxisMotion { which, .. } | Event::ControllerButtonDown { which, .. } | Event::ControllerButtonUp { which, .. } => {
                if which != 0 {
                    continue
                }

                let sent;

                match communication_mode {
                    ControllerEmulatorPacketType::None => {
                        sent = controller_map_for_ps2_controller_emulator(&active_controllers[&which]);
                    }

                    ControllerEmulatorPacketType::SevenByte => {
                        let state = controller_map_for_ps2_controller_emulator(&active_controllers[&which]);

                        serial.write_all(&state)?;

                        sent = state;
                    }

                    ControllerEmulatorPacketType::TwentyByte => {
                        let state = controller_map_for_ps2_controller_emulator(&active_controllers[&which]);

                        serial.write_all(&state)?;

                        sent = state;
                    }
                };

                if verbose {
                    println!("Sent: {:?}", sent);
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

fn print_events(sdl_context: sdl2::Sdl,
                game_controller_subsystem: sdl2::GameControllerSubsystem,
                active_controllers: &mut HashMap<i32, sdl2::controller::GameController>) {
    println!("Printing all controller events...");
    for event in sdl_context.event_pump().unwrap().wait_iter() {
        use sdl2::event::Event;

        match event {
            Event::ControllerDeviceAdded { which, .. } => {
                match game_controller_subsystem.open(which as u32) {
                    Ok(controller) => {
                        let controller_id = &controller.instance_id();
                        if !active_controllers.contains_key(controller_id) {
                            println!("{} (#{}): connected", controller.name(), controller_id);
                            println!("(There are {} controllers connected)",
                                     active_controllers.len() + 1);
                            active_controllers.insert(*controller_id, controller);
                        }
                    }
                    Err(error) => {
                        println!("could not initialise connected joystick {}: {:?}",
                                 which,
                                 error)
                    }
                }
            }

            Event::ControllerDeviceRemoved { which, .. } => {
                println!("{} (#{}): disconnected",
                         active_controllers[&which].name(),
                         which);
                println!("(There are {} controllers connected)",
                         active_controllers.len() - 1);
                active_controllers.remove(&which);
            }

            Event::ControllerDeviceRemapped { which, .. } => {
                println!("{} (#{}) remapped!",
                         active_controllers[&which].name(),
                         which);
            }

            Event::ControllerAxisMotion { which, axis, value, .. } => {
                println!("{} (#{}): {:?}: {}",
                         active_controllers[&which].name(),
                         which,
                         axis,
                         value);
            }

            Event::ControllerButtonDown { which, button, .. } => {
                println!("{} (#{}): {:?}: down",
                         active_controllers[&which].name(),
                         which,
                         button);
            }

            Event::ControllerButtonUp { which, button, .. } => {
                println!("{} (#{}): {:?}: up",
                         active_controllers[&which].name(),
                         which,
                         button);
            }

            Event::Quit { .. } => break,
            _ => (),
        }
    }
}
