extern crate sdl2;
use std::collections::HashMap;

// SDL Manager
// Structure for passing around access to the SDL Subsystems,
// and central place for setting up defaults

pub struct SDLManager {
    pub context: sdl2::Sdl,
    pub game_controller_subsystem: sdl2::GameControllerSubsystem,
    pub haptic_subsystem: sdl2::HapticSubsystem,
    pub active_controllers: HashMap<i32, sdl2::controller::GameController>,
}

impl SDLManager {
    pub fn init() -> SDLManager {
        // Initialise SDL2, and the game controller subsystem
        let context = sdl2::init().unwrap();
        let haptic_subsystem = context.haptic().unwrap();
        let game_controller_subsystem = context.game_controller().unwrap();

        // Load pre-set controller mappings (note that SDL will still read
        // others from the SDL_GAMECONTROLLERCONFIG environment variable)
        let controller_mappings = include_str!(
            "../vendor/SDL_GameControllerDB/gamecontrollerdb.txt"
        ).lines()
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
                        println!(
                            "could not initialise joystick {} as controller: {:?}",
                            id,
                            error
                        )
                    }
                }
            }
        }

        return SDLManager {
            context,
            game_controller_subsystem,
            haptic_subsystem,
            active_controllers,
        };
    }
}
