extern crate sdl2;
extern crate serial;
#[macro_use]
extern crate clap;
use clap::{Arg, AppSettings, SubCommand};
use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use serial::prelude::*;

fn main() {
    let matches = app_from_crate!()
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("ps2ce")
            .about("Start a transliteration session using a Teensy 2.0 PS2 Controller Emulator")
            .arg(Arg::with_name("device")
                .index(1)
                .takes_value(true)
                .required(true)
                .help("USB Serial device to use to communcate.\nUsually /dev/cu.usbmodem12341 on macOS.")))
        .subcommand(SubCommand::with_name("test").about("Tests the game controller subsystem"))
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

    match matches.subcommand_name() {
        Some("ps2ce") => {
            let ps2ce_matches = matches.subcommand_matches("ps2ce").unwrap();
            let device_path = ps2ce_matches.value_of("device").unwrap();

            send_to_ps2_controller_emulator(&device_path,
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

fn send_to_ps2_controller_emulator(device_path: &str,
                                   sdl_context: sdl2::Sdl,
                                   game_controller_subsystem: sdl2::GameControllerSubsystem,
                                   active_controllers: &mut HashMap<i32, sdl2::controller::GameController>) -> io::Result<()> {
    println!("Connecting to PS2 Controller Emulator device at '{}'...", device_path);

    let mut serial = match serial::open(device_path) {
        Ok(serial) => serial,
        Err(error) => panic!("failed to open serial device: {}", error)
    };

    try!(serial.reconfigure(&|settings| {
        try!(settings.set_baud_rate(serial::Baud9600));
        settings.set_char_size(serial::Bits8);
        Ok(())
    }));

    println!("Connected!");

    let buf = vec!(
        0x5A, // DualShock Magic

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
    );

    try!(serial.write(&buf[..]));

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
