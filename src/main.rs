use std::collections::HashMap;
extern crate sdl2;

fn main() {
    let sdl_context = sdl2::init().unwrap();
    let game_controller_subsystem = sdl_context.game_controller().unwrap();

    // Load pre-set controller mappings (note that SDL will still read
    // others from the SDL_GAMECONTROLLERCONFIG environment variable)
    let controller_mappings =
        include_str!("../vendor/SDL_GameControllerDB/gamecontrollerdb.txt")
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'));

    for mapping in controller_mappings {
        match game_controller_subsystem.add_mapping(mapping) {
            Err(error) => panic!("failed to load mapping: {}", error),
            _ => (),
        }
    }

    // Keep track of the controllers we know of
    let mut active_controllers: HashMap<i32, sdl2::controller::GameController> = HashMap::new();

    // Look into controllers that were already connected at start-up
    let joystick_count =
        match game_controller_subsystem.num_joysticks() {
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
                },
                Err(error) => println!("could not initialise joystick {} as controller: {:?}", id, error),
            }
        }
    }

    println!("(There are {} controllers connected)", active_controllers.len());

    // Listen to SDL events!
    for event in sdl_context.event_pump().unwrap().wait_iter() {
        use sdl2::event::Event;

        match event {
            Event::ControllerDeviceAdded{ which, .. } => {
                match game_controller_subsystem.open(which as u32) {
                    Ok(controller) => {
                        let controller_id = &controller.instance_id();
                        if !active_controllers.contains_key(controller_id) {
                            println!("{} (#{}): connected", controller.name(), controller_id);
                            println!("(There are {} controllers connected)", active_controllers.len() + 1);
                            active_controllers.insert(*controller_id, controller);
                        }
                    },
                    Err(error) => println!("could not initialise connected joystick {}: {:?}", which, error),
                }
            },

            Event::ControllerAxisMotion{ which, axis, value, .. } => {
                println!("{} (#{}): {:?}: {}", active_controllers[&which].name(), which, axis, value);
            },

            Event::ControllerButtonDown{ which, button, .. } => {
                println!("{} (#{}): {:?}: down", active_controllers[&which].name(), which, button);
            },

            Event::ControllerButtonUp{ which, button, .. } => {
                println!("{} (#{}): {:?}: up", active_controllers[&which].name(), which, button);
            },

            Event::ControllerDeviceRemoved{ which, .. } => {
                println!("{} (#{}): disconnected", active_controllers[&which].name(), which);
                println!("(There are {} controllers connected)", active_controllers.len() - 1);
                active_controllers.remove(&which);
            },

            Event::ControllerDeviceRemapped{ which, .. } => {
                println!("{} (#{}) remapped!", active_controllers[&which].name(), which);
            },

            Event::Quit{..} => break,
            _ => (),
        }
    }
}
